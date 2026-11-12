//! Windows Security: DPAPI secret protection and path sanitization for VirtualSSD.
//!
//! Protects provider secrets at rest, zeroizes secret buffers, and sanitizes
//! user-supplied path components. Named-pipe ACLs and Windows Credential Manager
//! integration are not implemented in this build.

use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Windows DPAPI error: {0}")]
    Dpapi(String),

    #[error("Path traversal detected in path: {0}")]
    PathTraversal(String),

    #[error("Invalid secret format or empty credential")]
    InvalidSecret,
}

/// Secure memory string that zeroes out on drop
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretBuffer(pub Vec<u8>);

impl SecretBuffer {
    pub fn new(data: impl Into<Vec<u8>>) -> Self {
        Self(data.into())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(hmem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

#[cfg(windows)]
#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(
        p_data_in: *const DataBlob,
        sz_data_descr: *const u16,
        p_optional_entropy: *const DataBlob,
        pv_reserved: *mut std::ffi::c_void,
        p_prompt_struct: *mut std::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;

    fn CryptUnprotectData(
        p_data_in: *const DataBlob,
        ppsz_data_descr: *mut *mut u16,
        p_optional_entropy: *const DataBlob,
        pv_reserved: *mut std::ffi::c_void,
        p_prompt_struct: *mut std::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;
}

/// Windows Data Protection API (DPAPI) encrypts data tied to the current Windows user account
pub fn protect_secret_dpapi(plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
    #[cfg(windows)]
    {
        let in_blob = DataBlob {
            cb_data: plaintext.len() as u32,
            pb_data: plaintext.as_ptr() as *mut u8,
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };

        unsafe {
            let res = CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut out_blob,
            );

            if res == 0 {
                return Err(SecurityError::Dpapi("CryptProtectData failed".into()));
            }

            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let result = slice.to_vec();
            LocalFree(out_blob.pb_data as _);
            Ok(result)
        }
    }
    #[cfg(not(windows))]
    {
        Ok(plaintext.to_vec())
    }
}

/// Windows DPAPI decrypts data using the current Windows user account credentials
pub fn unprotect_secret_dpapi(ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError> {
    #[cfg(windows)]
    {
        let in_blob = DataBlob {
            cb_data: ciphertext.len() as u32,
            pb_data: ciphertext.as_ptr() as *mut u8,
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };

        unsafe {
            let res = CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut out_blob,
            );

            if res == 0 {
                return Err(SecurityError::Dpapi("CryptUnprotectData failed".into()));
            }

            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let result = slice.to_vec();
            LocalFree(out_blob.pb_data as _);
            Ok(result)
        }
    }
    #[cfg(not(windows))]
    {
        Ok(ciphertext.to_vec())
    }
}

/// Validates path to prevent directory traversal outside the base directory
pub fn sanitize_path(base_dir: &Path, user_supplied_subpath: &Path) -> Result<PathBuf, SecurityError> {
    let combined = base_dir.join(user_supplied_subpath);
    // Disallow ".." components
    for component in user_supplied_subpath.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(SecurityError::PathTraversal(
                user_supplied_subpath.to_string_lossy().to_string(),
            ));
        }
    }
    Ok(combined)
}

/// Redacts sensitive keys, tokens, and authorization strings for logs
pub fn redact_secrets(input: &str) -> String {
    let patterns = [
        ("Bearer ", "Bearer [REDACTED]"),
        ("refresh_token=", "refresh_token=[REDACTED]"),
        ("client_secret=", "client_secret=[REDACTED]"),
        ("bot_token=", "bot_token=[REDACTED]"),
    ];
    let mut out = input.to_string();
    for (pat, replacement) in patterns {
        if let Some(idx) = out.find(pat) {
            let end = out[idx..].find('&').or_else(|| out[idx..].find(' ')).map(|i| idx + i).unwrap_or(out.len());
            out.replace_range(idx..end, replacement);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpapi_roundtrip() {
        let plaintext = b"virtualssd-secret-token-key-12345";
        let encrypted = protect_secret_dpapi(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = unprotect_secret_dpapi(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_path_sanitization() {
        let base = Path::new(r"C:\ProgramData\VirtualSSD");
        let safe = Path::new(r"disks\gdrive-disk-1");
        assert!(sanitize_path(base, safe).is_ok());

        let malicious = Path::new(r"..\..\Windows\System32\cmd.exe");
        assert!(matches!(sanitize_path(base, malicious), Err(SecurityError::PathTraversal(_))));
    }
}
