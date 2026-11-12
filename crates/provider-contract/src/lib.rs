//! Provider Abstraction Contract for VirtualSSD.
//!
//! Standardizes remote cloud storage operations across Google Drive and Telegram.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use virtualssd_integrity::ChunkChecksum;
use virtualssd_manifest::DiskManifest;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Authentication error or token expired: {0}")]
    AuthExpired(String),

    #[error("Cloud storage quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("Rate limited by provider: retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },

    #[error("Remote object not found: {0}")]
    NotFound(String),

    #[error("Integrity check failed on remote chunk: {0}")]
    IntegrityFailure(String),

    #[error("Network connection failure: {0}")]
    Network(String),

    #[error("Provider internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageQuota {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResult {
    pub remote_object_id: String,
    pub remote_version: u64,
    pub bytes_transferred: usize,
}

#[async_trait]
pub trait CloudStorageProvider: Send + Sync {
    /// Friendly name of provider (e.g., "Google Drive", "Telegram")
    fn provider_name(&self) -> &'static str;

    /// Verifies that provider credentials and network connection are healthy
    async fn verify_connection(&self) -> Result<(), ProviderError>;

    /// Fetches current cloud account storage quota
    async fn get_quota(&self) -> Result<StorageQuota, ProviderError>;

    /// Uploads a single chunk with cryptographic checksum verification
    async fn upload_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        data: &[u8],
        checksum: &ChunkChecksum,
    ) -> Result<UploadResult, ProviderError>;

    /// Downloads a single chunk from the cloud and verifies BLAKE3 hash
    async fn download_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        remote_object_id: &str,
        expected_checksum: &ChunkChecksum,
    ) -> Result<Vec<u8>, ProviderError>;

    /// Deletes a chunk object from the remote provider
    async fn delete_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        remote_object_id: &str,
    ) -> Result<(), ProviderError>;

    /// Removes an older remote object after a newer version of the same chunk
    /// was successfully uploaded. Providers that replace objects in place (for
    /// example Telegram message documents) can treat this as already handled.
    async fn delete_superseded_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        remote_object_id: &str,
    ) -> Result<(), ProviderError> {
        self.delete_chunk(disk_id, chunk_index, remote_object_id)
            .await
    }

    /// Publishes a new authoritative disk manifest
    async fn publish_manifest(&self, manifest: &DiskManifest) -> Result<(), ProviderError>;

    /// Retrieves the latest authoritative disk manifest from the cloud
    async fn fetch_manifest(&self, disk_id: Uuid) -> Result<DiskManifest, ProviderError>;

    /// Deletes the published manifest for a disk. Implementations must be
    /// idempotent: a manifest that is already absent is not an error.
    async fn delete_manifest(&self, disk_id: Uuid) -> Result<(), ProviderError>;

    /// Flushes any provider-side state that is written lazily during a
    /// transfer, so nothing is left behind when the transfer ends.
    ///
    /// The default implementation does nothing. Providers that keep durable
    /// per-transfer state override it to write that state out once.
    async fn flush_transfer_state(&self) -> Result<(), ProviderError> {
        Ok(())
    }
}
