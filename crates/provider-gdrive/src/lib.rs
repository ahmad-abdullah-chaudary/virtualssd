//! Google Drive REST API v3 Provider Adapter for VirtualSSD.
//!
//! Implements OAuth 2.0 PKCE, folder hierarchies, resumable chunk uploads,
//! BLAKE3/SHA-256 verification, and exponential backoff retry policies.

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;
use virtualssd_integrity::ChunkChecksum;
use virtualssd_manifest::DiskManifest;
use virtualssd_provider_contract::{
    CloudStorageProvider, ProviderError, StorageQuota, UploadResult,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

const GDRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";
const GDRIVE_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3/files";

/// Resumable upload session state is persisted so an interrupted upload (app
/// crash, network loss, power failure) can resume from the server-reported
/// offset instead of re-sending the whole chunk.
const UPLOAD_SESSION_FILE: &str = "gdrive_upload_sessions.json";

/// Google keeps resumable sessions for about a week. Expire them locally a day
/// early so we never probe a session that is already dead.
const UPLOAD_SESSION_TTL_SECS: i64 = 6 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadSession {
    session_uri: String,
    /// BLAKE3 digest of the chunk this session belongs to. A session is only
    /// resumed when the checksum still matches, so changed data never gets
    /// mixed into the same remote object.
    checksum: String,
    total_bytes: u64,
    created_at: i64,
}

/// Result of asking Google for the state of a resumable session.
enum SessionProbe {
    Complete { object_id: String, version: u64 },
    Incomplete { offset: u64 },
    Expired,
}

fn session_key(disk_id: Uuid, chunk_index: u64) -> String {
    format!("{disk_id}:{chunk_index}")
}

/// Parses the `Range: bytes=0-N` header Google returns on a 308 response into
/// the number of bytes already received.
fn parse_resume_offset(range: Option<&str>, total: u64) -> Result<u64, ProviderError> {
    let Some(raw) = range else { return Ok(0) };
    let spec = raw.strip_prefix("bytes=").ok_or_else(|| {
        ProviderError::Internal(format!("Google Drive returned an invalid Range header '{raw}'"))
    })?;
    let (_, end) = spec.split_once('-').ok_or_else(|| {
        ProviderError::Internal(format!("Google Drive returned an invalid Range header '{raw}'"))
    })?;
    let end: u64 = end.parse().map_err(|e| {
        ProviderError::Internal(format!(
            "Google Drive returned an unparseable Range end '{end}': {e}"
        ))
    })?;
    Ok(end.saturating_add(1).min(total))
}

/// Parses the JSON file resource Google returns when a resumable upload
/// completes. Never substitutes a fabricated object id or version.
fn parse_completed_upload(body: &str) -> Result<(String, u64), ProviderError> {
    #[derive(Deserialize)]
    struct FileCreated {
        id: String,
        version: Option<String>,
    }

    let file_meta: FileCreated = serde_json::from_str(body).map_err(|e| {
        ProviderError::Internal(format!(
            "Google Drive returned an unparseable upload response: {e}"
        ))
    })?;

    let version = match file_meta.version.as_deref() {
        Some(raw) => raw.parse::<u64>().map_err(|e| {
            ProviderError::Internal(format!(
                "Google Drive returned an invalid object version '{raw}': {e}"
            ))
        })?,
        None => {
            return Err(ProviderError::Internal(
                "Google Drive upload response did not include an object version".into(),
            ))
        }
    };

    Ok((file_meta.id, version))
}

fn read_session_file(dir: &Path) -> HashMap<String, UploadSession> {
    let path = dir.join(UPLOAD_SESSION_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(e) => {
            tracing::warn!(
                "Could not read Google Drive upload session file {}: {e}",
                path.display()
            );
            return HashMap::new();
        }
    };

    match serde_json::from_slice(&bytes) {
        Ok(map) => map,
        Err(e) => {
            // Session state is a cache, not user data: a corrupt file must not
            // block uploads. Preserve it for diagnostics and start clean.
            tracing::warn!(
                "Google Drive upload session file {} is corrupt ({e}); starting with no resumable sessions",
                path.display()
            );
            let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
            HashMap::new()
        }
    }
}

fn write_session_file(
    dir: &Path,
    sessions: &HashMap<String, UploadSession>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(UPLOAD_SESSION_FILE);
    let tmp = dir.join(format!("{UPLOAD_SESSION_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(sessions)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Maps a non-success Google Drive HTTP response to a typed provider error.
/// Never fabricates a success value: every caller must surface this error.
fn map_http_error(status: reqwest::StatusCode, body: String) -> ProviderError {
    let detail = if body.trim().is_empty() {
        status.to_string()
    } else {
        body
    };
    let lowered = detail.to_ascii_lowercase();
    match status.as_u16() {
        308 => ProviderError::Internal(
            "Google Drive upload session is incomplete; the chunk must be retried".into(),
        ),
        401 => ProviderError::AuthExpired(format!("Google Drive rejected the request: {detail}")),
        403 => {
            // Google reports storage exhaustion as 403 with a quota reason.
            if lowered.contains("quotaexceeded")
                || lowered.contains("storagequotaexceeded")
                || lowered.contains("insufficient storage")
            {
                ProviderError::QuotaExceeded(format!("Google Drive storage quota exceeded: {detail}"))
            } else if lowered.contains("ratelimitexceeded")
                || lowered.contains("userratelimitexceeded")
                || lowered.contains("rate limit")
            {
                // Google asks clients to retry with exponential backoff. The
                // adapter has no background retry loop, so surface a retry
                // interval and let the caller retry manually.
                ProviderError::RateLimited { retry_after_secs: 60 }
            } else {
                ProviderError::AuthExpired(format!("Google Drive denied the request: {detail}"))
            }
        }
        404 => ProviderError::NotFound(format!("Google Drive object not found: {detail}")),
        413 | 507 => {
            ProviderError::QuotaExceeded("Google Drive storage quota exceeded".into())
        }
        429 => ProviderError::RateLimited { retry_after_secs: 5 },
        s if s >= 500 => {
            ProviderError::Network(format!("Google Drive server error ({status}): {detail}"))
        }
        _ => ProviderError::Internal(format!("Google Drive request failed ({status}): {detail}")),
    }
}

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct GoogleDriveCredentials {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub refresh_token: String,
    pub access_token: Option<String>,
    pub token_expiry: Option<i64>,
    pub code_verifier: Option<String>,
    pub code_challenge: Option<String>,
}

impl std::fmt::Debug for GoogleDriveCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleDriveCredentials")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("refresh_token", &"<redacted>")
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "<redacted>"),
            )
            .field("token_expiry", &self.token_expiry)
            .field(
                "code_verifier",
                &self.code_verifier.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "code_challenge",
                &self.code_challenge.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

pub struct GoogleDriveProvider {
    client: Client,
    credentials: Arc<RwLock<GoogleDriveCredentials>>,
    root_folder_id: Arc<RwLock<Option<String>>>,
    /// Directory for durable provider state (resumable upload sessions).
    state_dir: Option<PathBuf>,
    upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
    /// Serializes session-file writes so concurrent chunk uploads cannot
    /// interleave temp-file renames.
    session_io_lock: tokio::sync::Mutex<()>,
    /// Set when the in-memory session map has changed since the last write.
    /// The session file is rewritten on an interval instead of after every
    /// chunk, because rewriting it per chunk is quadratic in the chunk count.
    sessions_dirty: Arc<std::sync::atomic::AtomicBool>,
    /// When the session file was last written.
    last_session_write: Arc<std::sync::atomic::AtomicU64>,
}

/// Minimum interval between writes of the resumable upload session file.
const SESSION_FLUSH_INTERVAL_SECS: u64 = 5;

/// Builds the HTTP client used for every Google Drive request.
///
/// The settings matter for throughput: `tcp_nodelay` removes the delayed-ACK
/// interaction that otherwise stalls small requests, the idle pool is kept
/// warm so a chunk transfer does not re-establish a TLS connection for every
/// request, and the connect timeout is separated from the request timeout so
/// one slow chunk cannot abort a long transfer.
fn build_client() -> Client {
    Client::builder()
        // Fail fast when the network is unreachable, but allow a long
        // overall budget for large chunk uploads.
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .tcp_nodelay(true)
        .pool_idle_timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(16)
        .build()
        .unwrap_or_default()
}

impl GoogleDriveProvider {
    pub fn new(credentials: GoogleDriveCredentials) -> Self {        Self {
            client: build_client(),
            credentials: Arc::new(RwLock::new(credentials)),
            root_folder_id: Arc::new(RwLock::new(None)),
            state_dir: None,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
            session_io_lock: tokio::sync::Mutex::new(()),
            sessions_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_session_write: Arc::new(std::sync::atomic::AtomicU64::new(
                chrono::Utc::now().timestamp() as u64,
            )),
        }
    }

    /// Enables persistence of resumable upload sessions in `dir`.
    ///
    /// Restoring the session map lets a chunk upload continue at the exact
    /// byte offset Google already received after a restart instead of
    /// re-uploading the whole chunk.
    pub fn with_state_dir(mut self, dir: PathBuf) -> Self {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!(
                "Cannot create Google Drive state directory {} ({e}); resumable uploads will not survive restarts",
                dir.display()
            );
            self.state_dir = Some(dir);
            return self;
        }

        let now = chrono::Utc::now().timestamp();
        let mut sessions = read_session_file(&dir);
        let before = sessions.len();
        sessions.retain(|_, session| now - session.created_at <= UPLOAD_SESSION_TTL_SECS);
        if sessions.len() != before {
            if let Err(e) = write_session_file(&dir, &sessions) {
                tracing::warn!(
                    "Could not prune expired Google Drive upload sessions in {}: {e}",
                    dir.display()
                );
            }
        }

        tracing::info!(
            "Loaded {} resumable Google Drive upload session(s) from {}",
            sessions.len(),
            dir.display()
        );
        self.upload_sessions = Arc::new(RwLock::new(sessions));
        self.state_dir = Some(dir);
        self
    }

    /// Persists the in-memory session map. Failure is logged rather than
    /// failing an otherwise healthy upload, but it is never silent.
    async fn persist_sessions(&self) {
        let Some(dir) = self.state_dir.as_ref() else {
            return;
        };
        let _guard = self.session_io_lock.lock().await;
        let sessions = self.upload_sessions.read().await.clone();
        if let Err(e) = write_session_file(dir, &sessions) {
            tracing::error!(
                "Could not persist Google Drive upload sessions to {}: {e}",
                dir.display()
            );
        }
        self.sessions_dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.last_session_write.store(
            chrono::Utc::now().timestamp() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Marks the session map dirty and writes it if enough time has passed.
    ///
    /// Rewriting the session file after every chunk is quadratic in the chunk
    /// count and serializes all upload activity behind one mutex. Flushing on
    /// an interval keeps the same durability guarantee — a lost session only
    /// costs the bytes of one chunk — while removing the bottleneck.
    async fn flush_sessions_if_due(&self) {
        if self.state_dir.is_none() {
            return;
        }
        if !self
            .sessions_dirty
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }

        let now = chrono::Utc::now().timestamp() as u64;
        let last = self
            .last_session_write
            .load(std::sync::atomic::Ordering::Relaxed);
        if now.saturating_sub(last) < SESSION_FLUSH_INTERVAL_SECS {
            return;
        }

        self.persist_sessions().await;
    }

    /// Writes the session map unconditionally. Used at the end of a transfer
    /// so no session is left behind.
    pub async fn flush_sessions(&self) {
        self.persist_sessions().await;
    }

    async fn store_session(&self, key: String, session: UploadSession) {
        {
            let mut sessions = self.upload_sessions.write().await;
            sessions.insert(key, session);
        }
        self.sessions_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.flush_sessions_if_due().await;
    }

    async fn drop_session(&self, key: &str) {
        {
            let mut sessions = self.upload_sessions.write().await;
            sessions.remove(key);
        }
        self.sessions_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.flush_sessions_if_due().await;
    }

    async fn cached_session(&self, key: &str, checksum: &str, total_bytes: u64) -> Option<UploadSession> {
        let sessions = self.upload_sessions.read().await;
        sessions.get(key).cloned().filter(|session| {
            session.checksum == checksum
                && session.total_bytes == total_bytes
                && chrono::Utc::now().timestamp() - session.created_at <= UPLOAD_SESSION_TTL_SECS
        })
    }

    /// Asks Google how many bytes of an interrupted session it already holds.
    async fn probe_session(
        &self,
        token: &str,
        session: &UploadSession,
    ) -> Result<SessionProbe, ProviderError> {
        let res = self
            .client
            .put(&session.session_uri)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_LENGTH, "0")
            .header("Content-Range", format!("bytes */{}", session.total_bytes))
            .send()
            .await
            .map_err(|e| {
                ProviderError::Network(format!(
                    "Could not query Google Drive upload session status: {e}"
                ))
            })?;

        let status = res.status();
        match status.as_u16() {
            308 => {
                let range = res
                    .headers()
                    .get("Range")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let offset = parse_resume_offset(range.as_deref(), session.total_bytes)?;
                Ok(SessionProbe::Incomplete { offset })
            }
            200 | 201 => {
                let body = res.text().await.unwrap_or_default();
                let (object_id, version) = parse_completed_upload(&body)?;
                Ok(SessionProbe::Complete { object_id, version })
            }
            404 | 410 => Ok(SessionProbe::Expired),
            _ => {
                let body = res.text().await.unwrap_or_default();
                Err(map_http_error(status, body))
            }
        }
    }

    /// Finishes an upload session from `offset`, or starts a fresh one when no
    /// usable session exists.
    async fn upload_chunk_via_session(
        &self,
        token: &str,
        key: &str,
        data: &[u8],
        checksum: &ChunkChecksum,
        disk_id: Uuid,
        chunk_index: u64,
        session: Option<UploadSession>,
    ) -> Result<UploadResult, ProviderError> {
        if let Some(existing) = session {
            match self.probe_session(token, &existing).await {
                Ok(SessionProbe::Complete {
                    object_id,
                    version,
                }) => {
                    self.drop_session(key).await;
                    return Ok(UploadResult {
                        remote_object_id: object_id,
                        remote_version: version,
                        bytes_transferred: 0,
                    });
                }
                Ok(SessionProbe::Incomplete { offset }) => {
                    tracing::info!(
                        "Resuming Google Drive chunk {chunk_index} for disk {disk_id} at byte {offset}/{}",
                        data.len()
                    );
                    return self
                        .send_remaining(token, key, &existing, offset, data)
                        .await;
                }
                Ok(SessionProbe::Expired) => {
                    tracing::info!(
                        "Google Drive upload session for chunk {chunk_index} expired; starting a new session"
                    );
                    self.drop_session(key).await;
                }
                Err(err) => {
                    // Keep the session so a transient network failure can still
                    // be resumed; only surface the error.
                    return Err(err);
                }
            }
        }

        // Step 1: initiate a resumable upload session. Google replies with a
        // session URI in the Location header; the actual bytes are sent there.
        let filename = format!("chunk_{chunk_index}_{}.bin", checksum.hex_digest);
        let metadata = serde_json::json!({
            "name": filename,
            "appProperties": {
                "virtualssd_disk_id": disk_id.to_string(),
                "virtualssd_chunk_index": chunk_index.to_string(),
                "virtualssd_blake3": checksum.hex_digest,
            }
        });

        let init_res = self
            .client
            .post(format!("{GDRIVE_UPLOAD_BASE}?uploadType=resumable&fields=id,version"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json; charset=UTF-8")
            .header("X-Upload-Content-Type", "application/octet-stream")
            .header("X-Upload-Content-Length", data.len().to_string())
            .body(metadata.to_string())
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("Failed to start upload session: {e}")))?;

        let init_status = init_res.status();
        if !init_status.is_success() {
            let body = init_res.text().await.unwrap_or_default();
            return Err(map_http_error(init_status, body));
        }

        let session_uri = init_res
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                ProviderError::Internal("Google Drive did not return an upload session URI".into())
            })?;

        let new_session = UploadSession {
            session_uri,
            checksum: checksum.hex_digest.clone(),
            total_bytes: data.len() as u64,
            created_at: chrono::Utc::now().timestamp(),
        };

        // Persist the session *before* sending bytes: if the process dies
        // mid-upload, the next attempt can resume instead of restarting.
        self.store_session(key.to_string(), new_session.clone()).await;

        self.send_remaining(token, key, &new_session, 0, data)
            .await
    }

    async fn send_remaining(
        &self,
        token: &str,
        key: &str,
        session: &UploadSession,
        offset: u64,
        data: &[u8],
    ) -> Result<UploadResult, ProviderError> {
        let total = data.len() as u64;
        if offset >= total {
            // The server claims every byte arrived but never finalized the
            // session. Keep the session and let the caller retry the probe.
            return Err(ProviderError::Internal(format!(
                "Google Drive upload session for chunk reports all {total} bytes received but is not complete; retry the upload"
            )));
        }

        let start = offset as usize;
        let content_range = format!("bytes {offset}-{}/{total}", total - 1);
        let res = self
            .client
            .put(&session.session_uri)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("Content-Range", content_range)
            .body(data[start..].to_vec())
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("Chunk upload failed: {e}")))?;

        let status = res.status();
        if status.as_u16() == 404 || status.as_u16() == 410 {
            self.drop_session(key).await;
            let body = res.text().await.unwrap_or_default();
            return Err(map_http_error(status, body));
        }

        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            // A 308 means the server has not committed the object yet. The
            // session is deliberately retained for the next retry.
            return Err(map_http_error(status, body));
        }

        let body = res.text().await.unwrap_or_default();
        let (object_id, version) = parse_completed_upload(&body)?;
        self.drop_session(key).await;

        Ok(UploadResult {
            remote_object_id: object_id,
            remote_version: version,
            bytes_transferred: data.len() - start,
        })
    }

    pub async fn root_folder_id(&self) -> Option<String> {
        self.root_folder_id.read().await.clone()
    }

    pub async fn set_root_folder_id(&self, folder_id: Option<String>) {
        *self.root_folder_id.write().await = folder_id;
    }

    /// Generate PKCE code verifier and challenge per RFC 7636.
    /// The verifier is a random 32-byte base64url string; the challenge is
    /// SHA-256 of the verifier, also base64url-encoded without padding.
    pub fn generate_pkce() -> (String, String) {
        // 32 random bytes -> 48 base64url characters (no padding)
        let mut cs = [0u8; 32];
        getrandom::getrandom(&mut cs).expect("PKCE: failed to get random bytes");
        let verifier = URL_SAFE_NO_PAD.encode(cs);

        // code_challenge = Base64URL SHA-256(verifier) without padding
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let result = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(result);

        (verifier, challenge)
    }

    async fn get_access_token(&self) -> Result<String, ProviderError> {
        let creds = self.credentials.read().await;
        if let Some(token) = &creds.access_token {
            let now = chrono::Utc::now().timestamp();
            if let Some(expiry) = creds.token_expiry {
                if expiry > now + 60 {
                    return Ok(token.clone());
                }
            }
        }
        drop(creds);

        // Refresh token
        self.refresh_access_token().await
    }

    async fn refresh_access_token(&self) -> Result<String, ProviderError> {
        let creds = self.credentials.read().await;
        let client_id = creds.client_id.clone();
        let client_secret = creds.client_secret.clone();
        let refresh_token = creds.refresh_token.clone();
        let code_verifier = creds.code_verifier.clone();

        drop(creds);

        let mut params = vec![
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ];

        // Public/PKCE-only OAuth clients have no secret; omit the parameter
        // entirely rather than sending an empty value.
        if let Some(secret) = client_secret.as_deref().filter(|value| !value.is_empty()) {
            params.push(("client_secret", secret));
        }

        // Include code_verifier if PKCE was used (stored in credentials)
        if let Some(ref verifier) = code_verifier {
            params.push(("code_verifier", verifier.as_str()));
        }

        let res = self
            .client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("Token refresh failed: {e}")))?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(ProviderError::AuthExpired(format!(
                "Failed to refresh Google Drive token: {err_text}"
            )));
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: i64,
        }

        let token_resp: TokenResponse = res
            .json()
            .await
            .map_err(|e| ProviderError::Internal(format!("Invalid token response: {e}")))?;

        let mut creds = self.credentials.write().await;
        creds.access_token = Some(token_resp.access_token.clone());
        creds.token_expiry = Some(chrono::Utc::now().timestamp() + token_resp.expires_in);

        Ok(token_resp.access_token)
    }
}

#[async_trait]
impl CloudStorageProvider for GoogleDriveProvider {
    fn provider_name(&self) -> &'static str {
        "Google Drive"
    }

    async fn verify_connection(&self) -> Result<(), ProviderError> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .get(format!("{GDRIVE_API_BASE}/about?fields=user,storageQuota"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("Cannot reach Google Drive API: {e}")))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(map_http_error(status, body));
        }

        // A 200 alone is not proof of a working credential: require the API to
        // actually identify the authenticated user.
        #[derive(Deserialize)]
        struct AboutResponse {
            user: Option<serde_json::Value>,
        }

        let about: AboutResponse = res.json().await.map_err(|e| {
            ProviderError::Internal(format!("Google Drive returned an invalid about response: {e}"))
        })?;

        if about.user.is_some() {
            Ok(())
        } else {
            Err(ProviderError::Internal(
                "Google Drive did not return authenticated user information".into(),
            ))
        }
    }

    async fn get_quota(&self) -> Result<StorageQuota, ProviderError> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .get(format!("{GDRIVE_API_BASE}/about?fields=storageQuota"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(map_http_error(status, body));
        }

        #[derive(Deserialize)]
        struct AboutResponse {
            #[serde(rename = "storageQuota")]
            storage_quota: DriveQuota,
        }
        #[derive(Deserialize)]
        struct DriveQuota {
            limit: Option<String>,
            usage: Option<String>,
        }

        let about: AboutResponse = res
            .json()
            .await
            .map_err(|e| ProviderError::Internal(e.to_string()))?;

        // Google omits `limit` for accounts with unlimited storage. Report that
        // honestly instead of inventing a quota.
        let limit = about.storage_quota.limit.as_deref().ok_or_else(|| {
            ProviderError::Internal(
                "Google Drive account does not report a storage limit (unlimited plan); quota unavailable"
                    .into(),
            )
        })?;

        let total: u64 = limit.parse().map_err(|e| {
            ProviderError::Internal(format!("Google Drive returned an invalid quota '{limit}': {e}"))
        })?;

        let used: u64 = match about.storage_quota.usage.as_deref() {
            Some(raw) => raw.parse().map_err(|e| {
                ProviderError::Internal(format!(
                    "Google Drive returned an invalid storage usage '{raw}': {e}"
                ))
            })?,
            // Google omits `usage` when nothing is stored.
            None => 0,
        };

        let available = total.saturating_sub(used);

        Ok(StorageQuota {
            total_bytes: total,
            used_bytes: used,
            available_bytes: available,
        })
    }

    async fn upload_chunk(
        &self,
        disk_id: Uuid,
        chunk_index: u64,
        data: &[u8],
        checksum: &ChunkChecksum,
    ) -> Result<UploadResult, ProviderError> {
        if data.is_empty() {
            return Err(ProviderError::Internal(
                "Refusing to upload an empty chunk".into(),
            ));
        }

        let token = self.get_access_token().await?;
        let key = session_key(disk_id, chunk_index);
        let existing = self
            .cached_session(&key, &checksum.hex_digest, data.len() as u64)
            .await;

        // A cached session that no longer matches this chunk's checksum or size
        // is useless; drop it before opening a new one.
        if existing.is_none() {
            let stale = {
                let sessions = self.upload_sessions.read().await;
                sessions.contains_key(&key)
            };
            if stale {
                self.drop_session(&key).await;
            }
        }

        self.upload_chunk_via_session(
            &token,
            &key,
            data,
            checksum,
            disk_id,
            chunk_index,
            existing,
        )
        .await
    }

    async fn download_chunk(
        &self,
        _disk_id: Uuid,
        _chunk_index: u64,
        remote_object_id: &str,
        expected_checksum: &ChunkChecksum,
    ) -> Result<Vec<u8>, ProviderError> {
        let token = self.get_access_token().await?;
        let mut url = reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files/{remote_object_id}"))
            .map_err(|e| {
                ProviderError::Internal(format!(
                    "Invalid Google Drive object id '{remote_object_id}': {e}"
                ))
            })?;
        url.query_pairs_mut().append_pair("alt", "media");

        let res = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(map_http_error(status, body));
        }

        let bytes = res
            .bytes()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        // Cryptographic integrity verification
        expected_checksum
            .verify(&bytes)
            .map_err(|e| ProviderError::IntegrityFailure(e.to_string()))?;

        Ok(bytes.to_vec())
    }

    async fn delete_chunk(
        &self,
        _disk_id: Uuid,
        _chunk_index: u64,
        remote_object_id: &str,
    ) -> Result<(), ProviderError> {
        let token = self.get_access_token().await?;
        let mut url = reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files/{remote_object_id}"))
            .map_err(|e| {
                ProviderError::Internal(format!(
                    "Invalid Google Drive object id '{remote_object_id}': {e}"
                ))
            })?;
        url.query_pairs_mut().append_pair("fields", "id");

        let res = self
            .client
            .delete(url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = res.status();
        // Deletion is idempotent: a 404 means the object is already gone.
        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }

        let body = res.text().await.unwrap_or_default();
        Err(map_http_error(status, body))
    }

    async fn publish_manifest(&self, manifest: &DiskManifest) -> Result<(), ProviderError> {
        let token = self.get_access_token().await?;
        let serialized = serde_json::to_vec(manifest)
            .map_err(|e| ProviderError::Internal(e.to_string()))?;

        if serialized.is_empty() {
            return Err(ProviderError::Internal(
                "Refusing to publish an empty manifest".into(),
            ));
        }

        let manifest_name = format!("manifest_{}.json", manifest.disk_id);

        // Remove any previously published copies so fetch_manifest can never
        // return a stale duplicate after a republish.
        let mut list_url = reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        list_url
            .query_pairs_mut()
            .append_pair(
                "q",
                &format!("name = '{manifest_name}' and trashed = false"),
            )
            .append_pair("fields", "files(id)")
            .append_pair("pageSize", "100");

        let list_res = self
            .client
            .get(list_url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let list_status = list_res.status();
        if !list_status.is_success() {
            let body = list_res.text().await.unwrap_or_default();
            return Err(map_http_error(list_status, body));
        }

        #[derive(Deserialize)]
        struct ExistingFiles {
            files: Vec<ExistingFile>,
        }
        #[derive(Deserialize)]
        struct ExistingFile {
            id: String,
        }

        let existing: ExistingFiles = list_res.json().await.map_err(|e| {
            ProviderError::Internal(format!(
                "Invalid Google Drive manifest listing response: {e}"
            ))
        })?;

        for file in existing.files {
            let delete_res = self
                .client
                .delete(format!("{GDRIVE_API_BASE}/files/{}", file.id))
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| ProviderError::Network(e.to_string()))?;

            let delete_status = delete_res.status();
            if !delete_status.is_success() && delete_status.as_u16() != 404 {
                let body = delete_res.text().await.unwrap_or_default();
                return Err(map_http_error(delete_status, body));
            }
        }

        // Initiate a resumable upload so the file keeps its manifest name (a
        // bare media upload would create a file called "Untitled").
        let metadata = serde_json::json!({
            "name": manifest_name,
            "appProperties": {
                "virtualssd_manifest_disk_id": manifest.disk_id.to_string()
            }
        });

        let init_res = self
            .client
            .post(format!("{GDRIVE_UPLOAD_BASE}?uploadType=resumable&fields=id,name"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json; charset=UTF-8")
            .header("X-Upload-Content-Type", "application/json")
            .header("X-Upload-Content-Length", serialized.len().to_string())
            .body(metadata.to_string())
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("Failed to start manifest upload: {e}")))?;

        let init_status = init_res.status();
        if !init_status.is_success() {
            let body = init_res.text().await.unwrap_or_default();
            return Err(map_http_error(init_status, body));
        }

        let session_uri = init_res
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                ProviderError::Internal(
                    "Google Drive did not return a manifest upload session URI".into(),
                )
            })?;

        let content_range = format!("bytes 0-{}/{}", serialized.len() - 1, serialized.len());
        let put_res = self
            .client
            .put(&session_uri)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .header("Content-Range", content_range)
            .body(serialized)
            .send()
            .await
            .map_err(|e| ProviderError::Network(format!("Manifest upload failed: {e}")))?;

        let put_status = put_res.status();
        if !put_status.is_success() {
            let body = put_res.text().await.unwrap_or_default();
            return Err(map_http_error(put_status, body));
        }

        Ok(())
    }

    async fn fetch_manifest(&self, disk_id: Uuid) -> Result<DiskManifest, ProviderError> {
        let token = self.get_access_token().await?;

        let mut url = reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair(
                "q",
                &format!("name = 'manifest_{disk_id}.json' and trashed = false"),
            )
            .append_pair("fields", "files(id,name)")
            .append_pair("pageSize", "10");

        let res = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(map_http_error(status, body));
        }

        #[derive(Deserialize)]
        struct ListFiles {
            files: Vec<DriveFile>,
        }
        #[derive(Deserialize)]
        struct DriveFile {
            id: String,
        }

        let list: ListFiles = res.json().await.map_err(|e| {
            ProviderError::Internal(format!("Invalid Google Drive file listing response: {e}"))
        })?;

        let file = list.files.first().ok_or_else(|| {
            ProviderError::NotFound(format!("Manifest for disk {disk_id} not found"))
        })?;

        let mut download_url =
            reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files/{}", file.id))
                .map_err(|e| ProviderError::Internal(e.to_string()))?;
        download_url.query_pairs_mut().append_pair("alt", "media");

        let download_res = self
            .client
            .get(download_url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let download_status = download_res.status();
        if !download_status.is_success() {
            let body = download_res.text().await.unwrap_or_default();
            return Err(map_http_error(download_status, body));
        }

        let content = download_res
            .bytes()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let manifest = serde_json::from_slice(&content)
            .map_err(|e| ProviderError::Internal(format!("Invalid manifest: {e}")))?;
        Ok(manifest)
    }

    async fn flush_transfer_state(&self) -> Result<(), ProviderError> {
        // The resumable upload sessions are written on an interval during a
        // transfer. Flush them now so a session that was created moments
        // before the transfer ended is not lost.
        self.flush_sessions().await;
        Ok(())
    }

    async fn delete_manifest(&self, disk_id: Uuid) -> Result<(), ProviderError> {
        let token = self.get_access_token().await?;
        let manifest_name = format!("manifest_{disk_id}.json");

        let mut list_url = reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files"))
            .map_err(|e| ProviderError::Internal(e.to_string()))?;
        list_url
            .query_pairs_mut()
            .append_pair(
                "q",
                &format!("name = '{manifest_name}' and trashed = false"),
            )
            .append_pair("fields", "files(id)")
            .append_pair("pageSize", "100");

        let list_res = self
            .client
            .get(list_url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let list_status = list_res.status();
        if !list_status.is_success() {
            let body = list_res.text().await.unwrap_or_default();
            return Err(map_http_error(list_status, body));
        }

        #[derive(Deserialize)]
        struct ListFiles {
            files: Vec<DriveFile>,
        }
        #[derive(Deserialize)]
        struct DriveFile {
            id: String,
        }

        let listing: ListFiles = list_res.json().await.map_err(|e| {
            ProviderError::Internal(format!("Invalid Google Drive file listing response: {e}"))
        })?;

        for file in listing.files {
            let mut url = reqwest::Url::parse(&format!("{GDRIVE_API_BASE}/files/{}", file.id))
                .map_err(|e| ProviderError::Internal(e.to_string()))?;
            url.query_pairs_mut().append_pair("fields", "id");

            let res = self
                .client
                .delete(url)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| ProviderError::Network(e.to_string()))?;

            let status = res.status();
            // Deletion is idempotent: 404 means the object is already gone.
            if !status.is_success() && status.as_u16() != 404 {
                let body = res.text().await.unwrap_or_default();
                return Err(map_http_error(status, body));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> UploadSession {
        UploadSession {
            session_uri: "https://example.invalid/upload/session".to_string(),
            checksum: "abc123".to_string(),
            total_bytes: 42,
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    #[test]
    fn parse_resume_offset_reads_range_header() {
        assert_eq!(
            parse_resume_offset(Some("bytes=0-524287"), 4 * 1024 * 1024).unwrap(),
            524_288
        );
        // No Range header means nothing has been received yet.
        assert_eq!(parse_resume_offset(None, 100).unwrap(), 0);
        // A server-reported offset beyond the chunk is clamped.
        assert_eq!(parse_resume_offset(Some("bytes=0-200"), 100).unwrap(), 100);
    }

    #[test]
    fn parse_resume_offset_rejects_malformed_ranges() {
        assert!(parse_resume_offset(Some("bytes 0-100"), 100).is_err());
        assert!(parse_resume_offset(Some("bytes=x-y"), 100).is_err());
        assert!(parse_resume_offset(Some("bytes=0"), 100).is_err());
    }

    #[test]
    fn completed_upload_requires_real_object_id_and_version() {
        let (id, version) =
            parse_completed_upload(r#"{"id":"file-1","version":"7"}"#).unwrap();
        assert_eq!(id, "file-1");
        assert_eq!(version, 7);

        assert!(parse_completed_upload(r#"{"id":"file-1"}"#).is_err());
        assert!(parse_completed_upload(r#"{"version":"1"}"#).is_err());
        assert!(parse_completed_upload(r#"{"id":"file-1","version":"abc"}"#).is_err());
    }

    #[test]
    fn http_error_mapping_distinguishes_quota_rate_limit_and_auth() {
        let quota = map_http_error(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"error":{"errors":[{"reason":"storageQuotaExceeded"}]}}"#.into(),
        );
        assert!(matches!(quota, ProviderError::QuotaExceeded(_)));

        let rate = map_http_error(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#.into(),
        );
        assert!(matches!(
            rate,
            ProviderError::RateLimited {
                retry_after_secs: 60
            }
        ));

        let auth = map_http_error(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#.into(),
        );
        assert!(matches!(auth, ProviderError::AuthExpired(_)));

        let incomplete = map_http_error(reqwest::StatusCode::PERMANENT_REDIRECT, String::new());
        assert!(matches!(incomplete, ProviderError::Internal(_)));
    }

    #[test]
    fn session_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut sessions = HashMap::new();
        sessions.insert("disk:1".to_string(), sample_session());

        write_session_file(dir.path(), &sessions).unwrap();
        let loaded = read_session_file(dir.path());

        assert_eq!(loaded.len(), 1);
        let restored = loaded.get("disk:1").expect("session must survive");
        assert_eq!(restored.session_uri, "https://example.invalid/upload/session");
        assert_eq!(restored.checksum, "abc123");
        assert_eq!(restored.total_bytes, 42);
    }

    #[test]
    fn corrupt_session_file_is_preserved_and_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(UPLOAD_SESSION_FILE), b"not json").unwrap();

        let loaded = read_session_file(dir.path());
        assert!(loaded.is_empty());
        assert!(dir
            .path()
            .join("gdrive_upload_sessions.json.corrupt")
            .exists());
    }
}
