//! Logical Block Addressing (LBA) and Chunk Mapping Engine for VirtualSSD.
//!
//! Maps linear sector offsets to 4 MiB chunk indexes and handles multi-chunk I/O slicing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Default chunk size: 4 MiB (4,194,304 bytes)
pub const DEFAULT_CHUNK_SIZE: usize = 4 * 1024 * 1024;
/// Standard logical sector size: 512 bytes
pub const LOGICAL_SECTOR_SIZE: usize = 512;
/// Standard physical sector size: 4096 bytes (4Kn / 512e)
pub const PHYSICAL_SECTOR_SIZE: usize = 4096;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BlockMapError {
    #[error("I/O out of bounds: requested offset {offset} + len {length} exceeds disk capacity {capacity}")]
    OutOfBounds {
        offset: u64,
        length: usize,
        capacity: u64,
    },

    #[error(
        "Unaligned sector access: offset {offset} is not aligned to sector size {sector_size}"
    )]
    UnalignedSector { offset: u64, sector_size: usize },

    #[error("Zero length I/O request")]
    ZeroLengthIo,

    #[error("Invalid geometry: sector size {0} must be a power of two")]
    InvalidSectorSize(usize),
}

/// Disk physical and logical geometry configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskGeometry {
    pub total_capacity_bytes: u64,
    pub logical_sector_size: usize,
    pub physical_sector_size: usize,
    pub chunk_size: usize,
}

impl DiskGeometry {
    pub fn new(capacity_bytes: u64, chunk_size: usize) -> Result<Self, BlockMapError> {
        if chunk_size == 0 || (chunk_size & (chunk_size - 1)) != 0 {
            return Err(BlockMapError::InvalidSectorSize(chunk_size));
        }
        Ok(Self {
            total_capacity_bytes: capacity_bytes,
            logical_sector_size: LOGICAL_SECTOR_SIZE,
            physical_sector_size: PHYSICAL_SECTOR_SIZE,
            chunk_size,
        })
    }

    pub fn total_chunks(&self) -> u64 {
        self.total_capacity_bytes.div_ceil(self.chunk_size as u64)
    }

    pub fn total_sectors(&self) -> u64 {
        self.total_capacity_bytes / (self.logical_sector_size as u64)
    }
}

/// Represents a slice of an I/O operation targeting a single 4 MiB chunk
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkIoSlice {
    pub chunk_index: u64,
    pub chunk_offset: usize,
    pub length: usize,
    pub buffer_offset: usize,
}

/// Slices an arbitrary linear I/O operation into aligned chunk operations
pub fn slice_io(
    geometry: &DiskGeometry,
    offset: u64,
    length: usize,
) -> Result<Vec<ChunkIoSlice>, BlockMapError> {
    if length == 0 {
        return Err(BlockMapError::ZeroLengthIo);
    }
    if offset
        .checked_add(length as u64)
        .is_none_or(|end| end > geometry.total_capacity_bytes)
    {
        return Err(BlockMapError::OutOfBounds {
            offset,
            length,
            capacity: geometry.total_capacity_bytes,
        });
    }

    let mut slices = Vec::new();
    let chunk_size = geometry.chunk_size as u64;

    let mut current_offset = offset;
    let mut remaining = length;
    let mut buffer_offset = 0;

    while remaining > 0 {
        let chunk_index = current_offset / chunk_size;
        let chunk_offset = (current_offset % chunk_size) as usize;
        let bytes_in_chunk = ((chunk_size - (chunk_offset as u64)) as usize).min(remaining);

        slices.push(ChunkIoSlice {
            chunk_index,
            chunk_offset,
            length: bytes_in_chunk,
            buffer_offset,
        });

        current_offset += bytes_in_chunk as u64;
        buffer_offset += bytes_in_chunk;
        remaining -= bytes_in_chunk;
    }

    Ok(slices)
}

/// Tracks sparse allocated chunks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SparseChunkTable {
    allocated_chunks: BTreeMap<u64, bool>,
}

impl SparseChunkTable {
    pub fn new() -> Self {
        Self {
            allocated_chunks: BTreeMap::new(),
        }
    }

    pub fn mark_allocated(&mut self, chunk_index: u64) {
        self.allocated_chunks.insert(chunk_index, true);
    }

    pub fn is_allocated(&self, chunk_index: u64) -> bool {
        self.allocated_chunks.contains_key(&chunk_index)
    }

    pub fn allocated_count(&self) -> usize {
        self.allocated_chunks.len()
    }

    pub fn clear(&mut self) {
        self.allocated_chunks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_chunk_slice() {
        let geo = DiskGeometry::new(10 * 1024 * 1024, DEFAULT_CHUNK_SIZE).unwrap();
        let slices = slice_io(&geo, 1024, 2048).unwrap();
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].chunk_index, 0);
        assert_eq!(slices[0].chunk_offset, 1024);
        assert_eq!(slices[0].length, 2048);
        assert_eq!(slices[0].buffer_offset, 0);
    }

    #[test]
    fn test_multi_chunk_slice() {
        let geo = DiskGeometry::new(20 * 1024 * 1024, DEFAULT_CHUNK_SIZE).unwrap();
        // Span from (4 MiB - 100 bytes) to (4 MiB + 200 bytes)
        let offset = (DEFAULT_CHUNK_SIZE - 100) as u64;
        let len = 300;
        let slices = slice_io(&geo, offset, len).unwrap();
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].chunk_index, 0);
        assert_eq!(slices[0].chunk_offset, DEFAULT_CHUNK_SIZE - 100);
        assert_eq!(slices[0].length, 100);
        assert_eq!(slices[0].buffer_offset, 0);

        assert_eq!(slices[1].chunk_index, 1);
        assert_eq!(slices[1].chunk_offset, 0);
        assert_eq!(slices[1].length, 200);
        assert_eq!(slices[1].buffer_offset, 100);
    }

    #[test]
    fn test_out_of_bounds() {
        let geo = DiskGeometry::new(1024, DEFAULT_CHUNK_SIZE).unwrap();
        let res = slice_io(&geo, 1000, 50);
        assert!(matches!(res, Err(BlockMapError::OutOfBounds { .. })));
    }
}
