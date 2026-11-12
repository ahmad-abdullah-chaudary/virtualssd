//! Telegram Bot API Cloud Storage Adapter for VirtualSSD.
//!
//! Stores chunk objects within a dedicated Telegram channel or Saved Messages,
//! mapping chunk indices to Telegram message and file identifiers.
//!
//! The Telegram Bot API does not expose account storage quota, so `get_quota`
//! reports an explicit error instead of inventing a number.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;
use virtualssd_integrity::ChunkChecksum;
use virtualssd_manifest::DiskManifest;
use virtualssd_provider_contract::{
    CloudStorageProvider, ProviderError, StorageQuota, UploadResult,
};
use zeroize::Zeroize;

/// Documented Telegram Bot API limit for `sendDocument` uploads.
const TELEGRAM_MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

/// The pinned catalog maps every published disk to its manifest document. A
/// fresh install reads exactly one pinned file and can then discover any disk's
/// manifest, regardless of how many disks share the storage chat.
const MANIFEST_CATALOG_FILE: &str = "virtualssd_manifest_catalog.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestCatalogEntry {
    disk_id: Uuid,
    message_id: i64,
    file_id: String,
}

fn upsert_catalog_entry(entries: &mut Vec<ManifestCatalogEntry>, entry: ManifestCatalogEntry) {
    match entries
        .iter_mut()
        .find(|existing| existing.disk_id == entry.disk_id)
    {
        Some(existing) => *existing = entry,
        None => entries.push(entry),
    }
}

fn is_transient_provider_error(err: &ProviderError) -> bool {
    matches!(
        err,
        ProviderError::Network(_)
            | ProviderError::RateLimited { .. }
            | ProviderError::AuthExpired(_)
            | ProviderError::QuotaExceeded(_)
    )
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// The Bot API transports the token in the URL path; it is never serialized
    /// or printed, and is wiped from memory when the config is dropped.
    #[serde(skip_serializing)]
    pub bot_token: Option<String>,
    pub api_id: Option<i32>,
    #[serde(skip_serializing)]
    pub api_hash: Option<String>,
    #[serde(skip_serializing)]
    pub phone_number: Option<String>,
    pub storage_chat_id: i64,
    pub session_db_path: PathBuf,
}

impl std::fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("bot_token", &self.bot_token.as_ref().map(|_| "<redacted>"))
            .field("api_id", &self.api_id)
            .field("api_hash", &self.api_hash.as_ref().map(|_| "<redacted>"))
            .field(
                "phone_number",
                &self.phone_number.as_ref().map(|_| "<redacted>"),
            )
            .field("storage_chat_id", &self.storage_chat_id)
            .field("session_db_path", &self.session_db_path)
            .finish()
    }
}

impl Drop for TelegramConfig {
    fn drop(&mut self) {
        if let Some(token) = self.bot_token.as_mut() {
            token.zeroize();
        }
        if let Some(hash) = self.api_hash.as_mut() {
            hash.zeroize();
        }
        if let Some(phone) = self.phone_number.as_mut() {
            phone.zeroize();
        }
    }
}

pub struct TelegramProvider {
    config: TelegramConfig,
    client: Client,
    db: Arc<Mutex<rusqlite::Connection>>,
    /// Serializes manifest catalog read-modify-write cycles across disks that
    /// share this provider, so concurrent syncs cannot drop catalog entries.
    manifest_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for TelegramProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramProvider")
            .field("storage_chat_id", &self.config.storage_chat_id)
            .field("session_db_path", &self.config.session_db_path)
            .finish_non_exhaustive()
    }
}

/// Builds the HTTP client used for every Telegram Bot API request.
///
/// `tcp_nodelay` removes the delayed-ACK stall on small requests and the
/// idle pool is kept warm so a chunk transfer does not re-establish a TLS
/// connection per request.
fn build_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .tcp_nodelay(true)
        .pool_idle_timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(16)
        .build()
        .unwrap_or_default()
}

impl TelegramProvider {
    pub fn new(config: TelegramConfig) -> Result<Self, ProviderError> {
        let parent = config.session_db_path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| {
            ProviderError::Internal(format!(
                "Failed to create Telegram session directory {}: {e}",
                parent.display()
            ))
        })?;

        let conn = rusqlite::Connection::open(&config.session_db_path)
            .map_err(|e| ProviderError::Internal(format!("Failed to open Telegram DB: {e}")))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS chunk_mappings (
                disk_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                message_id INTEGER NOT NULL,
                file_id TEXT NOT NULL,
                checksum TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (disk_id, chunk_index)
            )",
            [],
        )
        .map_err(|e| ProviderError::Internal(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS manifest_index (
                disk_id TEXT PRIMARY KEY,
                message_id INTEGER NOT NULL,
                file_id TEXT NOT NULL,
                published_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| ProviderError::Internal(e.to_string()))?;

        Ok(Self {
            config,
            client: build_client(),
            db: Arc::new(Mutex::new(conn)),
            manifest_lock: tokio::sync::Mutex::new(()),
        })
    }

    fn bot_token(&self) -> Result<&str, ProviderError> {
        self.config
            .bot_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::AuthExpired("Telegram bot token not configured".into())
            })
    }

    /// Removes the bot token from any text before it can surface in an error
    /// message or log line.
    fn redact(&self, text: &str) -> String {
        match self.config.bot_token.as_deref() {
            Some(token) if !token.is_empty() => text.replace(token, "<redacted>"),
            _ => text.to_string(),
        }
    }

    fn network_error(&self, context: &str, error: reqwest::Error) -> ProviderError {
        ProviderError::Network(self.redact(&format!("{context}: {error}")))
    }

    /// Maps a Telegram API failure to a typed provider error, extracting the
    /// human-readable description and FloodWait retry delay when available.
    fn map_api_error(
        &self,
        status: reqwest::StatusCode,
        body: &str,
        operation: &str,
    ) -> ProviderError {
        let parsed: Option<Value> = serde_json::from_str(body).ok();
        let description = parsed
            .as_ref()
            .and_then(|value| value.get("description"))
            .and_then(|value| value.as_str());
        let retry_after = parsed
            .as_ref()
            .and_then(|value| value.get("parameters"))
            .and_then(|value| value.get("retry_after"))
            .and_then(|value| value.as_u64());
        let detail = self.redact(
            description
                .map(str::to_owned)
                .unwrap_or_else(|| body.to_string())
                .as_str(),
        );

        match status.as_u16() {
            400 => {
                let lowered = detail.to_ascii_lowercase();
                if lowered.contains("too large") || lowered.contains("too big") {
                    ProviderError::Internal(format!(
                        "Chunk exceeds the Telegram upload limit: {detail}"
                    ))
                } else {
                    ProviderError::Internal(format!("Telegram {operation} failed: {detail}"))
                }
            }
            401 => ProviderError::AuthExpired(format!(
                "Telegram rejected the bot token during {operation}: {detail}"
            )),
            403 => ProviderError::AuthExpired(format!(
                "Telegram denied access to the storage chat during {operation}: {detail}"
            )),
            404 => ProviderError::NotFound(format!(
                "Telegram {operation} target not found: {detail}"
            )),
            429 => ProviderError::RateLimited {
                retry_after_secs: retry_after.unwrap_or(15),
            },
            413 => ProviderError::Internal(format!(
                "Chunk exceeds the Telegram upload limit: {detail}"
            )),
            code if code >= 500 => ProviderError::Network(format!(
                "Telegram server error during {operation}: {detail}"
            )),
            _ => ProviderError::Internal(format!(
                "Telegram {operation} failed ({status}): {detail}"
            )),
        }
    }

    fn parse_api_response(&self, body: &str, operation: &str) -> Result<Value, ProviderError> {
        let parsed: Value = serde_json::from_str(body).map_err(|e| {
            ProviderError::Internal(self.redact(&format!(
                "Telegram returned an unparseable {operation} response: {e}"
            )))
        })?;

        let ok = parsed
            .get("ok")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if !ok {
            let description = parsed
                .get("description")
                .and_then(|value| value.as_str())
                .unwrap_or("response did not contain ok: true");
            return Err(ProviderError::Internal(self.redact(&format!(
                "Telegram {operation} was rejected: {description}"
            ))));
        }

        Ok(parsed)
    }

    fn manifest_file_name(disk_id: Uuid) -> String {
        format!("manifest_{disk_id}.json")
    }

    async fn cached_manifest(&self, disk_id: Uuid) -> Option<(i64, String)> {
        let db = self.db.lock().await;
        db.query_row(
            "SELECT message_id, file_id FROM manifest_index WHERE disk_id = ?1",
            rusqlite::params![disk_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
    }

    async fn record_manifest(
        &self,
        disk_id: Uuid,
        message_id: i64,
        file_id: &str,
    ) -> Result<(), ProviderError> {
        let db = self.db.lock().await;
        db.execute(
            "INSERT OR REPLACE INTO manifest_index VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                disk_id.to_string(),
                message_id,
                file_id,
                chrono::Utc::now().timestamp()
            ],
        )
        .map_err(|e| {
            ProviderError::Internal(format!("Failed to record Telegram manifest: {e}"))
        })?;
        Ok(())
    }

    async fn clear_manifest_index(&self, disk_id: Uuid) -> Result<(), ProviderError> {
        let db = self.db.lock().await;
        db.execute(
            "DELETE FROM manifest_index WHERE disk_id = ?1",
            rusqlite::params![disk_id.to_string()],
        )
        .map_err(|e| {
            ProviderError::Internal(format!("Failed to clear Telegram manifest index: {e}"))
        })?;
        Ok(())
    }

    async fn local_manifest_entries(&self) -> Vec<ManifestCatalogEntry> {
        let db = self.db.lock().await;
        let mut statement = match db.prepare(
            "SELECT disk_id, message_id, file_id FROM manifest_index ORDER BY disk_id",
        ) {
            Ok(statement) => statement,
            Err(e) => {
                tracing::warn!("Could not read Telegram manifest index: {e}");
                return Vec::new();
            }
        };

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        });

        let mut entries = Vec::new();
        match rows {
            Ok(rows) => {
                for row in rows.flatten() {
                    match Uuid::parse_str(&row.0) {
                        Ok(disk_id) => entries.push(ManifestCatalogEntry {
                            disk_id,
                            message_id: row.1,
                            file_id: row.2,
                        }),
                        Err(e) => tracing::warn!(
                            "Ignoring Telegram manifest index row with invalid disk id '{}': {e}",
                            row.0
                        ),
                    }
                }
            }
            Err(e) => tracing::warn!("Could not enumerate Telegram manifest index: {e}"),
        }
        entries
    }

    /// Downloads a Telegram document by file id.
    async fn download_file_bytes(
        &self,
        token: &str,
        file_id: &str,
        operation: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        let mut url = reqwest::Url::parse(&format!("{TELEGRAM_API_BASE}/bot{token}/getFile"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        url.query_pairs_mut().append_pair("file_id", file_id);

        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram getFile failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, operation));
        }

        let parsed = self.parse_api_response(&body, operation)?;
        let file_path = parsed
            .get("result")
            .and_then(|value| value.get("file_path"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::NotFound(format!(
                    "Telegram {operation} document has no downloadable file path"
                ))
            })?;

        let download_res = self
            .client
            .get(format!("{TELEGRAM_API_BASE}/file/bot{token}/{file_path}"))
            .send()
            .await
            .map_err(|e| self.network_error("Telegram file download failed", e))?;

        let download_status = download_res.status();
        if !download_status.is_success() {
            let download_body = download_res.text().await.unwrap_or_default();
            return Err(self.map_api_error(download_status, &download_body, operation));
        }

        let bytes = download_res
            .bytes()
            .await
            .map_err(|e| self.network_error("Telegram file download failed", e))?;
        Ok(bytes.to_vec())
    }

    /// Returns the raw bytes of the chat's pinned document when its file name
    /// matches `expected_name`. A different pinned document means the chat has
    /// no catalog yet (legacy per-disk manifest), which is not an error.
    async fn download_pinned_document(
        &self,
        token: &str,
        expected_name: &str,
    ) -> Result<Option<Vec<u8>>, ProviderError> {
        let mut url = reqwest::Url::parse(&format!("{TELEGRAM_API_BASE}/bot{token}/getChat"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("chat_id", &self.config.storage_chat_id.to_string());

        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram getChat failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "getChat"));
        }

        let parsed = self.parse_api_response(&body, "getChat")?;
        let Some(pinned) = parsed
            .get("result")
            .and_then(|value| value.get("pinned_message"))
        else {
            return Ok(None);
        };
        let Some(document) = pinned.get("document") else {
            return Ok(None);
        };

        let file_name = document
            .get("file_name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if file_name != expected_name {
            return Ok(None);
        }

        let file_id = document
            .get("file_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::Internal(format!(
                    "Pinned Telegram document '{expected_name}' has no file_id"
                ))
            })?;

        let bytes = self
            .download_file_bytes(token, file_id, "download pinned document")
            .await?;
        Ok(Some(bytes))
    }

    async fn load_catalog(
        &self,
        token: &str,
    ) -> Result<Vec<ManifestCatalogEntry>, ProviderError> {
        let Some(bytes) = self
            .download_pinned_document(token, MANIFEST_CATALOG_FILE)
            .await?
        else {
            return Ok(Vec::new());
        };

        serde_json::from_slice(&bytes).map_err(|e| {
            ProviderError::Internal(format!(
                "Pinned Telegram manifest catalog is not valid JSON: {e}"
            ))
        })
    }

    /// Uploads and pins the catalog, returning the catalog message id.
    async fn upload_catalog(
        &self,
        token: &str,
        entries: &[ManifestCatalogEntry],
    ) -> Result<i64, ProviderError> {
        let serialized = serde_json::to_vec(entries).map_err(|e| {
            ProviderError::Internal(format!("Failed to serialize manifest catalog: {e}"))
        })?;

        let part = reqwest::multipart::Part::bytes(serialized)
            .file_name(MANIFEST_CATALOG_FILE)
            .mime_str("application/json")
            .map_err(|e| ProviderError::Internal(e.to_string()))?;

        let form = reqwest::multipart::Form::new()
            .text("chat_id", self.config.storage_chat_id.to_string())
            .text("caption", "VirtualSSD manifest catalog".to_string())
            .part("document", part);

        let res = self
            .client
            .post(format!("{TELEGRAM_API_BASE}/bot{token}/sendDocument"))
            .multipart(form)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram catalog upload failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "publish_manifest_catalog"));
        }

        let parsed = self.parse_api_response(&body, "publish_manifest_catalog")?;
        let message_id = parsed
            .get("result")
            .and_then(|value| value.get("message_id"))
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Telegram manifest catalog upload returned no message_id".into(),
                )
            })?;

        self.pin_manifest_message(token, message_id).await?;
        Ok(message_id)
    }

    /// Downloads and validates a manifest addressed by a Telegram file id.
    async fn download_manifest(
        &self,
        token: &str,
        file_id: &str,
        disk_id: Uuid,
    ) -> Result<DiskManifest, ProviderError> {
        let bytes = self
            .download_file_bytes(token, file_id, "fetch_manifest")
            .await?;

        let manifest: DiskManifest = serde_json::from_slice(&bytes).map_err(|e| {
            ProviderError::Internal(format!("Telegram manifest is not valid JSON: {e}"))
        })?;

        if manifest.disk_id != disk_id {
            return Err(ProviderError::Internal(format!(
                "Telegram manifest id mismatch: expected {disk_id}, found {}",
                manifest.disk_id
            )));
        }

        Ok(manifest)
    }

    /// Discovers a previously published manifest after a fresh install (or a
    /// lost local database) by reading the chat's pinned message. `publish_manifest`
    /// pins every manifest it uploads, so this is authoritative without a sync.
    async fn discover_pinned_manifest(
        &self,
        token: &str,
        disk_id: Uuid,
    ) -> Result<(i64, String), ProviderError> {
        let mut url = reqwest::Url::parse(&format!("{TELEGRAM_API_BASE}/bot{token}/getChat"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("chat_id", &self.config.storage_chat_id.to_string());

        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram getChat failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "getChat"));
        }

        let parsed = self.parse_api_response(&body, "getChat")?;
        let pinned = parsed
            .get("result")
            .and_then(|value| value.get("pinned_message"))
            .ok_or_else(|| {
                ProviderError::NotFound(format!(
                    "Telegram storage chat has no pinned manifest and no local manifest record for disk {disk_id}"
                ))
            })?;

        let document = pinned.get("document").ok_or_else(|| {
            ProviderError::NotFound(format!(
                "Pinned Telegram message is not a manifest document for disk {disk_id}"
            ))
        })?;

        let expected_name = Self::manifest_file_name(disk_id);
        let actual_name = document
            .get("file_name")
            .and_then(|value| value.as_str())
            .unwrap_or("<unnamed>");
        if actual_name != expected_name {
            return Err(ProviderError::NotFound(format!(
                "Pinned Telegram document is '{actual_name}', not the manifest '{expected_name}' for disk {disk_id}"
            )));
        }

        let message_id = pinned
            .get("message_id")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Pinned Telegram manifest message has no message_id".into(),
                )
            })?;

        let file_id = document
            .get("file_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::Internal("Pinned Telegram manifest has no document file_id".into())
            })?
            .to_string();

        Ok((message_id, file_id))
    }

    /// Best-effort removal of the previous manifest message so the chat keeps a
    /// single authoritative manifest and the pinned message stays current.
    /// Failures are logged; a stale message never causes a fabricated success.
    /// Returns the pinned document's file name and message id, if the chat has
    /// a pinned document. Used to clean up legacy per-disk manifest pins.
    async fn pinned_document(
        &self,
        token: &str,
    ) -> Result<Option<(String, i64)>, ProviderError> {
        let mut url = reqwest::Url::parse(&format!("{TELEGRAM_API_BASE}/bot{token}/getChat"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("chat_id", &self.config.storage_chat_id.to_string());

        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram getChat failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "getChat"));
        }

        let parsed = self.parse_api_response(&body, "getChat")?;
        let Some(pinned) = parsed
            .get("result")
            .and_then(|value| value.get("pinned_message"))
        else {
            return Ok(None);
        };
        let Some(document) = pinned.get("document") else {
            return Ok(None);
        };

        let file_name = document
            .get("file_name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Pinned Telegram document has no file_name".into(),
                )
            })?
            .to_string();
        let message_id = pinned
            .get("message_id")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Pinned Telegram document has no message_id".into(),
                )
            })?;

        Ok(Some((file_name, message_id)))
    }

    /// Deletes one Telegram message. A message that is already gone is treated
    /// as success because remote deletion must be idempotent.
    async fn delete_message(
        &self,
        token: &str,
        message_id: i64,
        operation: &str,
    ) -> Result<(), ProviderError> {
        let res = self
            .client
            .post(format!("{TELEGRAM_API_BASE}/bot{token}/deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": self.config.storage_chat_id,
                "message_id": message_id
            }))
            .send()
            .await
            .map_err(|e| self.network_error("Telegram deleteMessage failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if status.is_success() || body.to_ascii_lowercase().contains("not found") {
            return Ok(());
        }

        Err(self.map_api_error(status, &body, operation))
    }

    async fn delete_previous_manifest(&self, token: &str, message_id: i64) {
        let res = self
            .client
            .post(format!("{TELEGRAM_API_BASE}/bot{token}/deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": self.config.storage_chat_id,
                "message_id": message_id
            }))
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let already_gone = body.to_ascii_lowercase().contains("not found");
                if !status.is_success() && !already_gone {
                    tracing::warn!(
                        "Could not delete previous Telegram manifest message {message_id}: {}",
                        self.redact(&body)
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Could not delete previous Telegram manifest message {message_id}: {}",
                    self.redact(&e.to_string())
                );
            }
        }
    }

    async fn pin_manifest_message(
        &self,
        token: &str,
        message_id: i64,
    ) -> Result<(), ProviderError> {
        let res = self
            .client
            .post(format!("{TELEGRAM_API_BASE}/bot{token}/pinChatMessage"))
            .json(&serde_json::json!({
                "chat_id": self.config.storage_chat_id,
                "message_id": message_id,
                "disable_notification": true
            }))
            .send()
            .await
            .map_err(|e| self.network_error("Telegram pinChatMessage failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "pinChatMessage"));
        }

        self.parse_api_response(&body, "pinChatMessage")?;
        Ok(())
    }
}

#[async_trait]
impl CloudStorageProvider for TelegramProvider {
    fn provider_name(&self) -> &'static str {
        "Telegram"
    }

    async fn verify_connection(&self) -> Result<(), ProviderError> {
        let token = self.bot_token()?;
        let res = self
            .client
            .get(format!("{TELEGRAM_API_BASE}/bot{token}/getMe"))
            .send()
            .await
            .map_err(|e| self.network_error("Telegram getMe failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "getMe"));
        }

        // A 200 is only trusted when the body actually confirms the bot.
        let parsed = self.parse_api_response(&body, "getMe")?;
        let is_bot = parsed
            .get("result")
            .and_then(|value| value.get("is_bot"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if is_bot {
            Ok(())
        } else {
            Err(ProviderError::AuthExpired(
                "Telegram getMe did not identify a bot account".into(),
            ))
        }
    }

    async fn get_quota(&self) -> Result<StorageQuota, ProviderError> {
        // The Bot API has no endpoint for account storage quota. Do not invent
        // a value; report that the quota is unavailable.
        self.bot_token()?;
        Err(ProviderError::Internal(
            "Telegram Bot API does not expose account storage quota".into(),
        ))
    }

    async fn upload_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        data: &[u8],
        checksum: &ChunkChecksum,
    ) -> Result<UploadResult, ProviderError> {
        let token = self.bot_token()?;

        if data.is_empty() {
            return Err(ProviderError::Internal(
                "Refusing to upload an empty chunk".into(),
            ));
        }
        if data.len() > TELEGRAM_MAX_UPLOAD_BYTES {
            return Err(ProviderError::Internal(format!(
                "Chunk of {} bytes exceeds the Telegram upload limit of {} bytes",
                data.len(),
                TELEGRAM_MAX_UPLOAD_BYTES
            )));
        }

        // Re-uploading an identical chunk (e.g. after a crash between the
        // Telegram upload and the manifest commit) would duplicate the object.
        // The local mapping is authoritative by checksum, so reuse it.
        let mut replaced_message_id: Option<i64> = None;
        {
            let db = self.db.lock().await;
            let existing: Option<(String, String, u64, i64)> = db
                .query_row(
                    "SELECT file_id, checksum, size_bytes, message_id FROM chunk_mappings \
                     WHERE disk_id = ?1 AND chunk_index = ?2",
                    rusqlite::params![disk_id.to_string(), chunk_index],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .ok();

            if let Some((existing_file_id, existing_checksum, existing_size, existing_message_id)) =
                existing
            {
                if existing_checksum == checksum.hex_digest
                    && existing_size == data.len() as u64
                {
                    tracing::info!(
                        "Telegram chunk {chunk_index} for disk {disk_id} is already stored with the same checksum; skipping upload"
                    );
                    return Ok(UploadResult {
                        remote_object_id: existing_file_id,
                        remote_version: 1,
                        bytes_transferred: 0,
                    });
                }
                replaced_message_id = Some(existing_message_id);
            }
        }

        let filename = format!("chunk_{chunk_index}_{}.bin", checksum.hex_digest);
        let part = reqwest::multipart::Part::bytes(data.to_vec()).file_name(filename);
        let form = reqwest::multipart::Form::new()
            .text("chat_id", self.config.storage_chat_id.to_string())
            .text("caption", format!("VirtualSSD {disk_id} #{chunk_index}"))
            .part("document", part);

        let res = self
            .client
            .post(format!("{TELEGRAM_API_BASE}/bot{token}/sendDocument"))
            .multipart(form)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram sendDocument failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "sendDocument"));
        }

        let parsed = self.parse_api_response(&body, "sendDocument")?;
        let message = parsed.get("result").ok_or_else(|| {
            ProviderError::Internal("Telegram sendDocument returned no message".into())
        })?;

        let message_id = message
            .get("message_id")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                ProviderError::Internal("Telegram sendDocument returned no message_id".into())
            })?;

        // Never substitute a fabricated id: without a real file_id the object
        // cannot be downloaded later.
        let file_id = message
            .get("document")
            .and_then(|value| value.get("file_id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Telegram sendDocument returned no document file_id".into(),
                )
            })?
            .to_string();

        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp();
        db.execute(
            "INSERT OR REPLACE INTO chunk_mappings VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                disk_id.to_string(),
                chunk_index,
                message_id,
                file_id,
                checksum.hex_digest,
                data.len() as u64,
                now
            ],
        )
        .map_err(|e| {
            ProviderError::Internal(format!("Failed to persist Telegram chunk mapping: {e}"))
        })?;

        // The new document is stored and mapped. Remove the document it
        // replaced so a re-upload never leaks the previous version. Failure is
        // logged but does not invalidate the successful upload.
        if let Some(old_message_id) = replaced_message_id {
            if old_message_id != message_id {
                let token = self.bot_token()?;
                if let Err(e) = self
                    .delete_message(&token, old_message_id, "replace_chunk")
                    .await
                {
                    tracing::warn!(
                        "Could not delete superseded Telegram chunk message {old_message_id}: {}",
                        self.redact(&e.to_string())
                    );
                }
            }
        }

        Ok(UploadResult {
            remote_object_id: file_id,
            remote_version: 1,
            bytes_transferred: data.len(),
        })
    }

    async fn download_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        remote_object_id: &str,
        expected_checksum: &ChunkChecksum,
    ) -> Result<Vec<u8>, ProviderError> {
        let token = self.bot_token()?;

        let mut url = reqwest::Url::parse(&format!("{TELEGRAM_API_BASE}/bot{token}/getFile"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("file_id", remote_object_id);

        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram getFile failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "getFile"));
        }

        let parsed = self.parse_api_response(&body, "getFile")?;
        let file_path = parsed
            .get("result")
            .and_then(|value| value.get("file_path"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::NotFound(format!(
                    "Chunk #{chunk_index} for disk {disk_id} has no downloadable file path on Telegram"
                ))
            })?;

        let download_res = self
            .client
            .get(format!("{TELEGRAM_API_BASE}/file/bot{token}/{file_path}"))
            .send()
            .await
            .map_err(|e| self.network_error("Telegram file download failed", e))?;

        let download_status = download_res.status();
        if !download_status.is_success() {
            let download_body = download_res.text().await.unwrap_or_default();
            return Err(
                self.map_api_error(download_status, &download_body, "file download")
            );
        }

        let bytes = download_res
            .bytes()
            .await
            .map_err(|e| self.network_error("Telegram file download failed", e))?;

        expected_checksum
            .verify(&bytes)
            .map_err(|e| ProviderError::IntegrityFailure(e.to_string()))?;

        Ok(bytes.to_vec())
    }

    async fn delete_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        _remote_object_id: &str,
    ) -> Result<(), ProviderError> {
        let msg_id: Option<i64> = {
            let db = self.db.lock().await;
            db.query_row(
                "SELECT message_id FROM chunk_mappings WHERE disk_id = ?1 AND chunk_index = ?2",
                rusqlite::params![disk_id.to_string(), chunk_index],
                |row| row.get(0),
            )
            .ok()
        };

        // No known remote message means there is nothing to delete remotely.
        let Some(mid) = msg_id else {
            let db = self.db.lock().await;
            db.execute(
                "DELETE FROM chunk_mappings WHERE disk_id = ?1 AND chunk_index = ?2",
                rusqlite::params![disk_id.to_string(), chunk_index],
            )
            .map_err(|e| {
                ProviderError::Internal(format!("Failed to clear Telegram chunk mapping: {e}"))
            })?;
            return Ok(());
        };

        let token = self.bot_token()?;
        self.delete_message(&token, mid, "deleteMessage").await?;

        // Only drop the local mapping once the remote delete has succeeded (or
        // the message was already gone), so a failed delete can be retried.
        let db = self.db.lock().await;
        db.execute(
            "DELETE FROM chunk_mappings WHERE disk_id = ?1 AND chunk_index = ?2",
            rusqlite::params![disk_id.to_string(), chunk_index],
        )
        .map_err(|e| {
            ProviderError::Internal(format!("Failed to clear Telegram chunk mapping: {e}"))
        })?;

        Ok(())
    }

    async fn delete_superseded_chunk(
        &self,
        _disk_id: Uuid,
        _chunk_index: u64,
        _remote_object_id: &str,
    ) -> Result<(), ProviderError> {
        // Telegram chunk uploads replace the chat message in place and the
        // adapter removes the superseded message as part of the upload.
        Ok(())
    }

    async fn publish_manifest(&self, manifest: &DiskManifest) -> Result<(), ProviderError> {
        let token = self.bot_token()?;

        if manifest.disk_id.is_nil() {
            return Err(ProviderError::Internal(
                "Refusing to publish a manifest with a nil disk id".into(),
            ));
        }

        let _manifest_guard = self.manifest_lock.lock().await;

        let serialized = serde_json::to_vec(manifest)
            .map_err(|e| ProviderError::Internal(format!("Failed to serialize manifest: {e}")))?;

        // Remember the existing manifest message so it can be replaced after
        // the new one is safely uploaded and pinned.
        let previous = self.cached_manifest(manifest.disk_id).await;

        let filename = Self::manifest_file_name(manifest.disk_id);
        let part = reqwest::multipart::Part::bytes(serialized)
            .file_name(filename)
            .mime_str("application/json")
            .map_err(|e| ProviderError::Internal(e.to_string()))?;

        let form = reqwest::multipart::Form::new()
            .text("chat_id", self.config.storage_chat_id.to_string())
            .text(
                "caption",
                format!("VirtualSSD manifest {}", manifest.disk_id),
            )
            .part("document", part);

        let res = self
            .client
            .post(format!("{TELEGRAM_API_BASE}/bot{token}/sendDocument"))
            .multipart(form)
            .send()
            .await
            .map_err(|e| self.network_error("Telegram manifest upload failed", e))?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(self.map_api_error(status, &body, "publish_manifest"));
        }

        let parsed = self.parse_api_response(&body, "publish_manifest")?;
        let message = parsed.get("result").ok_or_else(|| {
            ProviderError::Internal("Telegram manifest upload returned no message".into())
        })?;

        let message_id = message
            .get("message_id")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Telegram manifest upload returned no message_id".into(),
                )
            })?;

        let file_id = message
            .get("document")
            .and_then(|value| value.get("file_id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Telegram manifest upload returned no document file_id".into(),
                )
            })?
            .to_string();

        if let Err(err) = self
            .record_manifest(manifest.disk_id, message_id, &file_id)
            .await
        {
            // Without an index entry the upload would be orphaned behind a
            // retry; remove it best-effort and surface the database failure.
            let _ = self
                .client
                .post(format!("{TELEGRAM_API_BASE}/bot{token}/deleteMessage"))
                .json(&serde_json::json!({
                    "chat_id": self.config.storage_chat_id,
                    "message_id": message_id
                }))
                .send()
                .await;
            return Err(err);
        }

        // Merge this manifest into the pinned catalog. The catalog is the
        // remote source of truth that lets a fresh install discover every disk,
        // not just the most recently pinned document.
        let mut entries = match self.load_catalog(&token).await {
            Ok(entries) => entries,
            Err(err) => {
                tracing::warn!(
                    "Could not read the pinned Telegram manifest catalog ({err}); rebuilding it from local state"
                );
                Vec::new()
            }
        };
        for local in self.local_manifest_entries().await {
            upsert_catalog_entry(&mut entries, local);
        }
        upsert_catalog_entry(
            &mut entries,
            ManifestCatalogEntry {
                disk_id: manifest.disk_id,
                message_id,
                file_id: file_id.clone(),
            },
        );

        // A manifest that is not referenced by the pinned catalog cannot be
        // discovered by a fresh installation. Report that as a failure so the
        // caller can retry instead of claiming a fully successful publication.
        self.upload_catalog(&token, &entries).await.map_err(|err| {
            ProviderError::Internal(format!(
                "Manifest message {message_id} was uploaded, but the pinned Telegram \
                 catalog could not be updated; a fresh installation may not discover \
                 this disk. Retry the sync. Catalog error: {err}"
            ))
        })?;

        // The catalog now references the new manifest document, so the
        // previous manifest message for this disk can be removed.
        if let Some((previous_message_id, _)) = previous {
            if previous_message_id != message_id {
                self.delete_previous_manifest(&token, previous_message_id)
                    .await;
            }
        }

        Ok(())
    }

    async fn fetch_manifest(&self, disk_id: Uuid) -> Result<DiskManifest, ProviderError> {
        let token = self.bot_token()?;
        let _manifest_guard = self.manifest_lock.lock().await;

        // Fast path: a locally recorded manifest from this installation.
        if let Some((message_id, file_id)) = self.cached_manifest(disk_id).await {
            match self.download_manifest(&token, &file_id, disk_id).await {
                Ok(manifest) => return Ok(manifest),
                Err(err) if is_transient_provider_error(&err) => return Err(err),
                Err(err) => {
                    // The cached file was deleted or belongs to an old chat.
                    // Drop only stale entries; transient failures keep the
                    // index intact.
                    tracing::warn!(
                        "Cached Telegram manifest message {message_id} for disk {disk_id} is unusable ({err}); rediscovering from the pinned catalog"
                    );
                    self.clear_manifest_index(disk_id).await?;
                }
            }
        }

        // Fresh-install path: the pinned catalog maps disk ids to manifest
        // documents, so no local state is required.
        let catalog = self.load_catalog(&token).await?;
        if let Some(entry) = catalog.iter().find(|entry| entry.disk_id == disk_id) {
            match self.download_manifest(&token, &entry.file_id, disk_id).await {
                Ok(manifest) => {
                    self.record_manifest(disk_id, entry.message_id, &entry.file_id)
                        .await?;
                    return Ok(manifest);
                }
                Err(err) if is_transient_provider_error(&err) => return Err(err),
                Err(err) => {
                    tracing::warn!(
                        "Pinned Telegram catalog entry for disk {disk_id} is unusable ({err}); falling back to the pinned manifest document"
                    );
                }
            }
        }

        // Legacy path: installations that pinned a per-disk manifest before
        // the catalog existed.
        let (message_id, file_id) = self.discover_pinned_manifest(&token, disk_id).await?;
        self.record_manifest(disk_id, message_id, &file_id).await?;
        self.download_manifest(&token, &file_id, disk_id).await
    }

    async fn delete_manifest(&self, disk_id: Uuid) -> Result<(), ProviderError> {
        let token = self.bot_token()?;
        let _manifest_guard = self.manifest_lock.lock().await;

        // Read the catalog first. If the catalog itself cannot be read, abort
        // before deleting anything so the remote state stays consistent.
        let mut entries = self.load_catalog(&token).await?;
        let catalog_contained_disk = entries.iter().any(|entry| entry.disk_id == disk_id);

        let mut message_ids: Vec<i64> = entries
            .iter()
            .filter(|entry| entry.disk_id == disk_id)
            .map(|entry| entry.message_id)
            .collect();
        entries.retain(|entry| entry.disk_id != disk_id);

        if let Some((message_id, _)) = self.cached_manifest(disk_id).await {
            message_ids.push(message_id);
        }

        // Legacy installations pinned the per-disk manifest document directly.
        if let Some((name, message_id)) = self.pinned_document(&token).await? {
            if name == Self::manifest_file_name(disk_id) {
                message_ids.push(message_id);
            }
        }

        message_ids.sort_unstable();
        message_ids.dedup();

        for message_id in message_ids {
            self.delete_message(&token, message_id, "delete_manifest")
                .await?;
        }

        self.clear_manifest_index(disk_id).await?;

        // Keep the pinned catalog truthful. Only republish it when it existed,
        // so deleting a legacy disk does not create a catalog unnecessarily.
        if catalog_contained_disk {
            self.upload_catalog(&token, &entries).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config(token: &str, db_path: PathBuf) -> TelegramConfig {
        TelegramConfig {
            bot_token: Some(token.to_string()),
            api_id: Some(12345),
            api_hash: Some("HASH-SECRET".to_string()),
            phone_number: Some("+10000000000".to_string()),
            storage_chat_id: -1001234567890,
            session_db_path: db_path,
        }
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let config = sample_config("123456:SUPER-SECRET", PathBuf::from("session.db"));
        let debug = format!("{config:?}");

        assert!(!debug.contains("SUPER-SECRET"));
        assert!(!debug.contains("HASH-SECRET"));
        assert!(!debug.contains("+10000000000"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn serialized_config_omits_secrets() {
        let config = sample_config("123456:SUPER-SECRET", PathBuf::from("session.db"));
        let json = serde_json::to_string(&config).unwrap();

        assert!(!json.contains("SUPER-SECRET"));
        assert!(!json.contains("bot_token"));
        assert!(!json.contains("HASH-SECRET"));
    }

    #[test]
    fn provider_redacts_token_from_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let token = "123456:SUPER-SECRET";
        let provider = TelegramProvider::new(sample_config(
            token,
            dir.path().join("telegram.db"),
        ))
        .unwrap();

        let message = format!(
            "request to https://api.telegram.org/bot{token}/getMe failed"
        );
        let redacted = provider.redact(&message);

        assert!(!redacted.contains(token));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn catalog_upsert_replaces_existing_disk_entry() {
        let disk_id = Uuid::new_v4();
        let mut entries = vec![ManifestCatalogEntry {
            disk_id,
            message_id: 1,
            file_id: "old".to_string(),
        }];

        upsert_catalog_entry(
            &mut entries,
            ManifestCatalogEntry {
                disk_id,
                message_id: 2,
                file_id: "new".to_string(),
            },
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_id, 2);
        assert_eq!(entries[0].file_id, "new");
    }

    #[test]
    fn catalog_upsert_appends_new_disk() {
        let mut entries = Vec::new();
        upsert_catalog_entry(&mut entries, ManifestCatalogEntry {
            disk_id: Uuid::new_v4(),
            message_id: 1,
            file_id: "a".to_string(),
        });
        upsert_catalog_entry(&mut entries, ManifestCatalogEntry {
            disk_id: Uuid::new_v4(),
            message_id: 2,
            file_id: "b".to_string(),
        });

        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn transient_errors_are_not_treated_as_stale_cache() {
        assert!(is_transient_provider_error(&ProviderError::Network(
            "offline".into()
        )));
        assert!(is_transient_provider_error(&ProviderError::RateLimited {
            retry_after_secs: 5
        }));
        assert!(is_transient_provider_error(&ProviderError::AuthExpired(
            "expired".into()
        )));
        assert!(!is_transient_provider_error(&ProviderError::NotFound(
            "gone".into()
        )));
        assert!(!is_transient_provider_error(&ProviderError::Internal(
            "bad".into()
        )));
    }
}
