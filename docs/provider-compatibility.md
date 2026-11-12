# Provider Compatibility Matrix: Google Drive & Telegram

- **Status**: Implemented behavior
- **Revision**: 2.1 (2026-11-07)

This document describes what the current adapters actually do. It intentionally does not repeat the earlier TDLib/MTProto or SHA-256 cross-check design claims that were never implemented.

---

## 1. Provider Capabilities Comparison

| Dimension | Google Drive | Telegram |
| :--- | :--- | :--- |
| **Official API** | Google Drive REST API v3 | Telegram Bot API |
| **Authentication** | OAuth 2.0 PKCE + refresh token | Bot token + numeric storage chat id |
| **Max Object Size** | Drive file limit (4 MiB chunks used) | 50 MiB per `sendDocument` upload |
| **Chunk Size** | 4 MiB | 4 MiB |
| **Resumable Uploads** | Yes (`uploadType=resumable`), sessions persisted for 6 days | No per-chunk resumable protocol; checksum dedupe avoids duplicate uploads |
| **Rate Limits** | HTTP 429 mapped to `RateLimited` | HTTP 429 uses Telegram `parameters.retry_after` when present |
| **Quota API** | `about?fields=storageQuota` | No Bot API quota endpoint |
| **Unlimited / Missing Quota** | Stored as `0`; UI says “Not reported by Google” | Stored as `0`; UI says “Not exposed by Bot API” |
| **Metadata** | File name + `appProperties` | Message caption + SQLite mapping table |
| **Deletion** | Drive file delete endpoint | `deleteMessage` plus local mapping removal |
| **Manifest Discovery** | Query by `manifest_<disk-id>.json` | Pinned catalog document, with legacy pinned per-disk manifest fallback |
| **Durability Classification** | Primary reliable cloud object store | Secondary object store with no archival SLA |

---

## 2. Google Drive Adapter

### Upload protocol

1. `POST https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,version`
   with file metadata (`name`, `appProperties`).
2. Read the session URI from the `Location` header. A missing `Location` is an error.
3. `PUT` the 4 MiB chunk bytes with `Content-Range: bytes 0-(n-1)/n`.
4. Accept only a completed 200/201 response containing a real `id` and numeric `version`.
   HTTP 308 means the upload is incomplete and is retried, never reported as success.

### Persistent resume

- Session URIs, checksums, and total sizes are stored in
  `providers/gdrive_<id8>/gdrive_upload_sessions.json`.
- Before retrying, the adapter probes the session with
  `Content-Range: bytes */<total>` and parses the returned `Range` header for the
  already-received offset.
- Sessions older than six days are treated as expired and restarted.
- A corrupt session file is preserved as `gdrive_upload_sessions.json.corrupt`
  and ignored.
- A probe that finds a completed upload reuses the existing object instead of
  uploading the chunk twice.

### Errors

- `401`: token refresh, then `AuthExpired` if refresh fails.
- `403` with `quotaExceeded`, `storageQuotaExceeded`, or `insufficient storage`:
  `QuotaExceeded`.
- `403` with `rateLimitExceeded` / `userRateLimitExceeded`: `RateLimited` with a
  60-second retry hint.
- `403` without a quota or rate-limit reason: `AuthExpired` / permission error.
- `404`: `NotFound`.
- `413` / `507`: `QuotaExceeded`.
- `429`: `RateLimited`.
- `5xx`: `Network`.

### Manifest publication

- The manifest is uploaded as `manifest_<disk-id>.json`.
- Any stale copies with the same name are deleted before the new file is uploaded,
  so a later fetch cannot select an old copy.
- `fetch_manifest` queries Drive by exact file name and validates the downloaded
  JSON.
- `create_disk` publishes the initial manifest and rolls back the local disk
  directory if publication fails.
- Every snapshot sync republishes the manifest, including syncs with no changed
  chunks, so a previous publish failure can be retried.
- `delete_manifest` removes every non-trashed copy with that exact name and
  reports failure instead of claiming success.

---

## 3. Telegram Adapter

### Storage model

- Chunks are uploaded as documents to the configured private storage chat through
  `sendDocument` using the bot token.
- The Bot API limit for a document is 50 MiB; VirtualSSD uses 4 MiB chunks.
- A SQLite database (`providers/tg_<id8>.db`) maps
  `(disk_id, chunk_index)` to `(file_id, message_id, checksum, size)`.
- Before uploading, the adapter checks the mapping and reuses the existing remote
  object when the checksum matches.

### Error handling

- `429`: `RateLimited { retry_after_secs }`, using Telegram's
  `parameters.retry_after` when supplied (default 15 seconds).
- `413`: explicit “chunk exceeds the Telegram upload limit” error.
- `400` / `403`: `AuthExpired` or a provider error with the redacted API description.
- `5xx`: `Network`.
- The bot token is redacted from error messages and omitted from serialized
  configuration.

### Manifest discovery

- On publish, the manifest document is uploaded and then merged into a pinned
  catalog document named `virtualssd_manifest_catalog.json`.
- Catalog entries are `{disk_id, message_id, file_id}`.
- A fresh installation calls `getChat`, downloads the pinned catalog, finds the
  requested disk, and downloads its manifest without relying on local state.
- If the catalog is missing or unreadable, the adapter falls back to a legacy
  pinned per-disk manifest document.
- A per-provider `manifest_lock` serializes catalog read-modify-write cycles so
  concurrent disk syncs cannot drop entries.
- After the catalog successfully references a new manifest, the previous manifest
  message for that disk is deleted best-effort. If the catalog upload or pin fails,
  `publish_manifest` returns an error instead of claiming success, because a fresh
  installation would not discover the disk; the next sync retries and removes the
  superseded message.

### Quota

The Telegram Bot API does not expose account storage quota. `get_quota` returns an
explicit error; the desktop maps that to `0` bytes and the UI displays
“Not exposed by Bot API”. No quota value is invented.

---

## 4. VHDX Snapshot Namespace

Both providers store the mirrored VHDX file under a namespace distinct from the
block-engine chunk namespace:

- VHDX chunk objects use `vhdx_namespace_id(disk_id)`, a UUID v5 of
  `"{disk_id}:vhdx-file-v1"`.
- The manifest keeps `vhdx_chunks` and `vhdx_file_size` separate from the
  block-engine `chunks`.
- Zero regions are not uploaded. If a previously mirrored region becomes zero or
  the file shrinks, the old remote object is deleted before the manifest is
  republished.
- Re-uploading a changed chunk records the new object id only after the provider
  returns it. Google then deletes the superseded object; Telegram's upload path
  replaces the chat message and deletes the previous one.
- Restore downloads every manifest-referenced chunk, verifies the plaintext
  checksum (and ciphertext checksum when encrypted), writes to
  `disk.vhdx.restore.tmp`, and renames it to `disk.vhdx` only on full success.

---

## 5. Shared Limitations

- Client-side chunk encryption is optional per disk and fixed at creation. Disks
  without it rely on provider account security for confidentiality.
- The Telegram Bot API carries the bot token in the URL path, so token secrecy
  depends on the local process and transport security.
- Remote deletion is available from the desktop delete dialog for an unmounted
  disk. It removes referenced chunks first, then the manifest, and only then
  clears local remote references; a failure leaves the local disk retryable.
- Google Drive quota values are only as accurate as the Drive API response for the
  authenticated account.
- Mirroring is point-in-time and requires the disk to be unmounted. Live Windows
  I/O is not intercepted.
