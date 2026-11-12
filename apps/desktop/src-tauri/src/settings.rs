//! Persisted runtime settings for the desktop application.
//!
//! Settings are stored as plain JSON because they contain no secrets. Every
//! write goes through a temp file + rename so a crash cannot leave a
//! half-written configuration. A damaged file is preserved next to the original
//! and the UI is told about it instead of silently falling back.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CACHE_LIMIT_MIN_GB: u64 = 1;
pub const CACHE_LIMIT_MAX_GB: u64 = 4096;
pub const POLL_INTERVAL_MIN_MS: u64 = 500;
pub const POLL_INTERVAL_MAX_MS: u64 = 60_000;
pub const CAPACITY_MIN_GB: u64 = 1;
pub const CAPACITY_MAX_GB: u64 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Per-disk persistent cache ceiling in GiB.
    pub cache_limit_gb: u64,
    /// Whether newly created disks encrypt VHDX chunks before upload.
    pub encrypt_new_disks: bool,
    /// Frontend telemetry polling interval in milliseconds.
    pub telemetry_poll_interval_ms: u64,
    /// Run a local BLAKE3 audit after every successful VHDX snapshot sync.
    pub verify_after_sync: bool,
    /// Pre-selected capacity in the create-disk dialog.
    pub default_capacity_gb: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            cache_limit_gb: 50,
            encrypt_new_disks: false,
            telemetry_poll_interval_ms: 10_000,
            verify_after_sync: false,
            default_capacity_gb: 100,
        }
    }
}

impl AppSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(CACHE_LIMIT_MIN_GB..=CACHE_LIMIT_MAX_GB).contains(&self.cache_limit_gb) {
            return Err(format!(
                "Cache limit must be between {CACHE_LIMIT_MIN_GB} and {CACHE_LIMIT_MAX_GB} GiB"
            ));
        }
        if !(POLL_INTERVAL_MIN_MS..=POLL_INTERVAL_MAX_MS)
            .contains(&self.telemetry_poll_interval_ms)
        {
            return Err(format!(
                "Telemetry polling interval must be between {POLL_INTERVAL_MIN_MS} and {POLL_INTERVAL_MAX_MS} ms"
            ));
        }
        if !(CAPACITY_MIN_GB..=CAPACITY_MAX_GB).contains(&self.default_capacity_gb) {
            return Err(format!(
                "Default capacity must be between {CAPACITY_MIN_GB} and {CAPACITY_MAX_GB} GB"
            ));
        }
        Ok(())
    }

    pub fn cache_limit_bytes(&self) -> u64 {
        self.cache_limit_gb * 1024 * 1024 * 1024
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsSnapshot {
    pub settings: AppSettings,
    /// Set when the stored file could not be read or parsed. The returned
    /// settings are defaults in that case, and this must be shown to the user.
    pub warning: Option<String>,
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid settings path {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)
        .map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot replace {}: {e}", path.display()));
    }
    Ok(())
}

pub fn load(data_dir: &Path) -> SettingsSnapshot {
    let path = settings_path(data_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return SettingsSnapshot {
                settings: AppSettings::default(),
                warning: None,
            }
        }
        Err(e) => {
            return SettingsSnapshot {
                settings: AppSettings::default(),
                warning: Some(format!(
                    "Could not read {}: {e}. Default settings are in use and nothing has been overwritten.",
                    path.display()
                )),
            }
        }
    };

    match serde_json::from_slice::<AppSettings>(&bytes) {
        Ok(settings) => match settings.validate() {
            Ok(()) => SettingsSnapshot {
                settings,
                warning: None,
            },
            Err(e) => SettingsSnapshot {
                settings: AppSettings::default(),
                warning: Some(format!(
                    "{} contains invalid values ({e}). Default settings are in use; update and save to replace the file.",
                    path.display()
                )),
            },
        },
        Err(e) => {
            let backup = path.with_extension(format!(
                "json.corrupt-{}",
                chrono::Utc::now().timestamp()
            ));
            let preserved = std::fs::rename(&path, &backup);
            let detail = match preserved {
                Ok(()) => format!("preserved at {}", backup.display()),
                Err(rename_err) => format!(
                    "could not be preserved: {rename_err}; the file was left in place"
                ),
            };
            SettingsSnapshot {
                settings: AppSettings::default(),
                warning: Some(format!(
                    "{} is not valid settings JSON ({e}); {detail}. Default settings are in use.",
                    path.display()
                )),
            }
        }
    }
}

pub fn save(data_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    settings.validate()?;
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("Cannot serialize settings: {e}"))?;
    write_atomic(&settings_path(data_dir), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_are_valid_and_round_trip() {
        let dir = tempdir().unwrap();
        let defaults = AppSettings::default();
        defaults.validate().unwrap();
        assert_eq!(defaults.cache_limit_bytes(), 50 * 1024 * 1024 * 1024);

        save(dir.path(), &defaults).unwrap();
        let loaded = load(dir.path());
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.settings, defaults);
    }

    #[test]
    fn rejects_out_of_range_values() {
        let mut settings = AppSettings::default();
        settings.cache_limit_gb = 0;
        assert!(settings.validate().is_err());

        settings = AppSettings::default();
        settings.telemetry_poll_interval_ms = 10;
        assert!(settings.validate().is_err());

        settings = AppSettings::default();
        settings.default_capacity_gb = 0;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn corrupt_file_is_preserved_and_reported() {
        let dir = tempdir().unwrap();
        std::fs::write(settings_path(dir.path()), b"{not json").unwrap();

        let snapshot = load(dir.path());
        assert!(snapshot.warning.is_some());
        assert_eq!(snapshot.settings, AppSettings::default());

        let preserved = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt"));
        assert!(preserved, "the damaged settings file must be preserved");
    }
}
