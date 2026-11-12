# VirtualSSD Troubleshooting & Diagnostics Guide

---

## Common Issues and Diagnostic Actions

### 1. Disk Mount Failed: Access Denied
- **Cause**: Attaching a VHDX through the Windows Virtual Disk API requires administrative privileges. The application starts with `asInvoker`.
- **Resolution**:
  - Use **Restart as administrator** in the header or on the Settings page, then retry the mount.
  - The backend checks elevation at runtime and returns this error before touching Windows. No service or named-pipe endpoint is involved.

### 2. Disk Shows as "Offline" in Disk Management
- **Cause**: Windows storage policy may set newly attached virtual disks to offline if SAN policy is set to `OfflineShared`.
- **Resolution**:
  - In PowerShell (Admin):
    ```powershell
    Get-Disk | Where-Object FriendlyName -like "*VirtualSSD*" | Set-Disk -IsOffline $false -IsReadOnly $false
    ```

### 3. Google Drive Rate Limiting (403 rateLimitExceeded / userRateLimitExceeded)
- **Cause**: Too many API requests in a short period.
- **Resolution**:
  - The adapter maps these reasons to `RateLimited` with a 60-second retry hint instead of treating them as an authentication failure.
  - There is no background retry loop and no editable concurrency setting. Wait before retrying the sync manually, and avoid starting concurrent syncs for multiple disks.
  - Quota exhaustion (`storageQuotaExceeded`, `quotaExceeded`, `insufficient storage`) is reported separately as `QuotaExceeded`.

### 4. Telegram Rate Limits (HTTP 429)
- **Cause**: Too many Bot API requests, or a chat-level rate limit.
- **Resolution**:
  - The adapter maps HTTP 429 to `RateLimited` using Telegram's `parameters.retry_after` value when present (15 seconds default).
  - The bot token is redacted from the error text. If the problem persists, check the bot's permissions in the storage chat and reduce concurrent disk syncs.

### 5. Snapshot Sync or Restore Fails
- **Cause**: Provider error, quota exhaustion, rate limiting, an unmounted-disk requirement, a missing encryption key, or a checksum mismatch.
- **Resolution**:
  - Sync refuses to run while the disk is `Online`; unmount it first.
  - An encrypted disk shows an amber warning when its recovery key is missing. Import the 64-character key on the Disks page.
  - A restore downloads to `disk.vhdx.restore.tmp`; on failure the temp file is removed and the existing `disk.vhdx` is left untouched.
  - A failed remote delete keeps the local disk and its remote object ids so the operation can be retried.
  - Check the provider error text on the Disks page or in the notification. Google 403 quota, Google 403 rate-limit, and missing Telegram quota are reported as distinct states.

### 6. Settings Warning on Startup
- **Cause**: `settings.json` could not be read, parsed, or validated.
- **Resolution**:
  - Invalid JSON is preserved as `settings.json.corrupt-<timestamp>`.
  - Defaults are active until you save settings again. Open **Settings**, correct the values, and save.

### 7. Creating a disk fails with a Telegram catalog or pin error
- **Cause**: the adapter publishes the disk manifest and then updates a pinned
  catalog document, which is how another machine discovers the disks.
  Telegram only allows a bot to pin messages in a **channel**, or in a group
  where the bot is an **administrator**. A bot in an ordinary group or in a
  private chat cannot pin, so the catalog update fails.
- **Resolution**:
  - The disk is still created and can still be mirrored. The application shows
    a warning explaining that another machine will not discover the disk
    automatically and must **import it by its disk ID** instead.
  - To get automatic discovery, use a channel, or promote the bot to
    administrator in the group.
  - The provider reports this limitation when it is added, so you see it before
    the first disk is created.

### 8. The Create Disk dialog clears itself
- **Cause**: this was a bug, fixed. The form was reset whenever the provider
  list refreshed, and the application polls the backend, so the volume name and
  any error message were wiped every few seconds.
- **Resolution**: the form is now reset only when the dialog is opened.

### 9. Diagnostics
- The desktop Diagnostics page shows the **live backend engine log**: real entries emitted by the Rust `tracing` subscriber in the desktop process and the shared `storage-core`, `provider-gdrive`, and `provider-telegram` crates.
- The stream is delivered over a Tauri event (`engine-log`) and is also retrievable with the `get_engine_logs` command, which returns the most recent 500 records of the current session. **Clear** empties that buffer and the clear action is itself recorded in the backend.
- The page does not attach a kernel trace probe, and the browser preview has no Rust backend, so it shows an explicit "backend log stream unavailable" state instead of placeholder entries.
- A standalone `virtualssd-diag.exe` export bundle is not implemented in this build. Provider tokens are redacted from the application log, but review log text before sharing it.
- Set `RUST_LOG` before launching to change the filter, for example `RUST_LOG=info,virtualssd_storage_core=debug`. The default filter is `info` with noisy HTTP and async runtimes muted.
