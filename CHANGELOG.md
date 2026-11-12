# Changelog

All notable changes to VirtualSSD are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The marketing version is `1.0.0`. Every build carries a build number taken
from the commit it was built from, so the released artifact is always
identifiable — for example `1.0.0+42`.

## [Unreleased]

### Added
- **A Google Drive setup guide and a token helper.** `docs/google-drive-setup.md`
  walks through creating a Google Cloud project, enabling the Drive API,
  configuring the OAuth consent screen, and creating a Desktop app OAuth
  client. `scripts/get-google-drive-token.ps1` then performs the PKCE consent
  flow in the browser and prints the client id and refresh token to paste into
  **Cloud Providers**. The script writes nothing to disk and stores no secret.
  The Cloud Providers page now points at both.
- **Volume Shadow Copy mirroring.** A mounted disk can now be mirrored
  without being unmounted. *Mirror While Mounted* takes a volume shadow copy
  of the volume that holds the VHDX, flushes the guest file system into the
  VHDX first with `FSCTL_FLUSH_VOLUME`, and mirrors the shadow copy with the
  existing chunk pipeline. The volume stays mounted and usable for the whole
  transfer. This is a documented Windows API — no kernel driver, no EV
  certificate, and no WHQL attestation. It requires administrator
  privileges, and the unmounted mirror path is unchanged and remains the
  fallback.
- Real backend engine log stream. The Diagnostics page renders actual
  `tracing` events from the Rust backend and the shared provider/storage
  crates, delivered live over a Tauri event and backed by a bounded
  500-entry ring buffer. The frontend never fabricates log entries.
- Branded application icon. The four-square sidebar mark is now the single
  source of truth for the window, the taskbar, the system tray, the
  uninstaller, and both installers. The artwork is generated from
  `apps/desktop/app-icon.svg`.
- System tray integration. Left-clicking the tray icon shows or hides the main
  window; right-clicking opens **Show / Hide VirtualSSD**, **Diagnostics**, and
  **Quit VirtualSSD**. Closing the main window hides the app to the tray
  instead of terminating it.
- Per-build versioning. The build number changes with every build and is
  written into the Windows version resource, the installer filename, the
  application footer, and the release tag.
- Authenticode code signing for the executable and both installers, with an
  RFC 3161 timestamp and SHA-256 digests.
- Windows resource metadata: CompanyName `Chaudary Global`, LegalCopyright
  `Copyright (c) 2026 Ahmad Abdullah Chaudary, Chaudary Global`.
- `CHANGELOG.md` and a release-notes template for GitHub releases.

### Performance
- **Concurrent chunk transfers.** The VHDX mirror now uses a bounded
  producer/consumer pipeline: the file is read and hashed while up to four
  chunks upload at the same time, instead of one chunk at a time.
- **Concurrent restore.** Chunk downloads run concurrently and are written
  straight to their offset in the preallocated temporary file.
- **Manifest checkpointing.** The manifest is written on a checkpoint interval
  instead of once per chunk. The previous behaviour was quadratic in the chunk
  count and dominated the wall-clock time of a large mirror.
- **Debounced Google Drive session persistence.** The resumable upload session
  file is flushed on an interval and at the end of a transfer, instead of
  being rewritten after every chunk.
- **Tuned HTTP clients.** `tcp_nodelay`, a warm idle connection pool, and a
  connect timeout separated from the request timeout.
### Changed
- The Dashboard sidebar badge shows the real number of known disks and is
  hidden when there are none. The previous hardcoded fallback value was
  removed.
- The Tauri CLI was aligned with the `tauri` crate (2.12.4 / 2.12.5). The
  previous pin at 2.0.0 could not express the NSIS uninstaller icon and
  validated the config against an outdated schema.
- The release build script now resolves the Cargo target directory instead of
  assuming `./target`, which this workspace redirects to `C:\cargo-build`.

### Fixed
- A provider connection failure now says what actually happened. The message
  used to be `Network connection failure: ... error sending request for url`,
  which hid the cause. It now distinguishes a blocked or filtered
  destination, a DNS failure, a timeout, a refused connection and a TLS
  failure, and tells the user what to do about each. A reset connection is
  reported as *"The connection was closed or reset before any data was
  exchanged"*, with the advice to use a different network, a VPN, or the
  standard `HTTPS_PROXY`/`ALL_PROXY` environment variables. SOCKS5 proxies
  are now supported.
- Telegram `publish_manifest` returned success even when the pinned manifest
  catalog could not be updated, which left a fresh installation unable to
  discover the disk. It now returns an explicit error so the sync is retried.
- **A Telegram bot that cannot pin messages no longer blocks disk creation.**
  Publishing a manifest uploads the manifest document and then updates a
  pinned catalog, which is how another machine discovers the disks. Telegram
  only lets a bot pin in a channel, or in a group where the bot is an
  administrator, so a bot in an ordinary group or a private chat could add the
  provider and then fail every disk creation. The catalog update is now a
  warning rather than a failure: the disk is created and can be mirrored, and
  the application tells the user that another machine must import the disk by
  its ID. The provider also checks the bot's pin capability when it is added,
  so the limitation is reported up front instead of at the first failure.
- **The Create Disk dialog no longer resets itself.** The form was reset
  whenever the provider list changed, and the application polls the backend, so
  the volume name and any error message were wiped every few seconds. The
  form is now reset when the dialog opens, not on every refresh.
- The Diagnostics page previously showed hardcoded startup lines and a fake
  "manual refresh" entry; both are removed.

### Security
- Provider credentials are stored DPAPI-encrypted under
  `%LOCALAPPDATA%\VirtualSSD`; the provider index contains no secrets.
- Recovery keys are never written to the log stream; only the fact that a key
  was exported or imported is recorded.
- Release binaries are Authenticode-signed. The signing step reports
  explicitly when no certificate is available rather than shipping unsigned
  binaries silently.

### Documentation
- `docs/performance-and-robustness.md` records the measured bottlenecks, the
  fixes that were implemented, and the limitations that remain.


## [1.0.0] — 2026-11-07

### Added
- VHDX creation, attach, detach, GPT initialization, NTFS formatting, and
  drive-letter assignment through the Windows Virtual Disk API.
- Unmounted VHDX snapshot mirroring to Google Drive and Telegram, with
  per-chunk integrity metadata and manifest publication.
- Automatic VHDX restore on mount, including encrypted chunks, with
  temp-file verification before replacement.
- Remote manifest import by disk UUID and provider rebinding.
- Local-only or local-plus-remote deletion with explicit provider
  confirmation.
- Optional AES-256-GCM chunk encryption, DPAPI-protected keys, and hex
  recovery-key export/import.
- Google Drive REST adapter: OAuth PKCE, real resumable upload protocol,
  persisted sessions, quota parsing, named manifest publish/fetch/delete.
- Telegram Bot API adapter: 4 MiB document chunks, checksum dedupe,
  idempotent delete, 50 MiB limit handling, pinned manifest catalog, and
  legacy fallback discovery.
- Editable, validated runtime settings with corrupt-file preservation.

### Known limitations
- Mirroring is point-in-time and requires the disk to be unmounted. Windows
  I/O is not intercepted, so there is no live write-through.
- Encryption is fixed at disk creation.
- Telegram Bot API cannot report account storage quota.
- No kernel-mode virtual disk driver or Windows service is shipped.

[Unreleased]: https://github.com/ahmad-abdullah-chaudary/virtualssd/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/ahmad-abdullah-chaudary/virtualssd/releases/tag/v1.0.0
