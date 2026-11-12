//! VirtualSSD Core Storage Engine.
//!
//! Integrates block slicing, persistent caching, write-ahead journaling,
//! cloud synchronization, cryptographic integrity, and Windows block device lifecycle.

pub mod crypto;
pub mod windows_vhd;

use crypto::{ChunkCipher, CryptoError};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;
use virtualssd_block_map::{slice_io, BlockMapError};
use virtualssd_cache_manager::{CacheError, ChunkState, PersistentCacheManager};
use virtualssd_integrity::{ChunkChecksum, IntegrityError};
use virtualssd_journal::{JournalError, WriteAheadJournal};
use virtualssd_manifest::{ChunkDescriptor, DiskManifest, ManifestError};
use virtualssd_provider_contract::{CloudStorageProvider, ProviderError};

pub mod transfer;
use transfer::{
    checkpoint_manifest, AbortFlag, ChunkOutcome, ChunkProducer, ChunkWork, SnapshotLayout,
    TransferCounters, UploadContext, MANIFEST_CHECKPOINT_INTERVAL,
};

#[derive(Debug, Error)]
pub enum StorageEngineError {
    #[error("Block mapping error: {0}")]
    BlockMap(#[from] BlockMapError),

    #[error("Manifest error: {0}")]
    Manifest(#[from] ManifestError),

    #[error("Journal error: {0}")]
    Journal(#[from] JournalError),

    #[error("Cache error: {0}")]
    Cache(#[from] CacheError),

    #[error("Integrity error: {0}")]
    Integrity(#[from] IntegrityError),

    #[error("Cloud provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Offline and uncached chunk {0}: data cannot be read without network")]
    OfflineUncached(u64),

    #[error("Disk is currently in Read-Only or Unmounted state")]
    ReadOnly,

    #[error("Windows storage device error: {0}")]
    WindowsDevice(String),

    #[error("Chunk encryption error: {0}")]
    Crypto(#[from] CryptoError),

    #[error("The disk is mounted. Dismount it before syncing so the VHDX snapshot is consistent.")]
    DiskMounted,

    #[error("This disk has no cloud provider bound to it")]
    NoProvider,

    #[error("No local VHDX exists and the remote manifest contains no VHDX snapshot")]
    NoVhdxSnapshot,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiskStatus {
    Connecting,
    Online,
    Degraded,
    Offline,
    Recovering,
    ReadOnly,
    Unmounted,
    NeedsAttention,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskTelemetry {
    pub disk_id: Uuid,
    pub name: String,
    pub status: DiskStatus,
    pub capacity_bytes: u64,
    pub total_chunks: u64,
    pub allocated_chunks: u64,
    /// Allocated chunks whose manifest entry references a remote object.
    pub committed_chunks: u64,
    pub cached_bytes: u64,
    pub dirty_bytes: u64,
    pub pending_sync_count: usize,
    pub drive_letter: Option<String>,
    pub physical_drive_path: Option<String>,
    pub provider_name: String,
    pub last_error: Option<String>,
    /// Size of the VHDX file at the last successful snapshot sync.
    pub vhdx_file_size: u64,
    /// Number of VHDX file chunks currently present in the manifest.
    pub vhdx_chunks: u64,
    /// Number of VHDX file chunks whose remote object id is recorded.
    pub mirrored_chunks: u64,
    /// Whether this disk encrypts chunk payloads before upload.
    pub encryption_enabled: bool,
    /// Whether the DPAPI-protected key for this encrypted disk is present.
    pub encryption_key_present: bool,
}

/// Result of mirroring the local VHDX file to the bound provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorReport {
    pub scanned_chunks: usize,
    pub uploaded_chunks: usize,
    pub deleted_chunks: usize,
    pub skipped_chunks: usize,
    pub file_size_bytes: u64,
    pub commit_version: u64,
    pub warnings: Vec<String>,
}

/// Result of restoring a VHDX file from remote chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreReport {
    pub restored: bool,
    pub downloaded_chunks: usize,
    pub file_size_bytes: u64,
}

/// Result of deleting every remote chunk and the published manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDeleteReport {
    pub deleted_chunks: usize,
    pub deleted_manifest: bool,
}

/// Provider namespace used for VHDX file chunks. Keeping it distinct from the
/// disk id prevents VHDX chunks from ever colliding with block-engine chunks.
pub fn vhdx_namespace_id(disk_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("{disk_id}:vhdx-file-v1").as_bytes(),
    )
}

/// Reads exactly `length` bytes at `offset` from a file. Short reads produce
/// `UnexpectedEof` rather than a silently padded buffer.
fn read_file_range(path: &Path, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0u8; length];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// Result of a local-cache BLAKE3 audit. Remote chunks are not downloaded
/// during verification, so `missing_chunks` means "not present locally" and
/// must not be reported as corruption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityAudit {
    pub verified_chunks: usize,
    pub missing_chunks: usize,
    pub total_chunks: usize,
    pub errors: Vec<(u64, String)>,
}

impl IntegrityAudit {
    pub fn passed(&self) -> bool {
        self.errors.is_empty()
    }
}

pub struct VirtualDiskEngine {
    manifest: Arc<RwLock<DiskManifest>>,
    manifest_path: PathBuf,
    disk_dir: PathBuf,
    journal: RwLock<WriteAheadJournal>,
    cache: Arc<PersistentCacheManager>,
    provider: Option<Arc<dyn CloudStorageProvider>>,
    status: RwLock<DiskStatus>,
    drive_letter: RwLock<Option<String>>,
    physical_drive_path: RwLock<Option<String>>,
    last_error: RwLock<Option<String>>,
    /// Maximum number of chunk transfers that run at the same time.
    upload_concurrency: usize,
    /// Maximum number of chunk downloads that run at the same time during a
    /// restore.
    download_concurrency: usize,
}

impl VirtualDiskEngine {
    pub fn open(
        disk_dir: impl AsRef<Path>,
        manifest: DiskManifest,
        provider: Option<Arc<dyn CloudStorageProvider>>,
        max_cache_bytes: u64,
    ) -> Result<Self, StorageEngineError> {
        let disk_dir = disk_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&disk_dir).map_err(ManifestError::Io)?;

        let manifest_path = disk_dir.join("manifest.json");
        manifest.save_atomic(&manifest_path)?;

        let wal_path = disk_dir.join("wal.log");
        let journal = WriteAheadJournal::open(&wal_path)?;

        let cache_dir = disk_dir.join("cache");
        let cache = Arc::new(PersistentCacheManager::new(
            cache_dir,
            max_cache_bytes,
            manifest.geometry.chunk_size,
        )?);

        let engine = Self {
            manifest: Arc::new(RwLock::new(manifest)),
            manifest_path,
            disk_dir,
            journal: RwLock::new(journal),
            cache,
            provider,
            status: RwLock::new(DiskStatus::Unmounted),
            drive_letter: RwLock::new(None),
            physical_drive_path: RwLock::new(None),
            last_error: RwLock::new(None),
            upload_concurrency: transfer::DEFAULT_UPLOAD_CONCURRENCY,
            download_concurrency: transfer::DEFAULT_DOWNLOAD_CONCURRENCY,
        };

        // Run crash recovery replay
        engine.recover_after_crash()?;

        Ok(engine)
    }

    /// Idempotent recovery after unexpected shutdown or crash
    pub fn recover_after_crash(&self) -> Result<(), StorageEngineError> {
        *self.status.write() = DiskStatus::Recovering;
        let journal = self.journal.write();
        let report = journal.scan_recovery()?;

        if !report.replayed_writes.is_empty() {
            tracing::info!(
                "Replaying {} uncommitted journal writes...",
                report.replayed_writes.len()
            );
            for write in &report.replayed_writes {
                self.cache.write_chunk_slice(
                    write.chunk_index,
                    write.offset_in_chunk,
                    &write.data,
                )?;
            }
        }

        // A disk whose journal was just scanned is not attached to Windows.
        // It becomes Online only after a real VHDX is attached and formatted.
        *self.status.write() = DiskStatus::Unmounted;
        Ok(())
    }

    /// Reads sectors spanning arbitrary chunk boundaries
    pub async fn read_blocks(
        &self,
        offset: u64,
        buffer: &mut [u8],
    ) -> Result<(), StorageEngineError> {
        let geometry = self.manifest.read().geometry;
        let slices = slice_io(&geometry, offset, buffer.len())?;

        for slice in slices {
            let chunk_idx = slice.chunk_index;
            let slice_buf = &mut buffer[slice.buffer_offset..slice.buffer_offset + slice.length];

            // 1. Try reading from persistent cache
            if self.cache.is_cached(chunk_idx) {
                self.cache
                    .read_chunk_slice(chunk_idx, slice.chunk_offset, slice_buf)?;
                continue;
            }

            // 2. Check if chunk is sparse (unallocated)
            let is_allocated = self.manifest.read().chunks.contains_key(&chunk_idx);
            if !is_allocated {
                // Unwritten sparse region returns zeros
                slice_buf.fill(0);
                continue;
            }

            // 3. Allocated in manifest, but not in cache: fetch from cloud provider
            let desc = self
                .manifest
                .read()
                .chunks
                .get(&chunk_idx)
                .cloned()
                .ok_or(StorageEngineError::OfflineUncached(chunk_idx))?;

            if let (Some(provider), Some(remote_id)) = (&self.provider, &desc.remote_object_id) {
                let disk_id = self.manifest.read().disk_id;
                match provider
                    .download_chunk(disk_id, chunk_idx, remote_id, &desc.checksum)
                    .await
                {
                    Ok(data) => {
                        self.cache
                            .put_chunk_bytes(chunk_idx, &data, ChunkState::Clean)?;
                        self.cache
                            .read_chunk_slice(chunk_idx, slice.chunk_offset, slice_buf)?;
                    }
                    Err(e) => {
                        *self.last_error.write() =
                            Some(format!("Failed to fetch chunk #{chunk_idx}: {e}"));
                        return Err(StorageEngineError::Provider(e));
                    }
                }
            } else {
                return Err(StorageEngineError::OfflineUncached(chunk_idx));
            }
        }

        Ok(())
    }

    /// Writes sectors spanning arbitrary chunk boundaries with WAL guarantees
    pub async fn write_blocks(&self, offset: u64, data: &[u8]) -> Result<(), StorageEngineError> {
        let status = *self.status.read();
        if status == DiskStatus::ReadOnly || status == DiskStatus::Unmounted {
            return Err(StorageEngineError::ReadOnly);
        }

        let geometry = self.manifest.read().geometry;
        let slices = slice_io(&geometry, offset, data.len())?;

        // 1. Begin durable WAL transaction
        let tx_id = self.journal.write().begin_transaction()?;

        // 2. Persist writes into WAL
        for slice in &slices {
            let slice_data = &data[slice.buffer_offset..slice.buffer_offset + slice.length];
            self.journal.write().record_chunk_write(
                tx_id,
                slice.chunk_index,
                slice.chunk_offset,
                slice_data,
            )?;
        }

        // 3. Commit WAL transaction to local disk with flush
        self.journal.write().commit_transaction(tx_id)?;

        // 4. Update persistent cache (marked DIRTY)
        for slice in &slices {
            let slice_data = &data[slice.buffer_offset..slice.buffer_offset + slice.length];
            self.cache
                .write_chunk_slice(slice.chunk_index, slice.chunk_offset, slice_data)?;
        }

        Ok(())
    }

    /// Synchronizes all dirty chunks to remote cloud provider
    pub async fn sync_dirty_chunks(&self) -> Result<usize, StorageEngineError> {
        let provider = match &self.provider {
            Some(p) => p.clone(),
            None => return Ok(0),
        };
        // Remember whether the volume was actually attached. A failed sync must
        // never make an unmounted disk look Online after a retry succeeds.
        let was_mounted = self.physical_drive_path.read().is_some();

        let dirty = self.cache.get_dirty_chunks();
        let disk_id = self.manifest.read().disk_id;
        let mut synced_count = 0;

        for chunk_idx in dirty {
            let chunk_bytes = match self.cache.get_chunk_bytes(chunk_idx) {
                Ok(b) => b,
                Err(_) => continue,
            };

            let checksum = ChunkChecksum::blake3(&chunk_bytes);

            match provider
                .upload_chunk(disk_id, chunk_idx, &chunk_bytes, &checksum)
                .await
            {
                Ok(upload_res) => {
                    // Update manifest
                    {
                        let mut manifest = self.manifest.write();
                        manifest.update_chunk(
                            chunk_idx,
                            chunk_bytes.len(),
                            checksum,
                            Some(upload_res.remote_object_id),
                            false,
                        );
                        manifest.save_atomic(&self.manifest_path)?;
                    }

                    // Mark chunk committed in cache
                    self.cache.mark_committed(chunk_idx);
                    synced_count += 1;
                }
                Err(e) => {
                    *self.last_error.write() = Some(format!("Sync error chunk #{chunk_idx}: {e}"));
                    *self.status.write() = DiskStatus::Degraded;
                    return Err(StorageEngineError::Provider(e));
                }
            }
        }

        // Publish the authoritative manifest even when no chunks were uploaded:
        // this retries a previous publish failure and makes the disk
        // discoverable on a fresh install.
        let manifest_snapshot = self.manifest_snapshot();
        if let Err(e) = provider.publish_manifest(&manifest_snapshot).await {
            *self.last_error.write() = Some(format!("Manifest publish failed: {e}"));
            *self.status.write() = DiskStatus::Degraded;
            return Err(StorageEngineError::Provider(e));
        }

        // Safely checkpoint WAL. A sync does not mount a disk: preserve the
        // actual mount state instead of falsely reporting Online.
        let _ = self.journal.write().checkpoint_truncate();
        *self.status.write() = if was_mounted {
            DiskStatus::Online
        } else {
            DiskStatus::Unmounted
        };

        Ok(synced_count)
    }

    /// Mirrors the local VHDX file to the bound provider as a chunked
    /// snapshot. The disk must be unmounted so Windows has flushed and
    /// released the file. Chunks whose content is unchanged are skipped;
    /// chunks that became zero are removed from the remote store.
    pub async fn sync_vhdx_snapshot(&self) -> Result<MirrorReport, StorageEngineError> {
        if *self.status.read() == DiskStatus::Online || self.physical_drive_path.read().is_some() {
            return Err(StorageEngineError::DiskMounted);
        }

        let provider = self
            .provider
            .clone()
            .ok_or(StorageEngineError::NoProvider)?;

        let vhdx_path = self.vhdx_path();
        if !vhdx_path.exists() {
            if self.manifest.read().vhdx_chunks.is_empty() {
                return Err(StorageEngineError::NoVhdxSnapshot);
            }
            self.restore_vhdx_from_remote().await?;
        }

        let (disk_id, chunk_size, encrypt_chunks) = {
            let manifest = self.manifest.read();
            (
                manifest.disk_id,
                manifest.geometry.chunk_size,
                manifest.encrypt_chunks,
            )
        };

        let cipher = if encrypt_chunks {
            let key = crypto::load_key(&self.disk_dir)?.ok_or(CryptoError::MissingKey)?;
            Some(ChunkCipher::new(key))
        } else {
            None
        };

        let file_size = tokio::fs::metadata(&vhdx_path).await?.len();
        if file_size == 0 {
            return Err(StorageEngineError::WindowsDevice(format!(
                "The VHDX file {} is empty; refusing to publish an empty snapshot",
                vhdx_path.display()
            )));
        }

        let namespace = vhdx_namespace_id(disk_id);
        let chunk_size_u64 = chunk_size as u64;
        let new_chunk_count = file_size.div_ceil(chunk_size_u64);
        let old_chunk_count = self
            .manifest
            .read()
            .vhdx_chunks
            .keys()
            .next_back()
            .map(|index| index + 1)
            .unwrap_or(0);
        let scan_count = new_chunk_count.max(old_chunk_count);

        tracing::info!(
            "Scanning the VHDX snapshot of disk {disk_id}: {file_size} byte(s) in {scan_count} chunk slot(s), encryption: {encrypt_chunks}"
        );

        let file = tokio::fs::File::open(&vhdx_path).await?;
        let previous_chunks: HashMap<u64, ChunkDescriptor> = self
            .manifest
            .read()
            .vhdx_chunks
            .iter()
            .map(|(index, descriptor)| (*index, descriptor.clone()))
            .collect();

        let concurrency = self.upload_concurrency.max(1);
        let manifest = self.manifest.clone();
        let context = UploadContext {
            provider: provider.clone(),
            namespace,
            manifest: manifest.clone(),
            encrypt_chunks,
        };

        tracing::info!(
            "Mirroring disk {disk_id} with up to {concurrency} concurrent chunk transfers"
        );

        let counters = Arc::new(TransferCounters::default());
        let abort = Arc::new(AbortFlag::default());
        let warnings = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));

        // The producer runs in its own task so the dispatcher below can
        // consume the channel while the file is still being read.
        let (work_tx, mut work_rx) = tokio::sync::mpsc::channel::<ChunkWork>(concurrency * 2);
        let producer = {
            let abort = abort.clone();
            // The cipher is moved into the producer task rather than cloned:
            // the disk key must not be duplicated in memory.
            tokio::spawn(async move {
                let mut producer = ChunkProducer::new(
                    file,
                    SnapshotLayout {
                        file_size,
                        chunk_size,
                        encrypt_chunks,
                        cipher,
                        previous: previous_chunks,
                        new_chunk_count,
                        old_chunk_count,
                    },
                );
                loop {
                    if abort.is_aborted() {
                        break;
                    }
                    let next = match producer.next().await {
                        Ok(Some(work)) => work,
                        Ok(None) => break,
                        Err(e) => {
                            abort.abort();
                            return Err(StorageEngineError::Io(e));
                        }
                    };
                    if work_tx.send(next).await.is_err() {
                        // The dispatcher stopped; nothing more to produce.
                        break;
                    }
                }
                Ok(())
            })
        };

        let mut upload_tasks = tokio::task::JoinSet::new();
        let mut first_error: Option<StorageEngineError> = None;
        let mut completed_since_checkpoint = 0usize;

        // The semaphore bounds how many chunk transfers are in flight. It is
        // the difference between saturating the link and flooding the
        // provider, and it keeps the in-flight chunk buffers bounded.
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

        while let Some(work) = work_rx.recv().await {
            if abort.is_aborted() {
                break;
            }

            // Backpressure: the dispatcher waits for a free slot before
            // queueing more work, so the producer cannot run ahead.
            let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                StorageEngineError::WindowsDevice(
                    "The transfer semaphore was closed unexpectedly".into(),
                )
            })?;

            let context = Arc::new(UploadContext {
                provider: context.provider.clone(),
                namespace: context.namespace,
                manifest: context.manifest.clone(),
                encrypt_chunks: context.encrypt_chunks,
            });
            let counters = counters.clone();
            let abort = abort.clone();
            let warnings = warnings.clone();

            upload_tasks.spawn(async move {
                // The permit is released when this task finishes, whether it
                // succeeded or failed.
                let _permit = permit;

                let outcome = match transfer::perform_upload_work(&context, work).await {
                    Ok(outcome) => outcome,
                    Err(e) => {
                        abort.abort();
                        return Err(e);
                    }
                };

                counters.record(&outcome);
                if let ChunkOutcome::Warning { message, .. } = &outcome {
                    warnings.lock().push(message.clone());
                }
                Ok::<(), ProviderError>(())
            });

            // Reap finished uploads and checkpoint the manifest on an interval
            // instead of once per chunk.
            while let Some(result) = upload_tasks.try_join_next() {
                match result {
                    Ok(Ok(())) => {
                        completed_since_checkpoint += 1;
                        if completed_since_checkpoint >= MANIFEST_CHECKPOINT_INTERVAL {
                            completed_since_checkpoint = 0;
                            if let Err(e) = checkpoint_manifest(&manifest, &self.manifest_path) {
                                tracing::warn!(
                                    "Could not checkpoint the manifest for disk {disk_id}: {e}"
                                );
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        if first_error.is_none() {
                            first_error = Some(StorageEngineError::Provider(e));
                        }
                    }
                    Err(join_error) => {
                        if first_error.is_none() {
                            first_error = Some(StorageEngineError::WindowsDevice(format!(
                                "An upload task failed: {join_error}"
                            )));
                        }
                    }
                }
            }
        }

        // The producer may still be running; wait for it so the file handle
        // and the channel are closed cleanly.
        match producer.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(join_error) => {
                if first_error.is_none() {
                    first_error = Some(StorageEngineError::WindowsDevice(format!(
                        "The chunk producer failed: {join_error}"
                    )));
                }
            }
        }

        // Drain the remaining upload tasks.
        while let Some(result) = upload_tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_error.is_none() {
                        first_error = Some(StorageEngineError::Provider(e));
                    }
                }
                Err(join_error) => {
                    if first_error.is_none() {
                        first_error = Some(StorageEngineError::WindowsDevice(format!(
                            "An upload task failed: {join_error}"
                        )));
                    }
                }
            }
        }

        if let Some(e) = first_error {
            *self.last_error.write() = Some(e.to_string());
            *self.status.write() = DiskStatus::Degraded;
            // Persist whatever did complete so a retry does not re-upload it.
            let _ = checkpoint_manifest(&manifest, &self.manifest_path);
            return Err(e);
        }

        {
            let mut manifest = manifest.write();
            manifest.set_vhdx_file_size(file_size);
        }
        checkpoint_manifest(&manifest, &self.manifest_path)?;

        // Write out any provider-side state that was kept lazily during the
        // transfer, so nothing is lost when the transfer ends.
        if let Err(e) = provider.flush_transfer_state().await {
            tracing::warn!(
                "The provider could not flush its transfer state for disk {disk_id}: {e}"
            );
        }

        let snapshot = self.manifest_snapshot();
        if let Err(e) = provider.publish_manifest(&snapshot).await {
            *self.last_error.write() = Some(format!("Manifest publish failed: {e}"));
            *self.status.write() = DiskStatus::Degraded;
            tracing::error!(
                "The manifest for disk {disk_id} could not be published after the snapshot upload: {e}"
            );
            return Err(StorageEngineError::Provider(e));
        }

        *self.status.write() = DiskStatus::Unmounted;
        *self.last_error.write() = None;

        let mut report = MirrorReport {
            scanned_chunks: scan_count as usize,
            uploaded_chunks: counters.uploaded() as usize,
            deleted_chunks: counters.deleted() as usize,
            skipped_chunks: counters.skipped() as usize,
            file_size_bytes: file_size,
            commit_version: snapshot.commit_version,
            warnings: warnings.lock().clone(),
        };
        if report.warnings.is_empty() {
            report.warnings = Vec::new();
        }

        tracing::info!(
            "Published the manifest for disk {disk_id} at commit version {}",
            snapshot.commit_version
        );
        Ok(report)
    }

    /// Restores a missing VHDX file from the remote chunk snapshot. The file is
    /// written to a temporary path and only renamed into place after every
    /// chunk has been downloaded, decrypted, and checksum-verified.
    pub async fn restore_vhdx_from_remote(&self) -> Result<RestoreReport, StorageEngineError> {
        let vhdx_path = self.vhdx_path();
        if vhdx_path.exists() {
            let file_size = tokio::fs::metadata(&vhdx_path).await?.len();
            return Ok(RestoreReport {
                restored: false,
                downloaded_chunks: 0,
                file_size_bytes: file_size,
            });
        }

        let provider = self
            .provider
            .clone()
            .ok_or(StorageEngineError::NoProvider)?;
        let manifest = self.manifest_snapshot();
        let disk_id = manifest.disk_id;
        let file_size = manifest
            .vhdx_file_size
            .ok_or(StorageEngineError::NoVhdxSnapshot)?;

        if file_size == 0 || file_size > manifest.geometry.total_capacity_bytes + 64 * 1024 * 1024 {
            return Err(StorageEngineError::Manifest(ManifestError::CorruptManifest(
                format!(
                    "Remote manifest reports an implausible VHDX size of {file_size} bytes for a {} byte disk",
                    manifest.geometry.total_capacity_bytes
                ),
            )));
        }

        let chunk_size = manifest.geometry.chunk_size as u64;
        let expected_chunks = file_size.div_ceil(chunk_size);
        if let Some(last) = manifest.vhdx_chunks.keys().next_back() {
            if *last >= expected_chunks {
                return Err(StorageEngineError::Manifest(ManifestError::CorruptManifest(
                    format!(
                        "Remote manifest contains VHDX chunk #{last} beyond the snapshot size ({expected_chunks} chunks)"
                    ),
                )));
            }
        }

        let cipher = if manifest.encrypt_chunks {
            let key = crypto::load_key(&self.disk_dir)?.ok_or(CryptoError::MissingKey)?;
            Some(Arc::new(ChunkCipher::new(key)))
        } else {
            None
        };

        let temp_path = vhdx_path.with_extension("restore.tmp");
        let _ = tokio::fs::remove_file(&temp_path).await;
        let namespace = vhdx_namespace_id(manifest.disk_id);

        tracing::info!(
            "Restoring disk {disk_id} from the provider: {file_size} byte(s) across {} chunk(s)",
            manifest.vhdx_chunks.len()
        );

        // Chunks are downloaded concurrently and written straight to their
        // offset in the temporary file. The file is preallocated, so a chunk
        // can land at any offset without a second pass.
        let concurrency = self.download_concurrency.max(1);
        tracing::info!(
            "Restoring disk {disk_id} with up to {concurrency} concurrent chunk downloads"
        );

        let result: Result<usize, StorageEngineError> = async {
            let file = tokio::fs::File::create(&temp_path).await?;
            file.set_len(file_size).await?;
            let file = Arc::new(tokio::sync::Mutex::new(file));

            let abort = Arc::new(AbortFlag::default());
            let downloaded = Arc::new(AtomicU64::new(0));
            let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

            let mut tasks = tokio::task::JoinSet::new();

            for (chunk_index, descriptor) in &manifest.vhdx_chunks {
                if abort.is_aborted() {
                    break;
                }

                // Backpressure: bound how many downloads are in flight.
                let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                    StorageEngineError::WindowsDevice(
                        "The restore semaphore was closed unexpectedly".into(),
                    )
                })?;

                let remote_id = descriptor.remote_object_id.clone().ok_or_else(|| {
                    StorageEngineError::Manifest(ManifestError::CorruptManifest(format!(
                        "VHDX chunk #{chunk_index} has no remote object id"
                    )))
                })?;
                let remote_checksum = descriptor
                    .remote_checksum
                    .as_ref()
                    .unwrap_or(&descriptor.checksum)
                    .clone();
                let expected_checksum = descriptor.checksum.clone();
                let expected_length = descriptor.length;
                let is_encrypted = descriptor.is_encrypted;

                let provider = provider.clone();
                let file = file.clone();
                let abort = abort.clone();
                let downloaded = downloaded.clone();
                let chunk_index = *chunk_index;
                // The cipher is shared, not cloned: the disk key must not be
                // duplicated for every in-flight download.
                let cipher = cipher.clone();

                tasks.spawn(async move {
                    // The permit is released when this task finishes.
                    let _permit = permit;

                    let payload = match provider
                        .download_chunk(namespace, chunk_index, &remote_id, &remote_checksum)
                        .await
                    {
                        Ok(payload) => payload,
                        Err(e) => {
                            abort.abort();
                            return Err(StorageEngineError::Provider(e));
                        }
                    };

                    let plaintext = if is_encrypted {
                        let cipher = cipher
                            .as_ref()
                            .ok_or(CryptoError::MissingKey)?
                            .clone();
                        match cipher.decrypt(&payload) {
                            Ok(plaintext) => plaintext,
                            Err(e) => {
                                abort.abort();
                                return Err(StorageEngineError::Crypto(e));
                            }
                        }
                    } else {
                        payload
                    };

                    expected_checksum
                        .verify(&plaintext)
                        .map_err(StorageEngineError::Integrity)?;

                    if plaintext.len() != expected_length {
                        abort.abort();
                        return Err(StorageEngineError::Manifest(
                            ManifestError::CorruptManifest(format!(
                                "VHDX chunk #{chunk_index} length mismatch: manifest says {expected_length}, downloaded {}",
                                plaintext.len()
                            )),
                        ));
                    }

                    let offset = chunk_index * chunk_size;
                    let mut guard = file.lock().await;
                    guard.seek(SeekFrom::Start(offset)).await?;
                    guard.write_all(&plaintext).await?;
                    drop(guard);

                    downloaded.fetch_add(1, Ordering::Relaxed);
                    Ok::<(), StorageEngineError>(())
                });
            }

            let mut first_error: Option<StorageEngineError> = None;
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                    }
                    Err(join_error) => {
                        if first_error.is_none() {
                            first_error = Some(StorageEngineError::WindowsDevice(format!(
                                "A download task failed: {join_error}"
                            )));
                        }
                    }
                }
            }

            if let Some(e) = first_error {
                return Err(e);
            }

            // Flush the file to disk before it is renamed into place.
            let guard = file.lock().await;
            guard.sync_all().await?;
            drop(guard);

            let written = tokio::fs::metadata(&temp_path).await?.len();
            if written != file_size {
                return Err(StorageEngineError::WindowsDevice(format!(
                    "Restored VHDX has {written} bytes, expected {file_size}"
                )));
            }
            Ok(downloaded.load(Ordering::Relaxed) as usize)
        }
        .await;

        let downloaded = match result {
            Ok(downloaded) => downloaded,
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                *self.last_error.write() = Some(format!("VHDX restore failed: {e}"));
                tracing::error!("The VHDX restore for disk {disk_id} failed: {e}");
                return Err(e);
            }
        };

        if let Err(e) = tokio::fs::rename(&temp_path, &vhdx_path).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            tracing::error!(
                "The restored VHDX for disk {disk_id} could not be moved into place: {e}"
            );
            return Err(StorageEngineError::Io(e));
        }

        tracing::info!(
            "Restored the VHDX for disk {disk_id}: {downloaded} chunk(s) downloaded and verified, {file_size} byte(s)"
        );
        *self.last_error.write() = None;
        Ok(RestoreReport {
            restored: true,
            downloaded_chunks: downloaded,
            file_size_bytes: file_size,
        })
    }

    /// Deletes every remote object referenced by this disk's manifest and then
    /// deletes the published manifest. Objects are removed before the local
    /// manifest is cleared, and any failure aborts so a retry remains possible.
    pub async fn delete_remote_data(&self) -> Result<RemoteDeleteReport, StorageEngineError> {
        if *self.status.read() == DiskStatus::Online || self.physical_drive_path.read().is_some() {
            return Err(StorageEngineError::DiskMounted);
        }

        let provider = self
            .provider
            .clone()
            .ok_or(StorageEngineError::NoProvider)?;
        let manifest = self.manifest_snapshot();
        let namespace = vhdx_namespace_id(manifest.disk_id);
        let mut deleted_chunks = 0usize;

        for (chunk_index, descriptor) in &manifest.vhdx_chunks {
            if let Some(remote_id) = &descriptor.remote_object_id {
                provider
                    .delete_chunk(namespace, *chunk_index, remote_id)
                    .await?;
                deleted_chunks += 1;
            }
        }

        for (chunk_index, descriptor) in &manifest.chunks {
            if let Some(remote_id) = &descriptor.remote_object_id {
                provider
                    .delete_chunk(manifest.disk_id, *chunk_index, remote_id)
                    .await?;
                deleted_chunks += 1;
            }
        }

        provider.delete_manifest(manifest.disk_id).await?;

        {
            let mut local = self.manifest.write();
            local.clear_remote_chunks();
            local.save_atomic(&self.manifest_path)?;
        }

        tracing::info!(
            "Deleted {deleted_chunks} remote chunk(s) and the published manifest for disk {}",
            manifest.disk_id
        );
        *self.last_error.write() = None;
        Ok(RemoteDeleteReport {
            deleted_chunks,
            deleted_manifest: true,
        })
    }

    pub fn provider(&self) -> Option<Arc<dyn CloudStorageProvider>> {
        self.provider.clone()
    }

    pub fn encryption_enabled(&self) -> bool {
        self.manifest.read().encrypt_chunks
    }

    /// True when a DPAPI-protected key file exists for this disk.
    pub fn encryption_key_present(&self) -> bool {
        crypto::key_path(&self.disk_dir).exists()
    }

    /// Applies a new cache ceiling to the running engine. Clean and committed
    /// chunks are evicted immediately if the new limit is lower.
    pub fn set_cache_limit(&self, max_bytes: u64) -> Result<(), StorageEngineError> {
        self.cache.set_max_bytes(max_bytes)?;
        Ok(())
    }

    /// Creates a recovery key for a new encrypted disk. Must only be called
    /// when the disk has no mirrored chunks yet.
    pub fn initialize_encryption_key(&self) -> Result<(), StorageEngineError> {
        crypto::ensure_key(&self.disk_dir)?;
        Ok(())
    }

    pub fn export_recovery_key(&self) -> Result<String, StorageEngineError> {
        if !self.encryption_enabled() {
            return Err(StorageEngineError::WindowsDevice(
                "This disk does not use client-side chunk encryption".into(),
            ));
        }
        Ok(crypto::export_key_hex(&self.disk_dir)?)
    }

    pub fn import_recovery_key(&self, value: &str) -> Result<(), StorageEngineError> {
        if !self.encryption_enabled() {
            return Err(StorageEngineError::WindowsDevice(
                "This disk is not marked as encrypted; the recovery key would not be used".into(),
            ));
        }
        crypto::import_key_hex(&self.disk_dir, value)?;
        Ok(())
    }

    /// Get current real-time telemetry
    pub fn telemetry(&self) -> DiskTelemetry {
        let manifest = self.manifest.read();
        let (cached, dirty, _count) = self.cache.stats();
        let dirty_chunks = self.cache.get_dirty_chunks();

        DiskTelemetry {
            disk_id: manifest.disk_id,
            name: manifest.name.clone(),
            status: *self.status.read(),
            capacity_bytes: manifest.geometry.total_capacity_bytes,
            total_chunks: manifest.geometry.total_chunks(),
            allocated_chunks: manifest.chunks.len() as u64,
            committed_chunks: manifest
                .chunks
                .values()
                .filter(|c| c.remote_object_id.is_some())
                .count() as u64,
            cached_bytes: cached,
            dirty_bytes: dirty,
            pending_sync_count: dirty_chunks.len(),
            drive_letter: self.drive_letter.read().clone(),
            physical_drive_path: self.physical_drive_path.read().clone(),
            provider_name: self
                .provider
                .as_ref()
                .map(|p| p.provider_name().to_string())
                .unwrap_or_else(|| "Local Backing Store".to_string()),
            last_error: self.last_error.read().clone(),
            vhdx_file_size: manifest.vhdx_file_size.unwrap_or(0),
            vhdx_chunks: manifest.vhdx_chunks.len() as u64,
            mirrored_chunks: manifest
                .vhdx_chunks
                .values()
                .filter(|chunk| chunk.remote_object_id.is_some())
                .count() as u64,
            encryption_enabled: manifest.encrypt_chunks,
            encryption_key_present: crypto::key_path(&self.disk_dir).exists(),
        }
    }

    pub fn set_drive_letter(&self, letter: Option<String>) {
        *self.drive_letter.write() = letter;
    }

    pub fn set_physical_drive_path(&self, path: Option<String>) {
        *self.physical_drive_path.write() = path;
    }

    pub fn set_last_error(&self, error: Option<String>) {
        *self.last_error.write() = error;
    }

    /// Verifies the integrity of the mirrored VHDX snapshot when one exists,
    /// otherwise of every allocated chunk present in the local cache. Chunks
    /// that are absent locally (no VHDX file, or a cache miss) are counted as
    /// `missing_chunks`; they are not corruption and no remote read is
    /// performed here.
    pub fn verify_integrity(&self) -> IntegrityAudit {
        let manifest = self.manifest.read();

        if manifest.vhdx_file_size.is_some() || !manifest.vhdx_chunks.is_empty() {
            let path = self.manifest_path.with_file_name("disk.vhdx");
            let chunk_size = manifest.geometry.chunk_size as u64;
            let mut verified = 0usize;
            let mut missing = 0usize;
            let total = manifest.vhdx_chunks.len();
            let mut errors = Vec::new();

            for (chunk_index, descriptor) in &manifest.vhdx_chunks {
                let offset = *chunk_index * chunk_size;
                match read_file_range(&path, offset, descriptor.length) {
                    Ok(bytes) => match descriptor.checksum.verify(&bytes) {
                        Ok(()) => verified += 1,
                        Err(e) => errors.push((*chunk_index, e.to_string())),
                    },
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound
                            || e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        missing += 1;
                    }
                    Err(e) => {
                        errors.push((*chunk_index, format!("Could not read the local VHDX: {e}")))
                    }
                }
            }

            return IntegrityAudit {
                verified_chunks: verified,
                missing_chunks: missing,
                total_chunks: total,
                errors,
            };
        }

        let mut verified = 0usize;
        let mut missing = 0usize;
        let total = manifest.chunks.len();
        let mut errors = Vec::new();
        for (chunk_idx, desc) in &manifest.chunks {
            match self.cache.get_chunk_bytes(*chunk_idx) {
                Ok(bytes) => match desc.checksum.verify(&bytes) {
                    Ok(()) => verified += 1,
                    Err(e) => errors.push((*chunk_idx, e.to_string())),
                },
                Err(CacheError::ChunkNotCached(_)) => missing += 1,
                Err(e) => errors.push((*chunk_idx, format!("Cache read failed: {e}"))),
            }
        }
        IntegrityAudit {
            verified_chunks: verified,
            missing_chunks: missing,
            total_chunks: total,
            errors,
        }
    }

    pub fn set_status(&self, status: DiskStatus) {
        *self.status.write() = status;
    }

    pub fn vhdx_path(&self) -> PathBuf {
        self.manifest_path.with_file_name("disk.vhdx")
    }

    /// Snapshot of the current manifest, used when publishing it to a provider.
    pub fn manifest_snapshot(&self) -> DiskManifest {
        self.manifest.read().clone()
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.manifest.read().geometry.total_capacity_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use virtualssd_block_map::DEFAULT_CHUNK_SIZE;
    use virtualssd_manifest::ProviderType;

    #[tokio::test]
    async fn test_virtual_disk_write_read_roundtrip() {
        let dir = tempdir().unwrap();
        let manifest = DiskManifest::new(
            "Test-Disk",
            ProviderType::Local,
            "local",
            100 * 1024 * 1024,
            DEFAULT_CHUNK_SIZE,
        )
        .unwrap();

        let engine = VirtualDiskEngine::open(dir.path(), manifest, None, 50 * 1024 * 1024).unwrap();
        engine.set_status(DiskStatus::Online);

        let data = b"Testing VirtualSSD direct block I/O with WAL and Cache!";
        engine.write_blocks(4096, data).await.unwrap();

        let mut read_buf = vec![0u8; data.len()];
        engine.read_blocks(4096, &mut read_buf).await.unwrap();
        assert_eq!(&read_buf, data);

        // Verify unwritten sparse blocks return zero
        let mut zero_buf = vec![0xFFu8; 1024];
        engine
            .read_blocks(1024 * 1024 * 50, &mut zero_buf)
            .await
            .unwrap();
        assert_eq!(zero_buf, vec![0u8; 1024]);
    }
}
