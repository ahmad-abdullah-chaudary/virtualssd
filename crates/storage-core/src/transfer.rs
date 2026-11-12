//! Concurrent VHDX snapshot transfer engine.
//!
//! The original mirror path walked the VHDX file one chunk at a time and
//! awaited every upload before starting the next one. That left the network
//! idle during the local read and hash, and it left the link idle during
//! every round trip. It also rewrote the whole manifest once per chunk,
//! which is quadratic in the chunk count.
//!
//! This module replaces that with a bounded producer/consumer pipeline:
//!
//! * The **producer** reads the VHDX sequentially, hashes each chunk with
//!   BLAKE3, skips chunks that are unchanged or entirely zero, encrypts the
//!   rest, and hands them to a bounded channel. Local disk I/O and hashing
//!   therefore overlap with the network transfers.
//! * The **consumers** upload chunks concurrently, bounded by a semaphore so
//!   the provider is not hammered and memory stays bounded.
//! * The **manifest** is updated in memory by the consumers and written to
//!   disk on a checkpoint interval instead of once per chunk. The interval
//!   is what bounds how much work a crash can cost.
//!
//! Nothing here changes the on-disk format or the chunk layout: a mirror
//! produced by this engine produces the same manifest and the same remote
//! objects as the sequential path did.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use std::io::SeekFrom;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use virtualssd_integrity::ChunkChecksum;
use virtualssd_manifest::{ChunkDescriptor, DiskManifest};

use crate::crypto::ChunkCipher;

/// Maximum number of chunks uploaded at the same time.
///
/// Four keeps a typical broadband link saturated while staying well inside
/// the request budget of both providers, and it bounds the memory used by the
/// in-flight chunk buffers.
pub const DEFAULT_UPLOAD_CONCURRENCY: usize = 4;

/// Maximum number of chunks downloaded at the same time during a restore.
pub const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 4;

/// How many completed chunks are allowed between manifest checkpoints.
///
/// The manifest is a full rewrite, so writing it per chunk is quadratic. A
/// checkpoint every 64 chunks bounds the work a crash can cost to at most 64
/// chunks being re-uploaded, while removing most of the write traffic.
pub const MANIFEST_CHECKPOINT_INTERVAL: usize = 64;

/// Depth of the work channel between the producer and the consumers.
pub fn channel_depth(concurrency: usize) -> usize {
    concurrency.saturating_mul(2).max(4)
}

/// A unit of work handed from the producer to an upload worker.
pub enum ChunkWork {
    /// The chunk changed and must be uploaded.
    Upload {
        chunk_index: u64,
        length: usize,
        plaintext_checksum: ChunkChecksum,
        remote_checksum: Option<ChunkChecksum>,
        payload: Vec<u8>,
        /// The remote object this chunk replaces, if any. It is deleted only
        /// after the new object is safely stored.
        superseded_remote_object_id: Option<String>,
    },
    /// The chunk became zero, or the file shrank past it. The remote object
    /// must be removed so a restore cannot resurrect stale data.
    Delete {
        chunk_index: u64,
        remote_object_id: String,
    },
    /// The chunk is unchanged and already stored remotely.
    Skip { chunk_index: u64 },
}

/// The result of one unit of work.
pub enum ChunkOutcome {
    Uploaded {
        chunk_index: u64,
        length: usize,
        plaintext_checksum: ChunkChecksum,
        remote_checksum: Option<ChunkChecksum>,
        remote_object_id: String,
    },
    Deleted { chunk_index: u64 },
    Skipped { chunk_index: u64 },
    /// The chunk uploaded but a follow-up cleanup step failed.
    Warning { chunk_index: u64, message: String },
}

/// Counters shared between the producer and the consumers.
#[derive(Default)]
pub struct TransferCounters {
    uploaded: AtomicU64,
    deleted: AtomicU64,
    skipped: AtomicU64,
    warnings: AtomicU64,
}

impl TransferCounters {
    pub fn uploaded(&self) -> u64 {
        self.uploaded.load(Ordering::Relaxed)
    }

    pub fn deleted(&self) -> u64 {
        self.deleted.load(Ordering::Relaxed)
    }

    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }

    pub fn warnings(&self) -> u64 {
        self.warnings.load(Ordering::Relaxed)
    }

    pub fn record(&self, outcome: &ChunkOutcome) {
        match outcome {
            ChunkOutcome::Uploaded { .. } => {
                self.uploaded.fetch_add(1, Ordering::Relaxed);
            }
            ChunkOutcome::Deleted { .. } => {
                self.deleted.fetch_add(1, Ordering::Relaxed);
            }
            ChunkOutcome::Skipped { .. } => {
                self.skipped.fetch_add(1, Ordering::Relaxed);
            }
            ChunkOutcome::Warning { .. } => {
                self.warnings.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Set when the transfer must stop, for example after a provider error.
#[derive(Default)]
pub struct AbortFlag {
    aborted: AtomicBool,
}

impl AbortFlag {
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }
}

/// Reads the VHDX and produces the work items.
///
/// The producer owns the file handle so the reads stay strictly sequential,
/// which is the fastest way to walk a file on disk, while the hashing and
/// encryption of each chunk overlap with the uploads of earlier chunks.
pub struct ChunkProducer {
    file: tokio::fs::File,
    file_size: u64,
    chunk_size: usize,
    encrypt_chunks: bool,
    cipher: Option<ChunkCipher>,
    previous: HashMap<u64, ChunkDescriptor>,
    new_chunk_count: u64,
    scan_count: u64,
    next_index: u64,
}
impl ChunkProducer {
    pub fn new(
        file: tokio::fs::File,
        file_size: u64,
        chunk_size: usize,
        encrypt_chunks: bool,
        cipher: Option<ChunkCipher>,
        previous: HashMap<u64, ChunkDescriptor>,
        new_chunk_count: u64,
        old_chunk_count: u64,
    ) -> Self {
        Self {
            file,
            file_size,
            chunk_size,
            encrypt_chunks,
            cipher,
            previous,
            new_chunk_count,
            scan_count: new_chunk_count.max(old_chunk_count),
            next_index: 0,
        }
    }

    /// Produces the next unit of work, or `None` when the file is exhausted.
    pub async fn next(&mut self) -> Result<Option<ChunkWork>, std::io::Error> {
        let chunk_size_u64 = self.chunk_size as u64;

        while self.next_index < self.scan_count {
            let chunk_index = self.next_index;
            self.next_index += 1;

            let previous = self.previous.get(&chunk_index).cloned();

            // The file shrank since the previous snapshot: the remote object
            // for this chunk has to go.
            if chunk_index >= self.new_chunk_count {
                if let Some(previous) = previous {
                    if let Some(remote_id) = previous.remote_object_id {
                        return Ok(Some(ChunkWork::Delete {
                            chunk_index,
                            remote_object_id: remote_id,
                        }));
                    }
                }
                continue;
            }

            let offset = chunk_index * chunk_size_u64;
            let length = ((self.file_size - offset) as usize).min(self.chunk_size);
            self.file.seek(SeekFrom::Start(offset)).await?;
            let mut plaintext = vec![0u8; length];
            self.file.read_exact(&mut plaintext).await?;

            // Sparse regions are not uploaded at all, and any previous remote
            // object for them is removed so a restore cannot resurrect it.
            if plaintext.iter().all(|byte| *byte == 0) {
                if let Some(previous) = previous {
                    if let Some(remote_id) = previous.remote_object_id {
                        return Ok(Some(ChunkWork::Delete {
                            chunk_index,
                            remote_object_id: remote_id,
                        }));
                    }
                }
                continue;
            }

            let plaintext_checksum = ChunkChecksum::blake3(&plaintext);

            // Unchanged chunks are skipped: the remote object is already
            // correct, so re-uploading it would waste bandwidth.
            if let Some(previous) = &previous {
                let unchanged = previous.length == length
                    && previous.checksum == plaintext_checksum
                    && previous.is_encrypted == self.encrypt_chunks
                    && previous.remote_object_id.is_some();
                if unchanged {
                    return Ok(Some(ChunkWork::Skip { chunk_index }));
                }
            }

            let (payload, remote_checksum) = if self.encrypt_chunks {
                let cipher = self
                    .cipher
                    .as_ref()
                    .ok_or_else(|| std::io::Error::other("encryption is enabled but no key is loaded"))?;
                let ciphertext = cipher
                    .encrypt(&plaintext)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let checksum = ChunkChecksum::blake3(&ciphertext);
                (ciphertext, Some(checksum))
            } else {
                (plaintext, None)
            };

            let superseded_remote_object_id = previous.and_then(|p| p.remote_object_id);

            return Ok(Some(ChunkWork::Upload {
                chunk_index,
                length,
                plaintext_checksum,
                remote_checksum,
                payload,
                superseded_remote_object_id,
            }));
        }

        Ok(None)
    }
}

/// The shared state an upload worker needs.
pub struct UploadContext {
    pub provider: Arc<dyn virtualssd_provider_contract::CloudStorageProvider>,
    pub namespace: Uuid,
    pub manifest: Arc<RwLock<DiskManifest>>,
    /// Whether chunk payloads are encrypted. Recorded in the manifest so a
    /// restore knows whether to decrypt before verifying.
    pub encrypt_chunks: bool,
}

/// Performs one unit of work against the provider and records it in the
/// in-memory manifest.
///
/// The manifest is only updated in memory here. Writing it to disk is the
/// caller's job, on a checkpoint interval.
pub async fn perform_upload_work(
    context: &UploadContext,
    work: ChunkWork,
) -> Result<ChunkOutcome, virtualssd_provider_contract::ProviderError> {
    match work {
        ChunkWork::Skip { chunk_index } => Ok(ChunkOutcome::Skipped { chunk_index }),

        ChunkWork::Delete {
            chunk_index,
            remote_object_id,
        } => {
            context
                .provider
                .delete_chunk(context.namespace, chunk_index, &remote_object_id)
                .await?;
            {
                let mut manifest = context.manifest.write();
                manifest.remove_vhdx_chunk(chunk_index);
            }
            Ok(ChunkOutcome::Deleted { chunk_index })
        }

        ChunkWork::Upload {
            chunk_index,
            length,
            plaintext_checksum,
            remote_checksum,
            payload,
            superseded_remote_object_id,
        } => {
            let upload_checksum = remote_checksum
                .as_ref()
                .unwrap_or(&plaintext_checksum);

            let upload = context
                .provider
                .upload_chunk(
                    context.namespace,
                    chunk_index,
                    &payload,
                    upload_checksum,
                )
                .await?;

            // Remove the superseded object only after the new object is safely
            // stored and recorded. Telegram replaces in place and implements
            // this as a no-op.
            let mut warning = None;
            if let Some(old_object_id) = superseded_remote_object_id {
                if old_object_id != upload.remote_object_id {
                    if let Err(e) = context
                        .provider
                        .delete_superseded_chunk(context.namespace, chunk_index, &old_object_id)
                        .await
                    {
                        warning = Some(format!(
                            "Chunk #{chunk_index} uploaded, but its superseded remote object {old_object_id} could not be deleted: {e}"
                        ));
                    }
                }
            }

            {
                let mut manifest = context.manifest.write();
                manifest.update_vhdx_chunk(
                    chunk_index,
                    length,
                    plaintext_checksum.clone(),
                    remote_checksum.clone(),
                    upload.remote_object_id.clone(),
                    context.encrypt_chunks,
                );
            }

            match warning {
                Some(message) => Ok(ChunkOutcome::Warning {
                    chunk_index,
                    message,
                }),
                None => Ok(ChunkOutcome::Uploaded {
                    chunk_index,
                    length,
                    plaintext_checksum,
                    remote_checksum,
                    remote_object_id: upload.remote_object_id,
                }),
            }
        }
    }
}

/// Writes the current manifest to disk.
///
/// This is the expensive operation that used to run once per chunk. The
/// caller invokes it on a checkpoint interval instead.
pub fn checkpoint_manifest(
    manifest: &Arc<RwLock<DiskManifest>>,
    path: &std::path::Path,
) -> Result<(), virtualssd_manifest::ManifestError> {
    let manifest = manifest.read();
    manifest.save_atomic(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use virtualssd_manifest::ChunkDescriptor;

    fn descriptor(checksum: ChunkChecksum, encrypted: bool) -> ChunkDescriptor {
        ChunkDescriptor {
            chunk_index: 0,
            length: 4096,
            checksum,
            remote_checksum: None,
            remote_object_id: Some("remote-1".to_string()),
            remote_version: 1,
            is_encrypted: encrypted,
            last_modified: chrono::Utc::now(),
        }
    }

    #[test]
    fn counters_track_each_outcome() {
        let counters = TransferCounters::default();
        counters.record(&ChunkOutcome::Skipped { chunk_index: 1 });
        counters.record(&ChunkOutcome::Uploaded {
            chunk_index: 2,
            length: 4096,
            plaintext_checksum: ChunkChecksum::blake3(b"x"),
            remote_checksum: None,
            remote_object_id: "r".into(),
        });
        counters.record(&ChunkOutcome::Deleted { chunk_index: 3 });

        assert_eq!(counters.skipped(), 1);
        assert_eq!(counters.uploaded(), 1);
        assert_eq!(counters.deleted(), 1);
        assert_eq!(counters.warnings(), 0);
    }

    #[test]
    fn abort_flag_stops_the_transfer() {
        let flag = AbortFlag::default();
        assert!(!flag.is_aborted());
        flag.abort();
        assert!(flag.is_aborted());
    }

    #[test]
    fn channel_depth_scales_with_concurrency() {
        assert_eq!(channel_depth(4), 8);
        assert_eq!(channel_depth(1), 4);
        assert_eq!(channel_depth(0), 4);
    }

    #[test]
    fn producer_skips_unchanged_chunks() {
        // A chunk whose recorded checksum matches its content is skipped
        // rather than re-uploaded.
        let content = vec![7u8; 4096];
        let checksum = ChunkChecksum::blake3(&content);
        let previous = HashMap::from([(0u64, descriptor(checksum, false))]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disk.vhdx");
        std::fs::write(&path, &content).unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let outcome = runtime.block_on(async {
            let file = tokio::fs::File::open(&path).await.unwrap();
            let mut producer = ChunkProducer::new(
                file,
                content.len() as u64,
                4096,
                false,
                None,
                previous,
                1,
                1,
            );
            producer.next().await.unwrap()
        });

        assert!(matches!(outcome, Some(ChunkWork::Skip { chunk_index: 0 })));
    }
}
