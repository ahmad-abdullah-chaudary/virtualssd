//! Integration tests for the concurrent VHDX snapshot transfer.
//!
//! These tests use an in-memory provider so they exercise the real engine
//! code path — the producer, the concurrent workers, the manifest checkpoint
//! interval and the final publish — without touching a network.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use uuid::Uuid;
use virtualssd_integrity::ChunkChecksum;
use virtualssd_manifest::{DiskManifest, ProviderType};
use virtualssd_provider_contract::{CloudStorageProvider, ProviderError, StorageQuota};
use virtualssd_storage_core::{DiskStatus, VirtualDiskEngine};

/// A provider that records what it was asked to do, in memory.
#[derive(Default)]
struct RecordingProvider {
    objects: Mutex<HashMap<(Uuid, u64), Vec<u8>>>,
    uploads: AtomicU64,
    max_in_flight: AtomicU64,
    in_flight: AtomicU64,
    published: Mutex<Vec<DiskManifest>>,
    /// When set, every upload fails with this message.
    fail_with: Mutex<Option<String>>,
    /// When set, every upload waits on this barrier before returning. The
    /// barrier only releases once the expected number of uploads are in
    /// flight at the same time, which proves the transfers really do run
    /// concurrently rather than relying on timing.
    overlap_barrier: Mutex<Option<Arc<tokio::sync::Barrier>>>,
    /// The number of uploads that must be in flight for the barrier to open.
    barrier_size: AtomicU64,
}

impl RecordingProvider {
    fn object_count(&self) -> usize {
        self.objects.lock().len()
    }
}

#[async_trait]
impl CloudStorageProvider for RecordingProvider {
    fn provider_name(&self) -> &'static str {
        "Recording Provider"
    }

    async fn verify_connection(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn get_quota(&self) -> Result<StorageQuota, ProviderError> {
        Ok(StorageQuota {
            total_bytes: 1024 * 1024 * 1024 * 1024,
            used_bytes: 0,
            available_bytes: 1024 * 1024 * 1024 * 1024,
        })
    }

    async fn upload_chunk(
        &self,
        namespace: Uuid,
        chunk_index: u64,
        data: &[u8],
        checksum: &ChunkChecksum,
    ) -> Result<virtualssd_provider_contract::UploadResult, ProviderError> {
        let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(in_flight, Ordering::SeqCst);

        // If a barrier is configured, wait for the expected number of
        // uploads to be in flight at the same time. The barrier only opens
        // when they really do overlap, so this proves concurrency rather
        // than inferring it from timing.
        let barrier = self.overlap_barrier.lock().clone();
        if let Some(barrier) = barrier {
            let expected = self.barrier_size.load(Ordering::SeqCst).max(1) as usize;
            // The barrier is waited on with a timeout so a sequential
            // implementation fails the test instead of hanging it.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), barrier.wait()).await;
            let _ = expected;
        }

        self.in_flight.fetch_sub(1, Ordering::SeqCst);

        if let Some(message) = self.fail_with.lock().clone() {
            return Err(ProviderError::Network(message));
        }

        checksum
            .verify(data)
            .map_err(|e| ProviderError::IntegrityFailure(e.to_string()))?;

        let object_id = format!("obj-{namespace}-{chunk_index}");
        self.objects
            .lock()
            .insert((namespace, chunk_index), data.to_vec());
        self.uploads.fetch_add(1, Ordering::SeqCst);

        Ok(virtualssd_provider_contract::UploadResult {
            remote_object_id: object_id,
            remote_version: 1,
            bytes_transferred: data.len(),
        })
    }

    async fn download_chunk(
        &self,
        _namespace: Uuid,
        chunk_index: u64,
        remote_object_id: &str,
        expected_checksum: &ChunkChecksum,
    ) -> Result<Vec<u8>, ProviderError> {
        let _ = remote_object_id;
        let data = self
            .objects
            .lock()
            .values()
            .find(|_| true)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(format!("chunk {chunk_index}")))?;
        expected_checksum
            .verify(&data)
            .map_err(|e| ProviderError::IntegrityFailure(e.to_string()))?;
        Ok(data)
    }

    async fn delete_chunk(
        &self,
        namespace: Uuid,
        chunk_index: u64,
        _remote_object_id: &str,
    ) -> Result<(), ProviderError> {
        self.objects.lock().remove(&(namespace, chunk_index));
        Ok(())
    }

    async fn publish_manifest(&self, manifest: &DiskManifest) -> Result<(), ProviderError> {
        self.published.lock().push(manifest.clone());
        Ok(())
    }

    async fn fetch_manifest(&self, disk_id: Uuid) -> Result<DiskManifest, ProviderError> {
        self.published
            .lock()
            .iter()
            .find(|m| m.disk_id == disk_id)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(disk_id.to_string()))
    }

    async fn delete_manifest(&self, _disk_id: Uuid) -> Result<(), ProviderError> {
        Ok(())
    }
}

/// Builds a disk whose VHDX contains `chunks` non-zero 4 MiB chunks.
fn make_disk(
    dir: &std::path::Path,
    provider: Arc<RecordingProvider>,
    chunks: usize,
) -> (Uuid, VirtualDiskEngine) {
    let chunk_size: usize = 4 * 1024 * 1024;
    let capacity = (chunks as u64) * chunk_size as u64 + (64 * 1024 * 1024);

    let manifest = DiskManifest::new(
        "Concurrency Test Disk".to_string(),
        ProviderType::GoogleDrive,
        "provider-1".to_string(),
        capacity,
        chunk_size,
    )
    .unwrap();

    let disk_id = manifest.disk_id;
    let disk_dir = dir.join(disk_id.to_string());
    std::fs::create_dir_all(&disk_dir).unwrap();

    // Write a VHDX-sized file with non-zero content in each chunk.
    let mut file = std::fs::File::create(disk_dir.join("disk.vhdx")).unwrap();
    for index in 0..chunks {
        let mut payload = vec![0u8; chunk_size];
        // Distinct, non-zero content per chunk so no chunk is treated as sparse.
        payload[0] = (index as u8) + 1;
        payload[1] = 0xFF;
        std::io::Write::write_all(&mut file, &payload).unwrap();
    }
    std::io::Write::write_all(&mut file, &vec![0u8; 1024]).unwrap();
    std::io::Write::flush(&mut file).unwrap();

    let engine = VirtualDiskEngine::open(
        &disk_dir,
        manifest,
        Some(provider as Arc<dyn CloudStorageProvider>),
        64 * 1024 * 1024,
    )
    .unwrap();

    (disk_id, engine)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_mirror_uploads_every_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(RecordingProvider::default());
    let (_disk_id, engine) = make_disk(dir.path(), provider.clone(), 4);

    let report = engine.sync_vhdx_snapshot().await.unwrap();

    assert_eq!(report.uploaded_chunks, 4, "every chunk must be uploaded");
    assert_eq!(report.skipped_chunks, 0);
    assert_eq!(provider.uploads.load(Ordering::SeqCst), 4);
    assert_eq!(provider.object_count(), 4);
    assert_eq!(provider.published.lock().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_mirror_runs_transfers_in_parallel() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(RecordingProvider::default());
    let (_disk_id, engine) = make_disk(dir.path(), provider.clone(), 6);

    // The barrier only opens once two uploads are in flight at the same time.
    // A sequential implementation would never satisfy it.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *provider.overlap_barrier.lock() = Some(barrier);
    provider.barrier_size.store(2, Ordering::SeqCst);

    engine.sync_vhdx_snapshot().await.unwrap();

    assert!(
        provider.max_in_flight.load(Ordering::SeqCst) >= 2,
        "expected concurrent transfers, saw a maximum of {} in flight",
        provider.max_in_flight.load(Ordering::SeqCst)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_mirror_respects_the_concurrency_limit() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(RecordingProvider::default());
    let (_disk_id, engine) = make_disk(dir.path(), provider.clone(), 8);

    // The barrier only opens once the full concurrency limit is in flight at
    // the same time. This proves the limit is reached, and the peak counter
    // proves it is not exceeded.
    let limit = virtualssd_storage_core::transfer::DEFAULT_UPLOAD_CONCURRENCY;
    let barrier = Arc::new(tokio::sync::Barrier::new(limit));
    *provider.overlap_barrier.lock() = Some(barrier);
    provider.barrier_size.store(limit as u64, Ordering::SeqCst);

    engine.sync_vhdx_snapshot().await.unwrap();

    let peak = provider.max_in_flight.load(Ordering::SeqCst);
    assert_eq!(
        peak, limit as u64,
        "the transfer limit must be reached but not exceeded, saw {peak} in flight (limit {limit})"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_mirror_skips_unchanged_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(RecordingProvider::default());
    let (_disk_id, engine) = make_disk(dir.path(), provider.clone(), 3);

    let first = engine.sync_vhdx_snapshot().await.unwrap();
    assert_eq!(first.uploaded_chunks, 3);

    let second = engine.sync_vhdx_snapshot().await.unwrap();
    assert_eq!(
        second.skipped_chunks, 3,
        "unchanged chunks must not be re-uploaded"
    );
    assert_eq!(second.uploaded_chunks, 0);
    assert_eq!(
        provider.uploads.load(Ordering::SeqCst),
        3,
        "the second mirror must not send anything"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_upload_is_reported_and_not_reported_as_success() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(RecordingProvider::default());
    let (_disk_id, engine) = make_disk(dir.path(), provider.clone(), 4);

    *provider.fail_with.lock() = Some("network is down".to_string());

    let result = engine.sync_vhdx_snapshot().await;
    assert!(
        result.is_err(),
        "a failed upload must not be reported as success"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("network is down"),
        "the provider error must reach the caller, got: {error}"
    );

    // The engine must be marked degraded, not silently left healthy.
    assert_eq!(engine.telemetry().status, DiskStatus::Degraded);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_manifest_is_persisted_after_the_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(RecordingProvider::default());
    let (disk_id, engine) = make_disk(dir.path(), provider.clone(), 3);

    engine.sync_vhdx_snapshot().await.unwrap();

    // The manifest on disk must list every chunk, so a restart resumes from
    // the recorded state instead of re-uploading.
    let manifest_path = dir.path().join(disk_id.to_string()).join("manifest.json");
    let stored = DiskManifest::load_with_fallback(&manifest_path).unwrap();
    assert_eq!(stored.vhdx_chunks.len(), 3);
    for descriptor in stored.vhdx_chunks.values() {
        assert!(descriptor.remote_object_id.is_some());
    }
}
