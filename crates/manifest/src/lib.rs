//! Versioned Disk Manifest and Metadata Management for VirtualSSD.
//!
//! Stores authoritative chunk mappings, provider bindings, disk geometry,
//! commit versions, and atomic publication with rollback protection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;
use virtualssd_block_map::DiskGeometry;
use virtualssd_integrity::ChunkChecksum;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("I/O error during manifest operation: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Manifest format mismatch: version {found} is not compatible with engine {expected}")]
    VersionMismatch { found: u32, expected: u32 },

    #[error("Manifest integrity verification failed: {0}")]
    CorruptManifest(String),

    #[error("Disk manifest file not found at {0}")]
    NotFound(PathBuf),
}

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderType {
    Local,
    GoogleDrive,
    Telegram,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkDescriptor {
    pub chunk_index: u64,
    pub length: usize,
    /// BLAKE3 checksum of the plaintext chunk. For encrypted chunks this is
    /// verified after decryption, never on the ciphertext.
    pub checksum: ChunkChecksum,
    /// BLAKE3 checksum of the exact bytes stored remotely. Populated only when
    /// `is_encrypted` is true; plaintext and ciphertext differ in that case.
    #[serde(default)]
    pub remote_checksum: Option<ChunkChecksum>,
    pub remote_object_id: Option<String>,
    pub remote_version: u64,
    pub is_encrypted: bool,
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskManifest {
    pub schema_version: u32,
    pub disk_id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,
    pub provider_account_id: String,
    pub geometry: DiskGeometry,
    pub commit_version: u64,
    pub chunks: BTreeMap<u64, ChunkDescriptor>,
    /// Size of the mirrored VHDX file at the last successful snapshot sync.
    /// `None` means the disk has never been mirrored.
    #[serde(default)]
    pub vhdx_file_size: Option<u64>,
    /// Chunks of the VHDX file itself, keyed by file offset / chunk_size.
    /// Kept separate from the block-engine `chunks` map so the two index
    /// spaces can never overwrite each other.
    #[serde(default)]
    pub vhdx_chunks: BTreeMap<u64, ChunkDescriptor>,
    /// When true, VHDX chunk payloads are encrypted with AES-256-GCM before
    /// leaving the machine. Fixed for the lifetime of the disk.
    #[serde(default)]
    pub encrypt_chunks: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DiskManifest {
    pub fn new(
        name: impl Into<String>,
        provider_type: ProviderType,
        provider_account_id: impl Into<String>,
        capacity_bytes: u64,
        chunk_size: usize,
    ) -> Result<Self, ManifestError> {
        let geometry = DiskGeometry::new(capacity_bytes, chunk_size)
            .map_err(|e| ManifestError::CorruptManifest(e.to_string()))?;
        let now = Utc::now();
        Ok(Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            disk_id: Uuid::new_v4(),
            name: name.into(),
            provider_type,
            provider_account_id: provider_account_id.into(),
            geometry,
            commit_version: 1,
            chunks: BTreeMap::new(),
            vhdx_file_size: None,
            vhdx_chunks: BTreeMap::new(),
            encrypt_chunks: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_chunk(
        &mut self,
        chunk_index: u64,
        length: usize,
        checksum: ChunkChecksum,
        remote_object_id: Option<String>,
        is_encrypted: bool,
    ) {
        let prev_ver = self
            .chunks
            .get(&chunk_index)
            .map_or(0, |c| c.remote_version);
        let desc = ChunkDescriptor {
            chunk_index,
            length,
            checksum,
            remote_checksum: None,
            remote_object_id,
            remote_version: prev_ver + 1,
            is_encrypted,
            last_modified: Utc::now(),
        };
        self.chunks.insert(chunk_index, desc);
        self.commit_version += 1;
        self.updated_at = Utc::now();
    }

    /// Records a VHDX file chunk. `checksum` is over the plaintext bytes and
    /// `remote_checksum` is over the exact uploaded bytes (ciphertext when the
    /// chunk is encrypted).
    pub fn update_vhdx_chunk(
        &mut self,
        chunk_index: u64,
        length: usize,
        checksum: ChunkChecksum,
        remote_checksum: Option<ChunkChecksum>,
        remote_object_id: String,
        is_encrypted: bool,
    ) {
        let prev_ver = self
            .vhdx_chunks
            .get(&chunk_index)
            .map_or(0, |c| c.remote_version);
        let desc = ChunkDescriptor {
            chunk_index,
            length,
            checksum,
            remote_checksum,
            remote_object_id: Some(remote_object_id),
            remote_version: prev_ver + 1,
            is_encrypted,
            last_modified: Utc::now(),
        };
        self.vhdx_chunks.insert(chunk_index, desc);
        self.commit_version += 1;
        self.updated_at = Utc::now();
    }

    /// Removes a VHDX chunk descriptor after its remote object was deleted.
    pub fn remove_vhdx_chunk(&mut self, chunk_index: u64) -> Option<ChunkDescriptor> {
        let removed = self.vhdx_chunks.remove(&chunk_index);
        if removed.is_some() {
            self.commit_version += 1;
            self.updated_at = Utc::now();
        }
        removed
    }

    /// Records the file size of the mirrored VHDX snapshot.
    pub fn set_vhdx_file_size(&mut self, size: u64) {
        if self.vhdx_file_size != Some(size) {
            self.vhdx_file_size = Some(size);
            self.commit_version += 1;
            self.updated_at = Utc::now();
        }
    }

    /// Drops every remote reference after the provider has confirmed deletion
    /// of the underlying objects. The local chunk metadata (checksums and
    /// lengths) is retained so the disk can be re-mirrored.
    pub fn clear_remote_chunks(&mut self) {
        for descriptor in self.chunks.values_mut() {
            descriptor.remote_object_id = None;
        }
        for descriptor in self.vhdx_chunks.values_mut() {
            descriptor.remote_object_id = None;
        }
        self.vhdx_file_size = None;
        self.commit_version += 1;
        self.updated_at = Utc::now();
    }

    /// Atomically persists the manifest to disk with `.prev` rollback protection
    pub fn save_atomic(&self, path: &Path) -> Result<(), ManifestError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let temp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
        let prev_path = path.with_extension("prev");

        let serialized = serde_json::to_string_pretty(self)?;
        fs::write(&temp_path, serialized.as_bytes())?;

        // If authoritative manifest exists, backup to .prev
        if path.exists() {
            let _ = fs::copy(path, &prev_path);
        }

        // Atomic rename
        fs::rename(&temp_path, path)?;
        Ok(())
    }

    /// Loads manifest with automatic fallback to `.prev` if corrupted
    pub fn load_with_fallback(path: &Path) -> Result<Self, ManifestError> {
        if !path.exists() {
            let prev_path = path.with_extension("prev");
            if prev_path.exists() {
                return Self::load_file(&prev_path);
            }
            return Err(ManifestError::NotFound(path.to_path_buf()));
        }

        match Self::load_file(path) {
            Ok(manifest) => Ok(manifest),
            Err(primary_err) => {
                let prev_path = path.with_extension("prev");
                if prev_path.exists() {
                    if let Ok(fallback) = Self::load_file(&prev_path) {
                        return Ok(fallback);
                    }
                }
                Err(primary_err)
            }
        }
    }

    fn load_file(path: &Path) -> Result<Self, ManifestError> {
        let content = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        if manifest.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(ManifestError::VersionMismatch {
                found: manifest.schema_version,
                expected: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use virtualssd_block_map::DEFAULT_CHUNK_SIZE;

    #[test]
    fn test_manifest_creation_and_atomic_save() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let mut manifest = DiskManifest::new(
            "Test-VirtualSSD",
            ProviderType::GoogleDrive,
            "user@gmail.com",
            100 * 1024 * 1024,
            DEFAULT_CHUNK_SIZE,
        )
        .unwrap();

        let dummy_chunk = vec![0xAB; 1024];
        let hash = ChunkChecksum::blake3(&dummy_chunk);
        manifest.update_chunk(0, 1024, hash.clone(), Some("gdrive-obj-123".into()), false);

        manifest.save_atomic(&path).unwrap();
        assert!(path.exists());

        let loaded = DiskManifest::load_with_fallback(&path).unwrap();
        assert_eq!(loaded.name, "Test-VirtualSSD");
        assert_eq!(loaded.commit_version, 2);
        assert_eq!(loaded.chunks.len(), 1);
        assert_eq!(loaded.chunks.get(&0).unwrap().checksum, hash);
    }
}
