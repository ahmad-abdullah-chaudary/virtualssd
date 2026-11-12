// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use uuid::Uuid;
use virtualssd_block_map::DEFAULT_CHUNK_SIZE;
use virtualssd_manifest::{DiskManifest, ProviderType};
use virtualssd_provider_contract::CloudStorageProvider;
use virtualssd_provider_gdrive::{GoogleDriveCredentials, GoogleDriveProvider};
use virtualssd_provider_telegram::{TelegramConfig, TelegramProvider};
use virtualssd_storage_core::windows_vhd;
use virtualssd_storage_core::{
    DiskStatus, DiskTelemetry, MirrorReport, RemoteDeleteReport, VirtualDiskEngine,
};
use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use zeroize::Zeroize;

mod settings;
use settings::{AppSettings, SettingsSnapshot};

mod engine_log;
use engine_log::EngineLogEntry;

mod tray;

// -- Provider Registry Types --------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ProviderKind {
    GoogleDrive,
    Telegram,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRecord {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub connected: bool,
    pub error_message: Option<String>,
    pub total_quota_bytes: u64,
    pub used_quota_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrityReport {
    pub disk_id: String,
    pub disk_name: String,
    pub verified_chunks: usize,
    pub missing_chunks: usize,
    pub total_chunks: usize,
    pub errors: Vec<String>,
    pub passed: bool,
    pub disk_error: Option<String>,
}

/// Result of a manual sync. `integrity` is populated only when the user
/// enabled "verify after sync" in the persisted settings.
#[derive(Debug, Clone, Serialize)]
pub struct SyncDiskReport {
    pub mirror: MirrorReport,
    pub integrity: Option<IntegrityReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub disk: DiskTelemetry,
    pub has_vhdx_snapshot: bool,
    pub vhdx_file_size: u64,
    pub vhdx_chunks: usize,
    /// True when the manifest says the disk is encrypted but no local key is
    /// installed. The disk is imported, but mounting or syncing it will fail
    /// until the recovery key is imported.
    pub requires_recovery_key: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteReport {
    pub remote_data_requested: bool,
    pub remote: Option<RemoteDeleteReport>,
}

// -- Secure Provider Persistence ---------------------------------------------
//
// The provider index (`providers.json`) contains no secrets. Credentials are
// serialized separately, encrypted with Windows DPAPI for the current user, and
// written to `cred_<provider_id>.bin`. Both files are replaced atomically.

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedProvider {
    id: String,
    kind: ProviderKind,
    display_name: String,
}

#[derive(Serialize, Deserialize, Zeroize)]
struct PersistedGDriveSecret {
    client_id: String,
    client_secret: Option<String>,
    refresh_token: String,
    code_verifier: Option<String>,
    code_challenge: Option<String>,
}

#[derive(Serialize, Deserialize, Zeroize)]
struct PersistedTelegramSecret {
    bot_token: String,
    storage_chat_id: i64,
}
fn short_id(provider_id: &str) -> &str {
    provider_id.get(..8).unwrap_or(provider_id)
}

fn gdrive_state_dir(data_dir: &Path, provider_id: &str) -> PathBuf {
    data_dir
        .join("providers")
        .join(format!("gdrive_{}", short_id(provider_id)))
}

fn telegram_db_path(data_dir: &Path, provider_id: &str) -> PathBuf {
    data_dir
        .join("providers")
        .join(format!("tg_{}.db", short_id(provider_id)))
}

fn providers_index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("providers").join("providers.json")
}

fn credential_path(data_dir: &Path, provider_id: &str) -> PathBuf {
    data_dir
        .join("providers")
        .join(format!("cred_{provider_id}.bin"))
}

/// Writes `bytes` to `path` via a temp file + rename so a crash can never
/// leave a half-written credential or index file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid persistence path {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;

    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Cannot replace {}: {e}", path.display()));
    }
    Ok(())
}

fn load_provider_index(data_dir: &Path) -> Vec<PersistedProvider> {
    let path = providers_index_path(data_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!("Could not read provider index {}: {e}", path.display());
            return Vec::new();
        }
    };

    match serde_json::from_slice::<Vec<PersistedProvider>>(&bytes) {
        Ok(entries) => entries,
        Err(e) => {
            // Preserve the damaged file instead of silently overwriting it.
            let backup =
                path.with_extension(format!("json.corrupt-{}", chrono::Utc::now().timestamp()));
            let _ = std::fs::rename(&path, &backup);
            tracing::error!(
                "Provider index {} is corrupt ({e}); preserved at {}",
                path.display(),
                backup.display()
            );
            Vec::new()
        }
    }
}

fn save_provider_index(data_dir: &Path, entries: &[PersistedProvider]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|e| format!("Cannot serialize provider index: {e}"))?;
    write_atomic(&providers_index_path(data_dir), &bytes)
}

fn upsert_provider_index(data_dir: &Path, entry: PersistedProvider) -> Result<(), String> {
    let mut entries = load_provider_index(data_dir);
    match entries.iter_mut().find(|existing| existing.id == entry.id) {
        Some(existing) => *existing = entry,
        None => entries.push(entry),
    }
    save_provider_index(data_dir, &entries)
}

fn remove_provider_index(data_dir: &Path, provider_id: &str) -> Result<(), String> {
    let mut entries = load_provider_index(data_dir);
    let before = entries.len();
    entries.retain(|entry| entry.id != provider_id);
    if entries.len() == before {
        return Ok(());
    }
    save_provider_index(data_dir, &entries)
}

fn persist_credential<T: Serialize>(
    data_dir: &Path,
    provider_id: &str,
    payload: &T,
) -> Result<(), String> {
    let mut plaintext = serde_json::to_vec(payload)
        .map_err(|e| format!("Cannot serialize provider credentials: {e}"))?;

    let protected = virtualssd_security::protect_secret_dpapi(&plaintext)
        .map_err(|e| format!("Windows DPAPI could not protect the provider credentials: {e}"));
    plaintext.zeroize();

    let protected = protected?;
    write_atomic(&credential_path(data_dir, provider_id), &protected)
}

fn load_credential<T: DeserializeOwned>(data_dir: &Path, provider_id: &str) -> Result<T, String> {
    let path = credential_path(data_dir, provider_id);
    let protected =
        std::fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut plaintext = virtualssd_security::unprotect_secret_dpapi(&protected)
        .map_err(|e| format!("Windows DPAPI could not decrypt {}: {e}", path.display()))?;

    let decoded = serde_json::from_slice(&plaintext);
    plaintext.zeroize();

    decoded.map_err(|e| format!("Credential file {} is corrupt: {e}", path.display()))
}

/// Persists credentials first, then the index entry. If the index cannot be
/// written the credential file is removed so no orphaned secret is left behind.
fn persist_provider<T: Serialize + Zeroize>(
    data_dir: &Path,
    entry: PersistedProvider,
    mut payload: T,
) -> Result<(), String> {
    let provider_id = entry.id.clone();

    if let Err(e) = persist_credential(data_dir, &provider_id, &payload) {
        payload.zeroize();
        return Err(e);
    }

    if let Err(e) = upsert_provider_index(data_dir, entry) {
        let _ = std::fs::remove_file(credential_path(data_dir, &provider_id));
        payload.zeroize();
        return Err(e);
    }

    payload.zeroize();
    Ok(())
}

fn build_google_provider(
    data_dir: &Path,
    provider_id: &str,
    secret: PersistedGDriveSecret,
) -> Result<Arc<dyn CloudStorageProvider>, String> {
    let PersistedGDriveSecret {
        client_id,
        client_secret,
        refresh_token,
        code_verifier,
        code_challenge,
    } = secret;

    let credentials = GoogleDriveCredentials {
        client_id,
        client_secret,
        refresh_token,
        // Access tokens are short-lived; the refresh token restores access.
        access_token: None,
        token_expiry: None,
        code_verifier,
        code_challenge,
    };

    Ok(Arc::new(
        GoogleDriveProvider::new(credentials)
            .with_state_dir(gdrive_state_dir(data_dir, provider_id)),
    ))
}

fn build_telegram_provider(
    data_dir: &Path,
    provider_id: &str,
    secret: PersistedTelegramSecret,
) -> Result<Arc<dyn CloudStorageProvider>, String> {
    let config = TelegramConfig {
        bot_token: Some(secret.bot_token),
        api_id: None,
        api_hash: None,
        phone_number: None,
        storage_chat_id: secret.storage_chat_id,
        session_db_path: telegram_db_path(data_dir, provider_id),
    };

    let provider = TelegramProvider::new(config)
        .map_err(|e| format!("Cannot reopen Telegram provider: {e}"))?;
    Ok(Arc::new(provider))
}

/// Restores encrypted providers at startup. No network call is made: each
/// provider is reported as disconnected until the user verifies it.
fn restore_providers(
    data_dir: &Path,
) -> (
    HashMap<String, Arc<dyn CloudStorageProvider>>,
    Vec<ProviderRecord>,
) {
    let mut live: HashMap<String, Arc<dyn CloudStorageProvider>> = HashMap::new();
    let mut records: Vec<ProviderRecord> = Vec::new();

    for entry in load_provider_index(data_dir) {
        let restored = match entry.kind {
            ProviderKind::GoogleDrive => {
                load_credential::<PersistedGDriveSecret>(data_dir, &entry.id)
                    .and_then(|secret| build_google_provider(data_dir, &entry.id, secret))
            }
            ProviderKind::Telegram => {
                load_credential::<PersistedTelegramSecret>(data_dir, &entry.id)
                    .and_then(|secret| build_telegram_provider(data_dir, &entry.id, secret))
            }
        };

        match restored {
            Ok(provider) => {
                tracing::info!(
                    "Restored encrypted provider '{}' ({})",
                    entry.display_name,
                    short_id(&entry.id)
                );
                live.insert(entry.id.clone(), provider);
                records.push(ProviderRecord {
                    id: entry.id,
                    kind: entry.kind,
                    display_name: entry.display_name,
                    connected: false,
                    error_message: Some(
                        "Credentials restored from encrypted storage; connection not verified since restart"
                            .into(),
                    ),
                    total_quota_bytes: 0,
                    used_quota_bytes: 0,
                });
            }
            Err(e) => {
                tracing::error!(
                    "Could not restore provider '{}' ({}): {e}",
                    entry.display_name,
                    short_id(&entry.id)
                );
                records.push(ProviderRecord {
                    id: entry.id,
                    kind: entry.kind,
                    display_name: entry.display_name,
                    connected: false,
                    error_message: Some(format!("Credentials could not be restored: {e}")),
                    total_quota_bytes: 0,
                    used_quota_bytes: 0,
                });
            }
        }
    }

    (live, records)
}

// -- Application State --------------------------------------------------------

pub struct AppState {
    data_dir: PathBuf,
    disks: RwLock<HashMap<Uuid, Arc<VirtualDiskEngine>>>,
    live_providers: RwLock<HashMap<String, Arc<dyn CloudStorageProvider>>>,
    provider_records: RwLock<Vec<ProviderRecord>>,
    disk_provider_map: RwLock<HashMap<Uuid, String>>,
    settings: RwLock<AppSettings>,
    settings_warning: RwLock<Option<String>>,
    mount_lock: tokio::sync::Mutex<()>,
    /// Serializes provider-index/credential writes across IPC commands.
    persist_lock: tokio::sync::Mutex<()>,
}

#[derive(Serialize)]
pub struct RuntimeStatus {
    pub elevated: bool,
    /// The shipped application version, e.g. `1.0.0+42`. This is the exact
    /// version baked into the Windows resources and the installers, so the UI
    /// never shows a version that does not match the running binary.
    pub version: String,
    pub build: String,
    pub company: String,
    pub developer: String,
}

#[derive(Serialize)]
pub struct AppStats {
    pub total_capacity_bytes: u64,
    pub total_allocated_bytes: u64,
    pub total_cached_bytes: u64,
    pub total_dirty_bytes: u64,
    pub active_disks_count: usize,
    pub mounted_disks_count: usize,
}

fn integrity_report_for(engine: &VirtualDiskEngine) -> IntegrityReport {
    let audit = engine.verify_integrity();
    let passed = audit.passed();
    let telemetry = engine.telemetry();
    IntegrityReport {
        disk_id: telemetry.disk_id.to_string(),
        disk_name: telemetry.name,
        verified_chunks: audit.verified_chunks,
        missing_chunks: audit.missing_chunks,
        total_chunks: audit.total_chunks,
        errors: audit
            .errors
            .into_iter()
            .map(|(idx, msg)| format!("chunk {idx}: {msg}"))
            .collect(),
        passed,
        disk_error: telemetry.last_error,
    }
}

/// Validates the parts of a remotely fetched manifest that this build relies
/// on. A malformed manifest must be rejected before it becomes a local disk.
fn validate_import_manifest(manifest: &DiskManifest) -> Result<(), String> {
    if manifest.disk_id.is_nil() {
        return Err("Remote manifest has a nil disk id".into());
    }
    if manifest.name.trim().is_empty() {
        return Err("Remote manifest has an empty disk name".into());
    }
    if manifest.geometry.chunk_size != DEFAULT_CHUNK_SIZE {
        return Err(format!(
            "Remote manifest uses a {}-byte chunk size; this build supports {DEFAULT_CHUNK_SIZE}",
            manifest.geometry.chunk_size
        ));
    }
    if manifest.geometry.total_capacity_bytes == 0 {
        return Err("Remote manifest reports a zero-capacity disk".into());
    }

    let chunk_size = manifest.geometry.chunk_size as u64;
    match manifest.vhdx_file_size {
        Some(file_size) => {
            if file_size == 0 {
                return Err("Remote manifest reports an empty VHDX snapshot".into());
            }
            // A dynamic VHDX must fit inside the virtual disk plus its metadata.
            if file_size > manifest.geometry.total_capacity_bytes + 64 * 1024 * 1024 {
                return Err(format!(
                    "Remote VHDX size {file_size} exceeds the disk capacity {}",
                    manifest.geometry.total_capacity_bytes
                ));
            }
            let expected_chunks = file_size.div_ceil(chunk_size);
            for (&index, descriptor) in &manifest.vhdx_chunks {
                if descriptor.chunk_index != index {
                    return Err(format!(
                        "VHDX chunk map key {index} disagrees with descriptor index {}",
                        descriptor.chunk_index
                    ));
                }
                if index >= expected_chunks {
                    return Err(format!(
                        "VHDX chunk #{index} is outside the {expected_chunks}-chunk snapshot"
                    ));
                }
                let offset = index * chunk_size;
                let expected_length =
                    ((file_size - offset) as usize).min(manifest.geometry.chunk_size);
                if descriptor.length != expected_length {
                    return Err(format!(
                        "VHDX chunk #{index} length {} does not match the snapshot length {expected_length}",
                        descriptor.length
                    ));
                }
                if descriptor.remote_object_id.is_none() {
                    return Err(format!("VHDX chunk #{index} has no remote object id"));
                }
                if descriptor.is_encrypted != manifest.encrypt_chunks {
                    return Err(format!(
                        "VHDX chunk #{index} encryption flag disagrees with the disk manifest"
                    ));
                }
                if descriptor.is_encrypted && descriptor.remote_checksum.is_none() {
                    return Err(format!(
                        "Encrypted VHDX chunk #{index} has no ciphertext checksum"
                    ));
                }
            }
        }
        None => {
            if !manifest.vhdx_chunks.is_empty() {
                return Err(
                    "Remote manifest contains VHDX chunks but no VHDX snapshot size".into(),
                );
            }
        }
    }

    for (&index, descriptor) in &manifest.chunks {
        if descriptor.chunk_index != index {
            return Err(format!(
                "Chunk map key {index} disagrees with descriptor index {}",
                descriptor.chunk_index
            ));
        }
    }

    Ok(())
}

// -- Provider IPC Commands ----------------------------------------------------

#[tauri::command]
async fn add_google_drive_provider(
    state: tauri::State<'_, Arc<AppState>>,
    client_id: String,
    refresh_token: String,
    display_name: String,
) -> Result<ProviderRecord, String> {
    let provider_id = Uuid::new_v4().to_string();
    // Generate PKCE values for the auth flow
    let (code_verifier, code_challenge) = GoogleDriveProvider::generate_pkce();

    // Build the durable payload before the input strings are moved into the
    // live provider. Nothing is persisted until verification succeeds.
    let persisted_secret = PersistedGDriveSecret {
        client_id: client_id.clone(),
        client_secret: None,
        refresh_token: refresh_token.clone(),
        code_verifier: Some(code_verifier.clone()),
        code_challenge: Some(code_challenge.clone()),
    };

    let creds = GoogleDriveCredentials {
        client_id,
        client_secret: None,
        refresh_token,
        access_token: None,
        token_expiry: None,
        code_verifier: Some(code_verifier),
        code_challenge: Some(code_challenge),
    };
    let provider = Arc::new(
        GoogleDriveProvider::new(creds)
            .with_state_dir(gdrive_state_dir(&state.data_dir, &provider_id)),
    );
    let (connected, error_message, total_quota, used_quota) =
        match provider.verify_connection().await {
            Ok(_) => {
                let (total, used) = match provider.get_quota().await {
                    Ok(q) => (q.total_bytes, q.used_bytes),
                    Err(e) => {
                        tracing::warn!("Google Drive connected but quota is unavailable: {e}");
                        (0u64, 0u64)
                    }
                };
                (true, None, total, used)
            }
            Err(e) => {
                tracing::error!(
                    target: "virtualssd_desktop::providers",
                    "Google Drive verification failed: {e}"
                );
                return Err(format!("Google Drive verification failed: {e}"));
            }
        };

    // Store the refresh token with DPAPI only after the credential proved it
    // works. A persistence failure is reported instead of pretending the
    // provider will survive a restart.
    {
        let _guard = state.persist_lock.lock().await;
        persist_provider(
            &state.data_dir,
            PersistedProvider {
                id: provider_id.clone(),
                kind: ProviderKind::GoogleDrive,
                display_name: display_name.clone(),
            },
            persisted_secret,
        )
        .map_err(|e| {
            format!("Google Drive verified, but credentials could not be stored securely: {e}")
        })?;
    }

    let record = ProviderRecord {
        id: provider_id.clone(),
        kind: ProviderKind::GoogleDrive,
        display_name,
        connected,
        error_message,
        total_quota_bytes: total_quota,
        used_quota_bytes: used_quota,
    };
    state
        .live_providers
        .write()
        .insert(provider_id.clone(), provider);
    state.provider_records.write().push(record.clone());
    tracing::info!(
        target: "virtualssd_desktop::providers",
        "Google Drive provider '{}' connected ({}); quota reported: {} of {} bytes",
        record.display_name,
        short_id(&record.id),
        record.used_quota_bytes,
        record.total_quota_bytes
    );
    Ok(record)
}

#[tauri::command]
async fn add_telegram_provider(
    state: tauri::State<'_, Arc<AppState>>,
    bot_token: String,
    storage_chat_id: i64,
    display_name: String,
) -> Result<ProviderRecord, String> {
    let provider_id = Uuid::new_v4().to_string();

    // Keep a copy for encrypted persistence; the live config is zeroized when
    // the provider is dropped.
    let persisted_secret = PersistedTelegramSecret {
        bot_token: bot_token.clone(),
        storage_chat_id,
    };

    let config = TelegramConfig {
        bot_token: Some(bot_token),
        api_id: None,
        api_hash: None,
        phone_number: None,
        storage_chat_id,
        session_db_path: telegram_db_path(&state.data_dir, &provider_id),
    };
    let provider =
        Arc::new(TelegramProvider::new(config).map_err(|e| format!("Telegram init failed: {e}"))?);
    let (connected, error_message) = match provider.verify_connection().await {
        Ok(_) => (true, None),
        Err(e) => {
            tracing::error!(
                target: "virtualssd_desktop::providers",
                "Telegram verification failed: {e}"
            );
            return Err(format!("Telegram verification failed: {e}"));
        }
    };

    {
        let _guard = state.persist_lock.lock().await;
        persist_provider(
            &state.data_dir,
            PersistedProvider {
                id: provider_id.clone(),
                kind: ProviderKind::Telegram,
                display_name: display_name.clone(),
            },
            persisted_secret,
        )
        .map_err(|e| {
            format!("Telegram verified, but credentials could not be stored securely: {e}")
        })?;
    }

    let record = ProviderRecord {
        id: provider_id.clone(),
        kind: ProviderKind::Telegram,
        display_name,
        connected,
        error_message,
        // The Telegram Bot API does not expose account storage quota.
        total_quota_bytes: 0,
        used_quota_bytes: 0,
    };
    state
        .live_providers
        .write()
        .insert(provider_id.clone(), provider);
    state.provider_records.write().push(record.clone());
    tracing::info!(
        target: "virtualssd_desktop::providers",
        "Telegram provider '{}' connected ({}); Bot API exposes no storage quota",
        record.display_name,
        short_id(&record.id)
    );
    Ok(record)
}

#[tauri::command]
fn list_providers(state: tauri::State<Arc<AppState>>) -> Vec<ProviderRecord> {
    let disk_map = state.disk_provider_map.read();
    let mut records = state.provider_records.write();
    for r in records.iter_mut() {
        let _ = disk_map.values().filter(|pid| **pid == r.id).count();
    }
    records.clone()
}

#[tauri::command]
async fn verify_provider(
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
) -> Result<ProviderRecord, String> {
    let provider = {
        let providers = state.live_providers.read();
        providers.get(&provider_id).cloned()
    };
    let provider = provider
        .ok_or_else(|| "Provider not found in session. Re-add it to reconnect.".to_string())?;
    let (connected, error_message, total_quota, used_quota) =
        match provider.verify_connection().await {
            Ok(_) => {
                let (total, used) = match provider.get_quota().await {
                    Ok(q) => (q.total_bytes, q.used_bytes),
                    Err(_) => (0u64, 0u64),
                };
                (true, None, total, used)
            }
            Err(e) => (false, Some(e.to_string()), 0u64, 0u64),
        };
    let mut records = state.provider_records.write();
    if let Some(record) = records.iter_mut().find(|r| r.id == provider_id) {
        record.connected = connected;
        record.error_message = error_message;
        record.total_quota_bytes = total_quota;
        record.used_quota_bytes = used_quota;
        let connected = record.connected;
        let display_name = record.display_name.clone();
        let error_message = record.error_message.clone();
        let outcome = if connected {
            tracing::info!(
                target: "virtualssd_desktop::providers",
                "Provider '{display_name}' verified as connected ({}); quota reported: {} of {} bytes",
                short_id(&provider_id),
                record.used_quota_bytes,
                record.total_quota_bytes
            );
            "connected"
        } else {
            tracing::warn!(
                target: "virtualssd_desktop::providers",
                "Provider '{display_name}' could not be verified: {}",
                error_message.as_deref().unwrap_or("unknown error")
            );
            "offline"
        };
        let _ = outcome;
        Ok(record.clone())
    } else {
        Err("Provider record not found".to_string())
    }
}

#[tauri::command]
async fn remove_provider(
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
) -> Result<(), String> {
    let in_use = state
        .disk_provider_map
        .read()
        .values()
        .any(|pid| *pid == provider_id);
    if in_use {
        return Err(
            "Cannot remove: provider is still used by active disks. Delete those disks first."
                .to_string(),
        );
    }

    let _guard = state.persist_lock.lock().await;

    // Remove the index entry first so a failure cannot leave a credential file
    // referenced by a provider that no longer exists.
    remove_provider_index(&state.data_dir, &provider_id)?;

    let credential = credential_path(&state.data_dir, &provider_id);
    match std::fs::remove_file(&credential) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "Provider removed from the index, but its encrypted credential file {} could not be deleted: {e}",
                credential.display()
            ))
        }
    }

    // Drop the live provider (closing its SQLite handle) before removing its
    // provider-state files.
    state.live_providers.write().remove(&provider_id);
    state
        .provider_records
        .write()
        .retain(|r| r.id != provider_id);

    // Provider state (cursor databases, resumable session URIs) contains live
    // capabilities; remove it on disconnect. Failures are logged, not hidden.
    let session_dir = gdrive_state_dir(&state.data_dir, &provider_id);
    if let Err(e) = std::fs::remove_dir_all(&session_dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Could not remove Google Drive session state {}: {e}",
                session_dir.display()
            );
        }
    }
    let telegram_db = telegram_db_path(&state.data_dir, &provider_id);
    if let Err(e) = std::fs::remove_file(&telegram_db) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Could not remove Telegram cursor database {}: {e}",
                telegram_db.display()
            );
        }
    }

    tracing::warn!(
        target: "virtualssd_desktop::providers",
        "Provider ({}) removed from the registry; its encrypted credential and provider state were deleted",
        short_id(&provider_id)
    );

    Ok(())
}

// -- Disk Lifecycle IPC Commands -----------------------------------------------

#[tauri::command]
fn list_disks(state: tauri::State<Arc<AppState>>) -> Result<Vec<DiskTelemetry>, String> {
    Ok(state.disks.read().values().map(|d| d.telemetry()).collect())
}

#[tauri::command]
fn get_runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        elevated: windows_vhd::is_elevated(),
        version: env!("VIRTUALSSD_BUILD_VERSION").to_string(),
        build: env!("VIRTUALSSD_BUILD_NUMBER").to_string(),
        company: "Chaudary Global".to_string(),
        developer: "Ahmad Abdullah Chaudary".to_string(),
    }
}

#[tauri::command]
fn get_app_stats(state: tauri::State<Arc<AppState>>) -> Result<AppStats, String> {
    let disks = state.disks.read();
    let mut total_cap = 0u64;
    let mut total_alloc = 0u64;
    let mut total_cached = 0u64;
    let mut total_dirty = 0u64;
    let mut mounted = 0usize;
    for disk in disks.values() {
        let t = disk.telemetry();
        total_cap += t.capacity_bytes;
        total_alloc += t.allocated_chunks * DEFAULT_CHUNK_SIZE as u64;
        total_cached += t.cached_bytes;
        total_dirty += t.dirty_bytes;
        if t.physical_drive_path.is_some() {
            mounted += 1;
        }
    }
    Ok(AppStats {
        total_capacity_bytes: total_cap,
        total_allocated_bytes: total_alloc,
        total_cached_bytes: total_cached,
        total_dirty_bytes: total_dirty,
        active_disks_count: disks.len(),
        mounted_disks_count: mounted,
    })
}

#[tauri::command]
fn get_settings(state: tauri::State<Arc<AppState>>) -> SettingsSnapshot {
    SettingsSnapshot {
        settings: state.settings.read().clone(),
        warning: state.settings_warning.read().clone(),
    }
}

#[tauri::command]
fn update_settings(
    state: tauri::State<Arc<AppState>>,
    settings: AppSettings,
) -> Result<SettingsSnapshot, String> {
    settings.validate()?;
    settings::save(&state.data_dir, &settings)?;

    let mut warnings = Vec::new();
    {
        let disks = state.disks.read();
        for (disk_id, engine) in disks.iter() {
            if let Err(e) = engine.set_cache_limit(settings.cache_limit_bytes()) {
                warnings.push(format!(
                    "Disk {} could not apply the new cache limit: {e}",
                    &disk_id.to_string()[..8]
                ));
            }
        }
    }

    *state.settings.write() = settings.clone();
    *state.settings_warning.write() = None;

    tracing::info!(
        target: "virtualssd_desktop::settings",
        "Settings saved (cache limit {} GiB, telemetry interval {} ms, encrypt new disks: {}, verify after sync: {})",
        settings.cache_limit_gb,
        settings.telemetry_poll_interval_ms,
        settings.encrypt_new_disks,
        settings.verify_after_sync
    );
    for warning in &warnings {
        tracing::warn!(target: "virtualssd_desktop::settings", "{warning}");
    }

    Ok(SettingsSnapshot {
        settings,
        warning: if warnings.is_empty() {
            None
        } else {
            Some(format!(
                "Settings were saved, but some live disks could not shrink their cache: {}",
                warnings.join("; ")
            ))
        },
    })
}

#[tauri::command]
fn restart_elevated() -> Result<(), String> {
    if windows_vhd::is_elevated() {
        return Err("VirtualSSD is already running as administrator.".to_string());
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("Could not locate the VirtualSSD executable: {e}"))?;
    let wide: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let result = unsafe {
        ShellExecuteW(
            windows::Win32::Foundation::HWND::default(),
            w!("runas"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    let code = result.0 as isize;
    if code <= 32 {
        let detail = windows::core::Error::from_win32();
        tracing::warn!(
            target: "virtualssd_desktop::windows-vhd",
            "Windows refused the elevated relaunch (ShellExecuteW returned {code}): {detail}"
        );
        return Err(format!(
            "Windows did not start an elevated instance (ShellExecuteW returned {code}): {detail}"
        ));
    }

    tracing::warn!(
        target: "virtualssd_desktop::windows-vhd",
        "Requested an elevated relaunch through the Windows UAC prompt"
    );

    // Give the elevated instance a moment to start, then exit so two instances
    // are not reading and writing the same disk journals.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(900));
        std::process::exit(0);
    });

    Ok(())
}

/// Imports a disk manifest from a connected provider. The manifest is fetched
/// before any local directory is created, so a failed or malformed fetch leaves
/// the filesystem untouched. The VHDX itself is restored lazily on mount.
#[tauri::command]
async fn import_disk(
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
    disk_id: String,
) -> Result<ImportReport, String> {
    let id = Uuid::parse_str(disk_id.trim())
        .map_err(|e| format!("'{disk_id}' is not a valid disk id: {e}"))?;

    let provider = {
        let providers = state.live_providers.read();
        providers.get(&provider_id).cloned()
    }
    .ok_or_else(|| {
        "Provider is not connected in this session. Verify it on the Providers page first."
            .to_string()
    })?;

    let provider_type = {
        let records = state.provider_records.read();
        records
            .iter()
            .find(|record| record.id == provider_id)
            .map(|record| match record.kind {
                ProviderKind::GoogleDrive => ProviderType::GoogleDrive,
                ProviderKind::Telegram => ProviderType::Telegram,
            })
            .ok_or_else(|| "Provider record not found".to_string())?
    };

    let mut manifest = provider
        .fetch_manifest(id)
        .await
        .map_err(|e| format!("Could not fetch manifest {id} from the provider: {e}"))?;

    if manifest.disk_id != id {
        return Err(format!(
            "Provider returned manifest for disk {}, not the requested {id}",
            manifest.disk_id
        ));
    }
    validate_import_manifest(&manifest)?;

    if state.disks.read().contains_key(&id) {
        return Err(format!("Disk {id} is already open in this session."));
    }

    // Rebind the imported disk to the provider selected on this machine. The
    // original account id belongs to the installation that published it.
    manifest.provider_type = provider_type;
    manifest.provider_account_id = provider_id.clone();

    let disk_dir = state.data_dir.join("disks").join(id.to_string());
    match std::fs::create_dir(&disk_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!(
                "A disk with id {id} already exists locally at {}. Delete it first, or import a different disk.",
                disk_dir.display()
            ))
        }
        Err(e) => {
            return Err(format!(
                "Could not create the disk directory {}: {e}",
                disk_dir.display()
            ))
        }
    }

    if state.disks.read().contains_key(&id) {
        let _ = std::fs::remove_dir_all(&disk_dir);
        return Err(format!(
            "Disk {id} was opened by another operation; import aborted."
        ));
    }

    let cache_limit = state.settings.read().cache_limit_bytes();
    let engine =
        match VirtualDiskEngine::open(&disk_dir, manifest.clone(), Some(provider), cache_limit) {
            Ok(engine) => engine,
            Err(e) => {
                let cleanup = std::fs::remove_dir_all(&disk_dir);
                let suffix = match cleanup {
                    Ok(()) => String::new(),
                    Err(cleanup_error) => format!(
                        " The partial directory {} could not be removed: {cleanup_error}",
                        disk_dir.display()
                    ),
                };
                return Err(format!("Could not open the imported disk: {e}.{suffix}"));
            }
        };

    let telemetry = engine.telemetry();
    let requires_recovery_key = manifest.encrypt_chunks && !engine.encryption_key_present();
    let has_vhdx_snapshot = manifest.vhdx_file_size.is_some();
    let mut warnings = Vec::new();
    if requires_recovery_key {
        warnings.push(
            "This disk is encrypted. Import its 64-character recovery key before mounting or syncing it."
                .to_string(),
        );
    }
    if !has_vhdx_snapshot {
        warnings.push(
            "The remote manifest has no VHDX snapshot yet. Mounting will create a new blank VHDX."
                .to_string(),
        );
    }

    let arc_engine = Arc::new(engine);
    state.disks.write().insert(id, arc_engine);
    state
        .disk_provider_map
        .write()
        .insert(id, provider_id.clone());

    tracing::info!(
        target: "virtualssd_desktop::manifest",
        "Imported disk '{}' ({id}) from provider ({}); {} VHDX chunk descriptor(s), snapshot size {} bytes",
        manifest.name,
        short_id(&provider_id),
        manifest.vhdx_chunks.len(),
        manifest.vhdx_file_size.unwrap_or(0)
    );
    for warning in &warnings {
        tracing::warn!(target: "virtualssd_desktop::manifest", "{warning}");
    }

    Ok(ImportReport {
        disk: telemetry,
        has_vhdx_snapshot,
        vhdx_file_size: manifest.vhdx_file_size.unwrap_or(0),
        vhdx_chunks: manifest.vhdx_chunks.len(),
        requires_recovery_key,
        warnings,
    })
}

#[tauri::command]
fn export_disk_key(state: tauri::State<Arc<AppState>>, disk_id: String) -> Result<String, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };
    engine
        .export_recovery_key()
        .map_err(|e| e.to_string())
        .inspect(|_key| {
            tracing::warn!(
                target: "virtualssd_desktop::security",
                "Recovery key exported for disk {id}; the key itself is never written to the log"
            );
        })
}

#[tauri::command]
fn import_disk_key(
    state: tauri::State<Arc<AppState>>,
    disk_id: String,
    recovery_key: String,
) -> Result<DiskTelemetry, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };
    engine
        .import_recovery_key(&recovery_key)
        .map_err(|e| e.to_string())?;
    tracing::info!(
        target: "virtualssd_desktop::security",
        "Recovery key imported for disk {id}; the key itself is never written to the log"
    );
    Ok(engine.telemetry())
}

/// Creates a virtual disk bound to a real provider. provider_account_id must
/// match an ID returned by add_google_drive_provider or add_telegram_provider.
#[tauri::command]
async fn create_disk(
    state: tauri::State<'_, Arc<AppState>>,
    name: String,
    provider_account_id: String,
    capacity_gb: u64,
) -> Result<DiskTelemetry, String> {
    let provider: Option<Arc<dyn CloudStorageProvider>> = {
        let providers = state.live_providers.read();
        providers.get(&provider_account_id).cloned()
    };
    let provider = provider.ok_or_else(|| {
        format!(
            "Provider '{}' not connected. Add a Google Drive or Telegram account on the Providers page first.",
            provider_account_id
        )
    })?;
    let provider_type = {
        let records = state.provider_records.read();
        records
            .iter()
            .find(|r| r.id == provider_account_id)
            .map(|r| match r.kind {
                ProviderKind::GoogleDrive => ProviderType::GoogleDrive,
                ProviderKind::Telegram => ProviderType::Telegram,
            })
            .unwrap_or(ProviderType::Local)
    };
    let capacity_bytes = capacity_gb
        .checked_mul(1024 * 1024 * 1024)
        .ok_or_else(|| "Capacity is too large to represent in bytes".to_string())?;
    if capacity_bytes < 1024 * 1024 * 1024 {
        return Err("Capacity must be at least 1 GB".to_string());
    }
    if capacity_gb > settings::CAPACITY_MAX_GB {
        return Err(format!(
            "Capacity must not exceed {} GB",
            settings::CAPACITY_MAX_GB
        ));
    }

    let settings = state.settings.read().clone();
    let mut manifest = DiskManifest::new(
        name.clone(),
        provider_type,
        provider_account_id.clone(),
        capacity_bytes,
        DEFAULT_CHUNK_SIZE,
    )
    .map_err(|e| e.to_string())?;
    manifest.encrypt_chunks = settings.encrypt_new_disks;
    let disk_id = manifest.disk_id;
    let disk_dir = state.data_dir.join("disks").join(disk_id.to_string());
    let engine = match VirtualDiskEngine::open(
        &disk_dir,
        manifest,
        Some(provider.clone()),
        settings.cache_limit_bytes(),
    ) {
        Ok(engine) => engine,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&disk_dir);
            return Err(format!("Could not create the disk engine: {e}"));
        }
    };

    // An encrypted disk must never exist without its key, otherwise its first
    // sync would upload chunks that can no longer be decrypted.
    if settings.encrypt_new_disks {
        if let Err(e) = engine.initialize_encryption_key() {
            drop(engine);
            let cleanup = std::fs::remove_dir_all(&disk_dir);
            let suffix = match cleanup {
                Ok(()) => String::new(),
                Err(cleanup_error) => format!(
                    " The partial directory {} could not be removed: {cleanup_error}",
                    disk_dir.display()
                ),
            };
            return Err(format!(
                "Disk was not created because its encryption key could not be stored: {e}.{suffix}"
            ));
        }
    }

    // Publish the initial manifest before reporting success. A disk whose
    // manifest never reached the provider cannot be rediscovered elsewhere,
    // so roll the local creation back instead of claiming success.
    if let Err(e) = provider.publish_manifest(&engine.manifest_snapshot()).await {
        drop(engine);
        if let Err(cleanup) = std::fs::remove_dir_all(&disk_dir) {
            return Err(format!(
                "Initial manifest publish failed ({e}) and the local disk directory could not be removed ({cleanup}). Remove {} manually.",
                disk_dir.display()
            ));
        }
        return Err(format!(
            "Disk was not created because its initial manifest could not be published: {e}"
        ));
    }

    let arc_engine = Arc::new(engine);
    state.disks.write().insert(disk_id, arc_engine.clone());
    state
        .disk_provider_map
        .write()
        .insert(disk_id, provider_account_id.clone());
    tracing::info!(
        target: "virtualssd_desktop::manifest",
        "Created disk '{name}' ({disk_id}) with a capacity of {capacity_gb} GB on provider ({}); chunk encryption: {}",
        short_id(&provider_account_id),
        settings.encrypt_new_disks
    );
    Ok(arc_engine.telemetry())
}

fn parse_requested_drive_letter(value: Option<String>) -> Result<Option<char>, String> {
    let Some(value) = value else { return Ok(None) };
    let trimmed = value.trim().trim_end_matches(':').trim();
    if trimmed.len() != 1 {
        return Err(format!(
            "Invalid drive letter '{value}'. Use a single letter such as V:"
        ));
    }
    let letter = trimmed.chars().next().unwrap().to_ascii_uppercase();
    if !letter.is_ascii_uppercase() {
        return Err(format!("Invalid drive letter '{value}'. Use A-Z."));
    }
    Ok(Some(letter))
}

#[tauri::command]
async fn mount_disk(
    state: tauri::State<'_, Arc<AppState>>,
    disk_id: String,
    drive_letter: Option<String>,
) -> Result<DiskTelemetry, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };

    if engine.telemetry().status == DiskStatus::Online {
        return Err("Disk is already mounted. Unmount it before mounting again.".to_string());
    }
    if !windows_vhd::is_elevated() {
        return Err(
            "Mounting a virtual disk requires administrator privileges. Restart VirtualSSD as administrator."
                .to_string(),
        );
    }

    let requested = parse_requested_drive_letter(drive_letter)?;
    let letter = match requested {
        Some(letter) => windows_vhd::validate_drive_letter(letter)?,
        None => windows_vhd::choose_available_drive_letter()?,
    };

    tracing::info!(
        target: "virtualssd_desktop::windows-vhd",
        "Mounting disk {id} through the Windows Virtual Disk API as drive {letter}:"
    );

    let _guard = state.mount_lock.lock().await;

    if engine.telemetry().status == DiskStatus::Online {
        return Err("Disk is already mounted. Unmount it before mounting again.".to_string());
    }

    // If the local VHDX is missing but the manifest has a remote snapshot,
    // restore it before touching Windows. Creating a blank VHDX here would
    // silently discard the user's data.
    if !engine.vhdx_path().exists() {
        let has_snapshot = engine.manifest_snapshot().vhdx_file_size.is_some();
        if has_snapshot {
            engine.set_status(DiskStatus::Recovering);
            let restore = engine.restore_vhdx_from_remote().await;
            engine.set_status(DiskStatus::Unmounted);
            match restore {
                Ok(report) => {
                    tracing::info!(
                        target: "virtualssd_desktop::storage-core",
                        "Restored the missing VHDX for disk {id} from the provider ({} chunk(s), {} bytes)",
                        report.downloaded_chunks,
                        report.file_size_bytes
                    );
                }
                Err(e) => {
                    let message = format!(
                        "The local VHDX is missing and could not be restored from the provider: {e}"
                    );
                    engine.set_last_error(Some(message.clone()));
                    tracing::error!(
                        target: "virtualssd_desktop::storage-core",
                        "{message}"
                    );
                    return Err(message);
                }
            }
        }
    }

    let engine_for_task = engine.clone();
    tokio::task::spawn_blocking(move || -> Result<DiskTelemetry, String> {
        let vhdx = engine_for_task.vhdx_path();
        if !vhdx.exists() {
            windows_vhd::create_vhdx(&vhdx, engine_for_task.capacity_bytes())?;
        }

        // A previous process may have crashed while the image was attached.
        if let Err(e) = windows_vhd::detach_vhdx(&vhdx) {
            tracing::warn!("Could not clear stale VHDX attachment: {e}");
        }

        let attached = match windows_vhd::attach_vhdx(&vhdx) {
            Ok(attached) => attached,
            Err(e) => {
                engine_for_task.set_status(DiskStatus::Unmounted);
                engine_for_task.set_drive_letter(None);
                engine_for_task.set_physical_drive_path(None);
                engine_for_task.set_last_error(Some(e.clone()));
                tracing::error!(
                    target: "virtualssd_desktop::windows-vhd",
                    "Windows could not attach the VHDX for disk {id}: {e}"
                );
                return Err(e);
            }
        };

        let disk_name = engine_for_task.telemetry().name;
        match windows_vhd::format_volume(attached.disk_number, &disk_name, Some(letter)) {
            Ok(assigned) => {
                engine_for_task.set_status(DiskStatus::Online);
                engine_for_task.set_drive_letter(Some(format!("{assigned}:")));
                engine_for_task.set_physical_drive_path(Some(attached.device_path.clone()));
                engine_for_task.set_last_error(None);
                tracing::info!(
                    target: "virtualssd_desktop::windows-vhd",
                    "Disk {id} attached and formatted as drive {assigned}:"
                );
                Ok(engine_for_task.telemetry())
            }
            Err(e) => {
                let _ = windows_vhd::detach_vhdx(&vhdx);
                engine_for_task.set_status(DiskStatus::Unmounted);
                engine_for_task.set_drive_letter(None);
                engine_for_task.set_physical_drive_path(None);
                engine_for_task.set_last_error(Some(e.clone()));
                tracing::error!(
                    target: "virtualssd_desktop::windows-vhd",
                    "Windows could not format the volume for disk {id}: {e}"
                );
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| format!("Mount task failed: {e}"))?
}

#[tauri::command]
async fn unmount_disk(
    state: tauri::State<'_, Arc<AppState>>,
    disk_id: String,
) -> Result<DiskTelemetry, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };

    let _guard = state.mount_lock.lock().await;
    let vhdx = engine.vhdx_path();

    tokio::task::spawn_blocking(move || -> Result<DiskTelemetry, String> {
        let telemetry = engine.telemetry();
        if telemetry.status != DiskStatus::Online && telemetry.physical_drive_path.is_none() {
            return Ok(telemetry);
        }

        match windows_vhd::detach_vhdx(&vhdx) {
            Ok(()) => {
                engine.set_status(DiskStatus::Unmounted);
                engine.set_drive_letter(None);
                engine.set_physical_drive_path(None);
                engine.set_last_error(None);
                tracing::info!(
                    target: "virtualssd_desktop::windows-vhd",
                    "Detached disk {id} from the Windows Virtual Disk API"
                );
                Ok(engine.telemetry())
            }
            Err(e) => {
                engine.set_last_error(Some(e.clone()));
                tracing::error!(
                    target: "virtualssd_desktop::windows-vhd",
                    "Windows could not detach disk {id}: {e}"
                );
                Err(format!(
                    "The disk is still attached because Windows could not detach it: {e}"
                ))
            }
        }
    })
    .await
    .map_err(|e| format!("Unmount task failed: {e}"))?
}

#[tauri::command]
async fn sync_disk(
    state: tauri::State<'_, Arc<AppState>>,
    disk_id: String,
) -> Result<SyncDiskReport, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };
    if engine.provider().is_none() {
        return Err("Sync requires a connected cloud provider. Add a Google Drive or Telegram account first.".to_string());
    }

    // Serialize with mount/unmount/delete: a snapshot must only be taken while
    // the disk is genuinely detached and no delete is in flight.
    let _guard = state.mount_lock.lock().await;
    tracing::info!(
        target: "virtualssd_desktop::storage-core",
        "Mirroring the VHDX snapshot of disk {id} to the provider"
    );
    let mirror = engine
        .sync_vhdx_snapshot()
        .await
        .map_err(|e| e.to_string())?;

    for warning in &mirror.warnings {
        tracing::warn!(target: "virtualssd_desktop::storage-core", "{warning}");
    }
    tracing::info!(
        target: "virtualssd_desktop::storage-core",
        "Mirror finished for disk {id}: {} uploaded / {} deleted / {} unchanged chunk(s), {} byte(s)",
        mirror.uploaded_chunks,
        mirror.deleted_chunks,
        mirror.skipped_chunks,
        mirror.file_size_bytes
    );

    let verify_after_sync = state.settings.read().verify_after_sync;
    let integrity = if verify_after_sync {
        Some(integrity_report_for(&engine))
    } else {
        None
    };

    if let Some(audit) = &integrity {
        if audit.passed {
            tracing::info!(
                target: "virtualssd_desktop::integrity",
                "Post-sync audit for disk {id}: {}/{} chunk(s) verified locally, {} not present locally",
                audit.verified_chunks,
                audit.total_chunks,
                audit.missing_chunks
            );
        } else {
            tracing::error!(
                target: "virtualssd_desktop::integrity",
                "Post-sync audit for disk {id} found {} checksum error(s): {}",
                audit.errors.len(),
                audit.errors.join("; ")
            );
        }
    }

    Ok(SyncDiskReport { mirror, integrity })
}

/// Mirrors a disk that is still mounted, using a volume shadow copy.
///
/// This is what makes VirtualSSD a persistent drive: the disk stays mounted
/// and usable while the mirror runs, because the transfer reads a point-in-time
/// image of the volume instead of the live file.
///
/// Creating a volume shadow copy requires administrator privileges.
#[tauri::command]
async fn sync_disk_live(
    state: tauri::State<'_, Arc<AppState>>,
    disk_id: String,
) -> Result<SyncDiskReport, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };
    if engine.provider().is_none() {
        return Err("Sync requires a connected cloud provider. Add a Google Drive or Telegram account first.".to_string());
    }

    if !windows_vhd::is_elevated() {
        return Err(
            "Mirroring a mounted disk uses a volume shadow copy, which requires administrator privileges. Restart VirtualSSD as administrator."
                .to_string(),
        );
    }

    // Serialize with mount/unmount/delete: a snapshot must not overlap a
    // detach or a delete.
    let _guard = state.mount_lock.lock().await;
    tracing::info!(
        target: "virtualssd_desktop::storage-core",
        "Mirroring the mounted disk {id} through a volume shadow copy"
    );

    let mirror = engine
        .sync_vhdx_snapshot_live()
        .await
        .map_err(|e| e.to_string())?;

    for warning in &mirror.warnings {
        tracing::warn!(target: "virtualssd_desktop::storage-core", "{warning}");
    }
    tracing::info!(
        target: "virtualssd_desktop::storage-core",
        "Live mirror finished for disk {id}: {} uploaded / {} deleted / {} unchanged chunk(s), {} byte(s)",
        mirror.uploaded_chunks,
        mirror.deleted_chunks,
        mirror.skipped_chunks,
        mirror.file_size_bytes
    );

    let verify_after_sync = state.settings.read().verify_after_sync;
    let integrity = if verify_after_sync {
        Some(integrity_report_for(&engine))
    } else {
        None
    };

    if let Some(audit) = &integrity {
        if audit.passed {
            tracing::info!(
                target: "virtualssd_desktop::integrity",
                "Post-sync audit for disk {id}: {}/{} chunk(s) verified locally, {} not present locally",
                audit.verified_chunks,
                audit.total_chunks,
                audit.missing_chunks
            );
        } else {
            tracing::error!(
                target: "virtualssd_desktop::integrity",
                "Post-sync audit for disk {id} found {} checksum error(s): {}",
                audit.errors.len(),
                audit.errors.join("; ")
            );
        }
    }

    Ok(SyncDiskReport { mirror, integrity })
}

#[tauri::command]
async fn delete_disk(
    state: tauri::State<'_, Arc<AppState>>,
    disk_id: String,
    delete_remote_data: bool,
) -> Result<DeleteReport, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let _guard = state.mount_lock.lock().await;

    let engine = {
        let disks = state.disks.read();
        disks
            .get(&id)
            .cloned()
            .ok_or_else(|| "Disk not found".to_string())?
    };
    let telemetry = engine.telemetry();
    if telemetry.status == DiskStatus::Online || telemetry.physical_drive_path.is_some() {
        return Err("Unmount the disk before deleting it.".to_string());
    }

    // Remote data is deleted first. If any object cannot be deleted the local
    // disk is retained so the user can retry instead of losing the only copy
    // of remote object ids.
    let remote = if delete_remote_data {
        Some(
            engine
                .delete_remote_data()
                .await
                .map_err(|e| format!("Remote delete failed; the local disk was kept: {e}"))?,
        )
    } else {
        None
    };

    let vhdx = engine.vhdx_path();
    if let Err(e) = tokio::task::spawn_blocking(move || windows_vhd::detach_vhdx(&vhdx))
        .await
        .map_err(|e| format!("Delete cleanup task failed: {e}"))?
    {
        tracing::warn!("Could not detach VHDX during delete: {e}");
    }

    let disk_dir = state.data_dir.join("disks").join(id.to_string());
    state.disks.write().remove(&id);
    state.disk_provider_map.write().remove(&id);

    // The engine owns an open WAL file; drop every reference before removing
    // the directory or Windows refuses to delete the file.
    drop(engine);
    std::fs::remove_dir_all(&disk_dir).map_err(|e| {
        format!(
            "Failed to delete disk directory {}: {e}",
            disk_dir.display()
        )
    })?;

    match &remote {
        Some(report) => {
            tracing::warn!(
                target: "virtualssd_desktop::storage-core",
                "Deleted disk {id} locally and removed {} remote chunk(s); manifest removed: {}",
                report.deleted_chunks,
                report.deleted_manifest
            );
        }
        None => {
            tracing::warn!(
                target: "virtualssd_desktop::storage-core",
                "Deleted disk {id} locally; remote data was left untouched"
            );
        }
    }

    Ok(DeleteReport {
        remote_data_requested: delete_remote_data,
        remote,
    })
}

#[tauri::command]
fn verify_disk_integrity(
    state: tauri::State<Arc<AppState>>,
    disk_id: String,
) -> Result<IntegrityReport, String> {
    let id = Uuid::parse_str(&disk_id).map_err(|e| e.to_string())?;
    let disks = state.disks.read();
    let engine = disks.get(&id).ok_or_else(|| "Disk not found".to_string())?;
    let audit = engine.verify_integrity();
    let passed = audit.passed();
    let t = engine.telemetry();

    if passed {
        tracing::info!(
            target: "virtualssd_desktop::integrity",
            "BLAKE3 audit for disk {id}: {}/{} chunk(s) verified locally, {} not present locally",
            audit.verified_chunks,
            audit.total_chunks,
            audit.missing_chunks
        );
    } else {
        tracing::error!(
            target: "virtualssd_desktop::integrity",
            "BLAKE3 audit for disk {id} found {} checksum error(s): {}",
            audit.errors.len(),
            audit
                .errors
                .iter()
                .map(|(idx, msg)| format!("chunk {idx}: {msg}"))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    Ok(IntegrityReport {
        disk_id: t.disk_id.to_string(),
        disk_name: t.name,
        verified_chunks: audit.verified_chunks,
        missing_chunks: audit.missing_chunks,
        total_chunks: audit.total_chunks,
        errors: audit
            .errors
            .into_iter()
            .map(|(idx, msg)| format!("chunk {idx}: {msg}"))
            .collect(),
        passed,
        disk_error: t.last_error,
    })
}

// -- Engine Log IPC Commands --------------------------------------------------

/// Returns the real backend log ring buffer, newest first. This is the same
/// data stream emitted live as `engine-log` events.
#[tauri::command]
fn get_engine_logs() -> Vec<EngineLogEntry> {
    engine_log::store().snapshot()
}

/// Clears the backend ring buffer. The clear action itself is recorded, so the
/// UI always shows at least the truthful backend entry that performed it.
#[tauri::command]
fn clear_engine_logs() -> Vec<EngineLogEntry> {
    engine_log::store().clear();
    tracing::info!(
        target: "virtualssd_desktop::diagnostics",
        "Engine log buffer cleared by user request"
    );
    engine_log::store().snapshot()
}

// -- Startup ------------------------------------------------------------------

fn load_persisted_disks(
    base_dir: &Path,
    providers: &HashMap<String, Arc<dyn CloudStorageProvider>>,
    settings: &AppSettings,
) -> (HashMap<Uuid, Arc<VirtualDiskEngine>>, HashMap<Uuid, String>) {
    let mut map = HashMap::new();
    let mut disk_provider_map = HashMap::new();
    let disks_dir = base_dir.join("disks");
    let Ok(entries) = std::fs::read_dir(&disks_dir) else {
        return (map, disk_provider_map);
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        let manifest_path = entry_path.join("manifest.json");
        let manifest = match DiskManifest::load_with_fallback(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Skipping unreadable disk directory {:?}: {e}", entry_path);
                continue;
            }
        };
        let disk_id = manifest.disk_id;
        let provider_account_id = manifest.provider_account_id.clone();

        // Re-bind a restored disk to its restored (DPAPI-protected) provider by
        // the account id recorded in the manifest. Without this, every restart
        // silently left the disk without a sync target.
        let provider = providers.get(&provider_account_id).cloned();
        if provider.is_some() {
            disk_provider_map.insert(disk_id, provider_account_id.clone());
        } else if !provider_account_id.is_empty() {
            tracing::warn!(
                "Disk {disk_id} is bound to provider '{}', which is not available; its remote chunks cannot sync until the provider is restored",
                short_id(&provider_account_id)
            );
        }

        match VirtualDiskEngine::open(
            &entry_path,
            manifest,
            provider,
            settings.cache_limit_bytes(),
        ) {
            Ok(engine) => {
                let vhdx = windows_vhd::vhdx_path_for_disk_dir(&entry_path);
                if vhdx.exists() {
                    if let Err(e) = windows_vhd::detach_vhdx(&vhdx) {
                        tracing::warn!("Could not detach stale VHDX {:?}: {e}", vhdx);
                    }
                }
                engine.set_status(DiskStatus::Unmounted);
                engine.set_drive_letter(None);
                engine.set_physical_drive_path(None);
                let key_warning = if engine.encryption_enabled() && !engine.encryption_key_present()
                {
                    Some(
                        "This disk is encrypted, but its recovery key is not installed. Import the recovery key before mounting or syncing."
                            .to_string(),
                    )
                } else {
                    None
                };
                engine.set_last_error(key_warning);
                map.insert(disk_id, Arc::new(engine));
                tracing::info!("Loaded persisted disk {disk_id} as Unmounted");
            }
            Err(e) => {
                tracing::error!("Failed to reopen disk {disk_id}: {e}");
            }
        }
    }
    (map, disk_provider_map)
}

fn main() {
    // Install the real backend log pipeline before anything else so startup
    // events are captured. The ring-buffer layer feeds the Diagnostics page;
    // the fmt layer keeps a console trace during development.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "info,tauri=warn,tao=warn,wry=warn,hyper=warn,hyper_util=warn,reqwest=warn,h2=warn,tokio=warn,rustls=warn",
        )
    });
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(engine_log::EngineLogLayer)
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .try_init();

    tracing::info!(
        target: "virtualssd_desktop::startup",
        "VirtualSSD desktop engine starting (v{})",
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!(
        target: "virtualssd_desktop::startup",
        "Engine log ring buffer active with {} entry capacity",
        engine_log::ENGINE_LOG_CAPACITY
    );

    let app_data =
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\VirtualSSDData".to_string());
    let base_dir = PathBuf::from(app_data).join("VirtualSSD");
    let _ = std::fs::create_dir_all(&base_dir);
    let _ = std::fs::create_dir_all(base_dir.join("disks"));
    let _ = std::fs::create_dir_all(base_dir.join("providers"));

    let (restored_providers, restored_records) = restore_providers(&base_dir);
    tracing::info!(
        "Restored {} encrypted provider(s) on startup",
        restored_providers.len()
    );

    let settings_snapshot = settings::load(&base_dir);
    if let Some(warning) = &settings_snapshot.warning {
        tracing::warn!("Settings warning: {warning}");
    }

    let (persisted, disk_provider_map) =
        load_persisted_disks(&base_dir, &restored_providers, &settings_snapshot.settings);
    tracing::info!("Loaded {} persisted disk(s) on startup", persisted.len());

    let state = Arc::new(AppState {
        data_dir: base_dir,
        disks: RwLock::new(persisted),
        live_providers: RwLock::new(restored_providers),
        provider_records: RwLock::new(restored_records),
        disk_provider_map: RwLock::new(disk_provider_map),
        settings: RwLock::new(settings_snapshot.settings),
        settings_warning: RwLock::new(settings_snapshot.warning),
        mount_lock: tokio::sync::Mutex::new(()),
        persist_lock: tokio::sync::Mutex::new(()),
    });

    tauri::Builder::default()
        .manage(state)
        .setup(|app| {
            engine_log::store().attach_app(app.handle().clone());
            tracing::info!(
                target: "virtualssd_desktop::startup",
                "Tauri event stream attached; engine logs are streaming live"
            );

            // Install the system tray icon using the same brand artwork as the
            // window and the installers. A failure here is reported, not
            // swallowed, because the tray is a real user-facing feature.
            if let Err(e) = tray::install(app.handle()) {
                tracing::error!(
                    target: "virtualssd_desktop::tray",
                    "The system tray icon could not be created: {e}"
                );
            }

            // Closing the main window hides the app to the tray instead of
            // terminating it, so mounted volumes and the background engine keep
            // running. The tray's "Quit VirtualSSD" entry exits for real.
            if let Some(window) = app.get_webview_window("main") {
                let tray_window = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Err(e) = tray_window.hide() {
                            tracing::warn!(
                                target: "virtualssd_desktop::tray",
                                "Could not hide the main window on close: {e}"
                            );
                            return;
                        }
                        tracing::info!(
                            target: "virtualssd_desktop::tray",
                            "Close request sent the application to the system tray"
                        );
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_google_drive_provider,
            add_telegram_provider,
            list_providers,
            verify_provider,
            remove_provider,
            list_disks,
            get_app_stats,
            get_runtime_status,
            get_settings,
            update_settings,
            restart_elevated,
            import_disk,
            export_disk_key,
            import_disk_key,
            create_disk,
            mount_disk,
            unmount_disk,
            sync_disk,
            sync_disk_live,
            delete_disk,
            verify_disk_integrity,
            get_engine_logs,
            clear_engine_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
