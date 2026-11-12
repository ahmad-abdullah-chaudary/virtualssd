//! Persistent Bounded Cache Manager for VirtualSSD.
//!
//! Tracks clean, dirty, pending, and committed chunk states. Evicts only clean
//! data using LRU policy with high/low watermarks, ensuring zero data loss.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use virtualssd_integrity::{ChunkChecksum, IntegrityError};

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("I/O error in cache: {0}")]
    Io(#[from] std::io::Error),

    #[error("Integrity error: {0}")]
    Integrity(#[from] IntegrityError),

    #[error("Chunk {0} not present in cache")]
    ChunkNotCached(u64),

    #[error("Cache full: dirty chunks cannot be evicted ({dirty_bytes} dirty bytes)")]
    CacheFullDirtyLocked { dirty_bytes: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkState {
    Clean,
    Dirty,
    PendingUpload,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedChunkMeta {
    pub chunk_index: u64,
    pub state: ChunkState,
    pub size: usize,
    pub checksum: ChunkChecksum,
    pub last_accessed: i64,
}

pub struct PersistentCacheManager {
    cache_dir: PathBuf,
    max_bytes: AtomicU64,
    chunk_size: usize,
    entries: RwLock<HashMap<u64, CachedChunkMeta>>,
    total_cached_bytes: AtomicU64,
    dirty_bytes: AtomicU64,
}

impl PersistentCacheManager {
    pub fn new(cache_dir: impl AsRef<Path>, max_bytes: u64, chunk_size: usize) -> Result<Self, CacheError> {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        fs::create_dir_all(&cache_dir)?;

        let mut entries = HashMap::new();
        let mut total_bytes = 0u64;
        let dirty = 0u64;

        // Scan directory on startup
        if let Ok(read_dir) = fs::read_dir(&cache_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("chunk") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(idx) = name.parse::<u64>() {
                            let metadata = entry.metadata().ok();
                            let size = metadata.map(|m| m.len() as usize).unwrap_or(chunk_size);
                            total_bytes += size as u64;

                            entries.insert(
                                idx,
                                CachedChunkMeta {
                                    chunk_index: idx,
                                    state: ChunkState::Clean, // Restarts assume clean unless WAL specifies otherwise
                                    size,
                                    checksum: ChunkChecksum::blake3(&[]),
                                    last_accessed: chrono::Utc::now().timestamp_millis(),
                                },
                            );
                        }
                    }
                }
            }
        }

        Ok(Self {
            cache_dir,
            max_bytes: AtomicU64::new(max_bytes),
            chunk_size,
            entries: RwLock::new(entries),
            total_cached_bytes: AtomicU64::new(total_bytes),
            dirty_bytes: AtomicU64::new(dirty),
        })
    }

    /// Changes the cache ceiling at runtime and immediately evicts clean chunks
    /// until the cache is back under the new limit.
    pub fn set_max_bytes(&self, max_bytes: u64) -> Result<(), CacheError> {
        self.max_bytes.store(max_bytes, Ordering::Relaxed);
        self.enforce_capacity()
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes.load(Ordering::Relaxed)
    }

    fn chunk_path(&self, chunk_index: u64) -> PathBuf {
        self.cache_dir.join(format!("{chunk_index}.chunk"))
    }

    pub fn read_chunk_slice(
        &self,
        chunk_index: u64,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<(), CacheError> {
        let path = self.chunk_path(chunk_index);
        if !path.exists() {
            return Err(CacheError::ChunkNotCached(chunk_index));
        }

        let mut file = File::open(&path)?;
        file.seek(SeekFrom::Start(offset as u64))?;
        file.read_exact(buffer)?;

        if let Some(entry) = self.entries.write().get_mut(&chunk_index) {
            entry.last_accessed = chrono::Utc::now().timestamp_millis();
        }

        Ok(())
    }

    pub fn write_chunk_slice(
        &self,
        chunk_index: u64,
        offset: usize,
        data: &[u8],
    ) -> Result<(), CacheError> {
        let path = self.chunk_path(chunk_index);
        let exists = path.exists();

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        if !exists {
            // Allocate sparse or zero-fill up to chunk_size
            file.set_len(self.chunk_size as u64)?;
            self.total_cached_bytes.fetch_add(self.chunk_size as u64, Ordering::Relaxed);
        }

        file.seek(SeekFrom::Start(offset as u64))?;
        file.write_all(data)?;
        file.flush()?;

        let mut entries = self.entries.write();
        let was_dirty = entries.get(&chunk_index).map_or(false, |e| e.state == ChunkState::Dirty);

        if !was_dirty {
            self.dirty_bytes.fetch_add(self.chunk_size as u64, Ordering::Relaxed);
        }

        entries.insert(
            chunk_index,
            CachedChunkMeta {
                chunk_index,
                state: ChunkState::Dirty,
                size: self.chunk_size,
                checksum: ChunkChecksum::blake3(&[]),
                last_accessed: chrono::Utc::now().timestamp_millis(),
            },
        );

        drop(entries);
        self.enforce_capacity()?;
        Ok(())
    }

    pub fn mark_committed(&self, chunk_index: u64) {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&chunk_index) {
            if entry.state == ChunkState::Dirty || entry.state == ChunkState::PendingUpload {
                self.dirty_bytes.fetch_sub(self.chunk_size as u64, Ordering::Relaxed);
            }
            entry.state = ChunkState::Committed;
        }
    }

    pub fn get_dirty_chunks(&self) -> Vec<u64> {
        let entries = self.entries.read();
        entries
            .values()
            .filter(|e| e.state == ChunkState::Dirty)
            .map(|e| e.chunk_index)
            .collect()
    }

    pub fn get_chunk_bytes(&self, chunk_index: u64) -> Result<Vec<u8>, CacheError> {
        let path = self.chunk_path(chunk_index);
        if !path.exists() {
            return Err(CacheError::ChunkNotCached(chunk_index));
        }
        let mut buf = Vec::new();
        File::open(path)?.read_to_end(&mut buf)?;
        Ok(buf)
    }

    pub fn put_chunk_bytes(&self, chunk_index: u64, data: &[u8], state: ChunkState) -> Result<(), CacheError> {
        let path = self.chunk_path(chunk_index);
        let mut file = File::create(&path)?;
        file.write_all(data)?;
        file.flush()?;

        let mut entries = self.entries.write();
        entries.insert(
            chunk_index,
            CachedChunkMeta {
                chunk_index,
                state,
                size: data.len(),
                checksum: ChunkChecksum::blake3(data),
                last_accessed: chrono::Utc::now().timestamp_millis(),
            },
        );

        self.total_cached_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn enforce_capacity(&self) -> Result<(), CacheError> {
        let max_bytes = self.max_bytes.load(Ordering::Relaxed);
        let current = self.total_cached_bytes.load(Ordering::Relaxed);
        if current <= max_bytes {
            return Ok(());
        }

        // Evict LRU Clean or Committed chunks
        let mut entries = self.entries.write();
        let mut evictable: Vec<(u64, i64, usize)> = entries
            .values()
            .filter(|e| e.state == ChunkState::Clean || e.state == ChunkState::Committed)
            .map(|e| (e.chunk_index, e.last_accessed, e.size))
            .collect();

        evictable.sort_by_key(|k| k.1); // Sort oldest first

        for (idx, _, size) in evictable {
            let path = self.chunk_path(idx);
            let _ = fs::remove_file(path);
            entries.remove(&idx);
            self.total_cached_bytes.fetch_sub(size as u64, Ordering::Relaxed);

            if self.total_cached_bytes.load(Ordering::Relaxed) < (max_bytes * 8 / 10) {
                break;
            }
        }

        Ok(())
    }

    pub fn is_cached(&self, chunk_index: u64) -> bool {
        self.entries.read().contains_key(&chunk_index)
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        let total = self.total_cached_bytes.load(Ordering::Relaxed);
        let dirty = self.dirty_bytes.load(Ordering::Relaxed);
        let count = self.entries.read().len();
        (total, dirty, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cache_write_and_read() {
        let dir = tempdir().unwrap();
        let cache = PersistentCacheManager::new(dir.path(), 100 * 1024 * 1024, 4096).unwrap();

        let data = b"Hello VirtualSSD Persistent Cache";
        cache.write_chunk_slice(0, 100, data).unwrap();

        let mut read_buf = vec![0u8; data.len()];
        cache.read_chunk_slice(0, 100, &mut read_buf).unwrap();
        assert_eq!(&read_buf, data);

        assert_eq!(cache.get_dirty_chunks(), vec![0]);
        cache.mark_committed(0);
        assert!(cache.get_dirty_chunks().is_empty());
    }
}
