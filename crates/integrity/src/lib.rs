//! Cryptographic checksum and data integrity module for VirtualSSD.
//!
//! Provides BLAKE3 and SHA-256 chunk hashing, validation, tampering detection,
//! and structured error reporting.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntegrityError {
    #[error("Checksum mismatch: expected {expected}, actual {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Invalid checksum format: {0}")]
    InvalidFormat(String),

    #[error("Corrupt payload: expected {expected_len} bytes, found {actual_len} bytes")]
    LengthMismatch {
        expected_len: usize,
        actual_len: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Blake3,
    Sha256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkChecksum {
    pub algorithm: HashAlgorithm,
    pub hex_digest: String,
}

impl ChunkChecksum {
    pub fn blake3(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Self {
            algorithm: HashAlgorithm::Blake3,
            hex_digest: hash.to_hex().to_string(),
        }
    }

    pub fn sha256(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        Self {
            algorithm: HashAlgorithm::Sha256,
            hex_digest: hex::encode(result),
        }
    }

    pub fn verify(&self, data: &[u8]) -> Result<(), IntegrityError> {
        let actual = match self.algorithm {
            HashAlgorithm::Blake3 => Self::blake3(data),
            HashAlgorithm::Sha256 => Self::sha256(data),
        };

        if self.hex_digest.eq_ignore_ascii_case(&actual.hex_digest) {
            Ok(())
        } else {
            Err(IntegrityError::ChecksumMismatch {
                expected: self.hex_digest.clone(),
                actual: actual.hex_digest,
            })
        }
    }
}

impl fmt::Display for ChunkChecksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            match self.algorithm {
                HashAlgorithm::Blake3 => "blake3",
                HashAlgorithm::Sha256 => "sha256",
            },
            self.hex_digest
        )
    }
}

/// Compute BLAKE3 checksum of a chunk.
pub fn compute_chunk_hash(data: &[u8]) -> ChunkChecksum {
    ChunkChecksum::blake3(data)
}

/// Verifies whether a chunk matches the given BLAKE3 hex digest.
pub fn verify_chunk_blake3(data: &[u8], expected_hex: &str) -> Result<(), IntegrityError> {
    let expected = ChunkChecksum {
        algorithm: HashAlgorithm::Blake3,
        hex_digest: expected_hex.to_string(),
    };
    expected.verify(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_integrity() {
        let data = b"VirtualSSD Block Storage Sector Data Chunk #42";
        let checksum = compute_chunk_hash(data);
        assert_eq!(checksum.algorithm, HashAlgorithm::Blake3);
        assert!(checksum.verify(data).is_ok());

        // Tamper test
        let mut tampered = data.to_vec();
        tampered[0] ^= 0xFF;
        let err = checksum.verify(&tampered).unwrap_err();
        match err {
            IntegrityError::ChecksumMismatch { expected, actual } => {
                assert_eq!(expected, checksum.hex_digest);
                assert_ne!(expected, actual);
            }
            _ => panic!("Expected ChecksumMismatch"),
        }
    }

    #[test]
    fn test_sha256_integrity() {
        let data = vec![0u8; 4096];
        let checksum = ChunkChecksum::sha256(&data);
        assert_eq!(checksum.algorithm, HashAlgorithm::Sha256);
        assert!(checksum.verify(&data).is_ok());
    }

    #[test]
    fn test_empty_buffer() {
        let checksum = compute_chunk_hash(&[]);
        assert!(checksum.verify(&[]).is_ok());
    }
}
