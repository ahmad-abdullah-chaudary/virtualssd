//! Client-side encryption for chunk payloads.
//!
//! Each encrypted disk has a random 256-bit key stored locally as a
//! DPAPI-protected file (`encryption.key`). Chunk payloads are sealed with
//! AES-256-GCM using a fresh random nonce for every upload.
//!
//! The key never leaves the machine unless the user explicitly exports the
//! recovery key and imports it on another installation.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

const KEY_FILE: &str = "encryption.key";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const FORMAT_VERSION: u8 = 1;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Encryption key I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Windows DPAPI could not protect the encryption key: {0}")]
    Dpapi(String),

    #[error("Encryption key file is invalid: {0}")]
    InvalidKey(String),

    #[error("This disk is encrypted but no local encryption key exists. Import the recovery key before syncing or mounting it.")]
    MissingKey,

    #[error("Chunk encryption failed: {0}")]
    Encrypt(String),

    #[error("Chunk decryption failed: {0}")]
    Decrypt(String),
}

pub fn key_path(disk_dir: &Path) -> PathBuf {
    disk_dir.join(KEY_FILE)
}

fn decode_key(bytes: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    if bytes.len() != KEY_LEN {
        return Err(CryptoError::InvalidKey(format!(
            "expected {KEY_LEN} bytes, found {}",
            bytes.len()
        )));
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(bytes);
    Ok(key)
}

/// Loads the disk key if it exists. Returns `None` when no key file is present.
pub fn load_key(disk_dir: &Path) -> Result<Option<Zeroizing<[u8; KEY_LEN]>>, CryptoError> {
    let path = key_path(disk_dir);
    let protected = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CryptoError::Io(e)),
    };

    let mut plaintext = virtualssd_security::unprotect_secret_dpapi(&protected)
        .map_err(|e| CryptoError::Dpapi(e.to_string()))?;
    let decoded = decode_key(&plaintext);
    plaintext.zeroize();
    decoded.map(|key| Some(Zeroizing::new(key)))
}

/// Persists a key with DPAPI protection using a temp file + rename.
pub fn write_key(disk_dir: &Path, key: &[u8; KEY_LEN]) -> Result<(), CryptoError> {
    let protected = virtualssd_security::protect_secret_dpapi(key)
        .map_err(|e| CryptoError::Dpapi(e.to_string()))?;

    std::fs::create_dir_all(disk_dir)?;
    let path = key_path(disk_dir);
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, &protected)?;
    if let Err(e) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(CryptoError::Io(e));
    }
    Ok(())
}

/// Returns the existing key or creates a new random one.
pub fn ensure_key(disk_dir: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    if let Some(key) = load_key(disk_dir)? {
        return Ok(key);
    }

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    use aes_gcm::aead::rand_core::RngCore;
    OsRng.fill_bytes(key.as_mut());
    write_key(disk_dir, &key)?;
    Ok(key)
}

pub fn export_key_hex(disk_dir: &Path) -> Result<String, CryptoError> {
    let key = load_key(disk_dir)?.ok_or(CryptoError::MissingKey)?;
    Ok(hex_encode(key.as_ref()))
}

/// Imports a recovery key. An existing different key is rejected rather than
/// silently replaced, because replacing it would make existing chunks
/// undecryptable.
pub fn import_key_hex(disk_dir: &Path, value: &str) -> Result<(), CryptoError> {
    let normalized: String = value
        .trim()
        .trim_start_matches("vssd:")
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = hex_decode(&normalized)?;
    let key = decode_key(&bytes)?;

    if let Some(existing) = load_key(disk_dir)? {
        if existing.as_ref() != key {
            return Err(CryptoError::InvalidKey(
                "a different encryption key is already installed for this disk".into(),
            ));
        }
        return Ok(());
    }

    write_key(disk_dir, &key)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, CryptoError> {
    if !value.len().is_multiple_of(2) {
        return Err(CryptoError::InvalidKey(
            "recovery key must contain an even number of hexadecimal characters".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let chars: Vec<char> = value.chars().collect();
    for pair in chars.chunks(2) {
        let hi = pair[0]
            .to_digit(16)
            .ok_or_else(|| CryptoError::InvalidKey("recovery key is not hexadecimal".into()))?;
        let lo = pair[1]
            .to_digit(16)
            .ok_or_else(|| CryptoError::InvalidKey("recovery key is not hexadecimal".into()))?;
        bytes.push(((hi << 4) | lo) as u8);
    }
    Ok(bytes)
}

/// AES-256-GCM cipher bound to one disk key.
pub struct ChunkCipher {
    key: Zeroizing<[u8; KEY_LEN]>,
}

impl ChunkCipher {
    pub fn new(key: Zeroizing<[u8; KEY_LEN]>) -> Self {
        Self { key }
    }

    fn cipher(&self) -> Result<Aes256Gcm, CryptoError> {
        Aes256Gcm::new_from_slice(self.key.as_ref())
            .map_err(|e| CryptoError::Encrypt(e.to_string()))
    }

    /// Returns `version || nonce || ciphertext || tag`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let cipher = self.cipher()?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| CryptoError::Encrypt(e.to_string()))?;

        let mut sealed = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        sealed.push(FORMAT_VERSION);
        sealed.extend_from_slice(nonce.as_slice());
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    pub fn decrypt(&self, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if sealed.len() < 1 + NONCE_LEN + TAG_LEN {
            return Err(CryptoError::Decrypt(
                "ciphertext is shorter than the authenticated envelope".into(),
            ));
        }
        if sealed[0] != FORMAT_VERSION {
            return Err(CryptoError::Decrypt(format!(
                "unsupported encryption envelope version {}",
                sealed[0]
            )));
        }

        let cipher = self.cipher()?;
        let nonce = Nonce::from_slice(&sealed[1..1 + NONCE_LEN]);
        cipher
            .decrypt(nonce, &sealed[1 + NONCE_LEN..])
            .map_err(|e| CryptoError::Decrypt(e.to_string()))
    }
}

impl Drop for ChunkCipher {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encrypts_and_decrypts_round_trip() {
        let key = Zeroizing::new([7u8; 32]);
        let cipher = ChunkCipher::new(key);
        let plaintext = b"VirtualSSD chunk payload";

        let sealed = cipher.encrypt(plaintext).unwrap();
        assert_ne!(sealed.as_slice(), plaintext);
        assert_eq!(cipher.decrypt(&sealed).unwrap(), plaintext);
    }

    #[test]
    fn rejects_tampered_ciphertext_and_wrong_key() {
        let cipher = ChunkCipher::new(Zeroizing::new([1u8; 32]));
        let mut sealed = cipher.encrypt(b"payload").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(cipher.decrypt(&sealed).is_err());

        let other = ChunkCipher::new(Zeroizing::new([2u8; 32]));
        let sealed = cipher.encrypt(b"payload").unwrap();
        assert!(other.decrypt(&sealed).is_err());
    }

    #[test]
    fn dpapi_key_file_round_trip_and_hex_export() {
        let dir = tempdir().unwrap();
        let key = ensure_key(dir.path()).unwrap();
        let restored = load_key(dir.path()).unwrap().unwrap();
        assert_eq!(key.as_ref(), restored.as_ref());

        let exported = export_key_hex(dir.path()).unwrap();
        assert_eq!(exported.len(), 64);
        assert!(exported.chars().all(|c| c.is_ascii_hexdigit()));

        // Re-importing the same key is idempotent.
        import_key_hex(dir.path(), &exported).unwrap();

        // A different key is rejected instead of overwriting the original.
        let other = "00".repeat(32);
        assert!(import_key_hex(dir.path(), &other).is_err());
    }
}
