# VirtualSSD

**Windows Virtual Block Storage with Snapshot Cloud Mirroring, Encryption, and Google Drive / Telegram Providers**

[![Windows 11](https://img.shields.io/badge/Windows-11%20x64-0078D6.svg?logo=windows)](https://microsoft.com)
[![Rust 1.80+](https://img.shields.io/badge/Rust-1.98+-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Tauri 2.0](https://img.shields.io/badge/Tauri-2.0-blue.svg?logo=tauri)](https://tauri.app)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

VirtualSSD creates **authentic, Windows-visible virtual block devices** (VHDX) through the Windows Virtual Disk API. Unmounted disks are mirrored to Google Drive or Telegram as 4 MiB chunks and can be restored on another installation. Chunk payloads can be client-side encrypted with AES-256-GCM.

| | |
|---|---|
| **Company** | Chaudary Global |
| **Developer** | Ahmad Abdullah Chaudary |
| **Country** | PK |
| **Version** | 1.0.0 — the build number changes with every build (`1.0.0+N`) |
| **Releases** | [github.com/ahmad-abdullah-chaudary/virtualssd/releases](https://github.com/ahmad-abdullah-chaudary/virtualssd/releases) |

---

## Key Capabilities

- **Real Block Devices**: Windows-visible VHDX volumes attached through the Windows Virtual Disk API. Mounting initializes GPT, creates a partition, and formats NTFS automatically; an existing non-NTFS volume is never silently reformatted. Attach/detach requires administrator privileges.
- **Snapshot Cloud Mirroring**: Unmount a disk, choose **Mirror Snapshot**, and the VHDX is split into 4 MiB chunks, uploaded, and recorded in the manifest. Missing local VHDX files are restored and checksum-verified on mount.
- **Client-Side Encryption**: Optional per-disk AES-256-GCM with a DPAPI-protected random key. Export a 64-character recovery key and import it on another installation.
- **Dual Cloud Backends**:
  - **Google Drive**: Official Drive REST API v3 with OAuth 2.0 PKCE, protocol-correct resumable uploads, persisted sessions (6-day resume window), revision tracking, named manifest publication, and remote deletion.
  - **Telegram**: Bot API storage chat with 4 MiB chunk documents, checksum dedupe, superseded-message cleanup, remote deletion, and a pinned manifest catalog that lets a fresh installation discover every disk.
- **Remote Disk Import**: Fetch a published manifest by disk UUID on a fresh installation, rebind it to a local provider, and restore the VHDX on first mount.
- **Remote Deletion**: Delete a disk locally, or explicitly delete its mirrored chunks and manifest from the provider first.
- **Sparse 4 MiB Chunk Architecture**: Only written sectors consume storage; unwritten sectors return zeros instantly without cloud network calls. All-zero VHDX regions are not uploaded.
- **Durable Write-Ahead Journal (WAL)**: Append-only journal with fsync on commit and idempotent replay when a disk engine is opened. This covers the block-engine API path; mounted Windows VHDX I/O is handled by Windows and is not journaled here.
- **Persistent Bounded Cache**: Per-disk engine cache with clean/dirty state tracking. The limit is editable in Settings and applied to open disks immediately. The desktop does not route mounted VHDX I/O through this cache.
- **Cryptographic Integrity**: BLAKE3-256 checksums for every chunk, plus a ciphertext checksum for encrypted chunks. On-demand audits distinguish missing local chunks from checksum failures.
- **Encrypted Provider Credentials**: Provider secrets are encrypted with Windows DPAPI for the current user (`cred_<id>.bin`); the provider index contains no secrets, and credential Debug output is redacted and zeroized on drop.
- **Honest Provider State**: Quota fields report `0` when the provider does not expose a value, and the UI distinguishes "not reported" from a real zero. Provider and upload failures surface as errors instead of silent success.
- **Editable Settings**: Cache limit, telemetry polling interval, default disk capacity, encryption default, and post-sync verification are persisted in `settings.json` with validation and atomic writes.
- **Real Backend Log Stream**: The Diagnostics page shows actual `tracing` events from the Rust backend and the shared provider/storage crates, delivered live over a Tauri event and backed by a bounded 500-entry ring buffer. The frontend never fabricates log entries.
- **Branded Application Icon and System Tray**: The four-square sidebar mark is the single source of truth for the window, taskbar, system tray, and installer artwork, generated from `apps/desktop/app-icon.svg`. The tray provides show/hide, a Diagnostics shortcut, and a real quit entry; closing the window hides the app to the tray instead of terminating it.

---

## Implementation Status

**Implemented in this build:**

- VHDX creation, attach, detach, GPT initialization, NTFS formatting, and drive-letter assignment through the Windows Virtual Disk API.
- Unmounted VHDX snapshot mirroring to Google Drive and Telegram, with per-chunk integrity metadata and manifest publication.
- Automatic VHDX restore on mount, including encrypted chunks, with temp-file verification before replacement.
- Remote manifest import by disk UUID and provider rebinding.
- Local-only or local-plus-remote deletion with explicit provider confirmation.
- Optional AES-256-GCM chunk encryption, DPAPI-protected keys, and hex recovery-key export/import.
- 4 MiB sparse chunk engine with WAL, persistent cache, BLAKE3 checksums, manifest atomic writes, and local integrity audits.
- Google Drive REST adapter: OAuth PKCE, real resumable upload protocol, persisted sessions, quota parsing, named manifest publish/fetch/delete.
- Telegram Bot API adapter: 4 MiB document chunks, checksum dedupe, idempotent delete, 50 MiB limit handling, pinned manifest catalog + legacy fallback discovery.
- DPAPI-encrypted provider credentials with a secret-free index, redacted Debug output, zeroization, and provider rebinding after restart.
- Provider quota/error states are surfaced honestly; missing quota is stored as `0` and labeled as not reported/not exposed.
- Editable, validated runtime settings with corrupt-file preservation.
- Live backend engine log stream: a bounded in-memory ring buffer fed by the Rust `tracing` subscriber, exposed through `get_engine_logs`/`clear_engine_logs` and streamed to the UI as `engine-log` events.

**Known limitations:**

- Mirroring is **point-in-time and requires the disk to be unmounted**. Windows I/O is not intercepted, so there is no live write-through or write-back to the cloud.
- Encryption is fixed at disk creation. Converting an existing unencrypted disk requires re-uploading a new snapshot.
- There is no background auto-sync, scheduled scrubbing, automatic mount on startup, or parallel upload tuning; sync and audits are manual.
- Integrity audits verify locally present chunks. Remote chunks are verified during restore, not downloaded for an audit.
- Telegram Bot API cannot report account storage quota; the UI says “Not exposed by Bot API”.
- No kernel-mode virtual disk driver or Windows service is shipped. VHDX attachment uses the Windows Virtual Disk API and requires administrator privileges.
- A damaged manifest is recovered from `manifest.json.prev` when possible; if both files are unreadable, the disk is logged and skipped at startup rather than guessed at.

---

See [Architecture Decision Record (ADR-001)](docs/architecture.md) for technical design details.

```
virtualssd/
  apps/
    desktop/               # Tauri + React + TypeScript + Tailwind (Light-Only Theme)
  crates/
    storage-core/          # Block engine, VHDX snapshot mirror/restore, encryption, WAL
    block-map/             # LBA to Chunk indexing and sparse extent tracking
    manifest/              # Versioned disk manifest & commit records
    journal/               # Write-Ahead Log (WAL) with crash-recovery replay
    cache-manager/         # Persistent local cache with dirty tracking & evictions
    integrity/             # BLAKE3 & SHA-256 integrity verifiers
    provider-contract/     # Unified cloud backend traits & lifecycle
    provider-gdrive/       # Google Drive REST v3 adapter with OAuth 2.0 PKCE
    provider-telegram/     # Telegram Bot API adapter
    security/              # Windows DPAPI secret protection and path sanitization
  archive/                 # Quarantined, unsupported prototypes (not built or shipped)
  docs/                    # Architecture, recovery, release, security, and provider docs
```

---

## Quick Start

### Prerequisites
- Windows 11 x64 or Windows 10 x64 (Build 19044+)
- Rust 1.80+ (`rustup default stable`)
- Node.js 20+ and `pnpm` (`npm install -g pnpm`)

### Build and Run Storage Core & Tests
```bash
# Compile every workspace target (including the desktop backend)
cargo check --workspace --all-targets

# Run the whole Rust test suite in a standard user session
cargo test --workspace

# Frontend type check
cd apps/desktop
npx tsc --noEmit

# Start the desktop control center (UI)
cd apps/desktop
pnpm install
pnpm tauri dev
```

### Build the Installable Desktop Application
```bash
cd apps/desktop
npx tauri build
```

This produces the Windows installers (MSI and NSIS) under the Cargo target
directory's `release\bundle\` folder, for example
`C:\cargo-build\release\bundle\`. The standalone executable is
`C:\cargo-build\release\virtualssd-desktop.exe`. Run it directly, or install
the MSI. Data is stored under `%LOCALAPPDATA%\VirtualSSD`.

To run the full release pipeline — version bump, workspace checks, tests,
build, and code signing — use the build script instead:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

### Versioning

Every build gets a unique version. The marketing version is `1.0.0` and the
build number is appended as semver build metadata (`1.0.0+N`), where `N` is
the number of commits on the branch being built. That number is written into:

- the Windows `FILEVERSION`/`PRODUCTVERSION` resource,
- the `FileVersion`/`ProductVersion` strings shown in Windows Explorer,
- the installer filename,
- the application version shown in the sidebar footer,
- the release tag (`v1.0.0+N`).

Because the build number is the commit count, a given source tree always
produces the same version, and every committed change produces a new one.

### Code Signing

Release binaries are Authenticode-signed so Windows does not report an
unknown publisher. The signing step runs automatically inside
`scripts\build.ps1` and resolves the certificate in this order:

1. `VIRTUALSSD_CERT_THUMBPRINT` — the thumbprint of a certificate in
   `Cert:\CurrentUser\My`.
2. `VIRTUALSSD_CERT_SUBJECT` — a substring of the certificate subject.
3. The only code-signing certificate in `Cert:\CurrentUser\My`, if there is
   exactly one.

If no certificate is available the build still completes, but it **reports
that the artifacts are unsigned** rather than silently shipping them.

To create a local certificate for development:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\create-dev-certificate.ps1
```

> **SmartScreen:** a self-signed certificate removes the "unknown publisher"
> prompt on machines that trust it, but it does **not** clear the Microsoft
> SmartScreen "Windows protected your PC" warning for the public. That warning
> is reputation-based and requires a certificate issued by a trusted CA
> (DigiCert, Sectigo, GlobalSign, …) plus real download history. To ship
> publicly without the warning, install a CA-issued code-signing certificate
> into `Cert:\CurrentUser\My` before running the build, and set
> `VIRTUALSSD_CERT_THUMBPRINT` to it.

Signing metadata in the binary:

| Field | Value |
|---|---|
| CompanyName | Chaudary Global |
| ProductName | VirtualSSD |
| FileDescription | VirtualSSD |
| LegalCopyright | Copyright (c) 2026 Ahmad Abdullah Chaudary, Chaudary Global |
| Digest algorithm | SHA-256 |
| Timestamp | RFC 3161 via `VIRTUALSSD_TIMESTAMP_URL` (default `http://timestamp.digicert.com`) |

### Application Icon and System Tray

- The application icon is the same four-square brand mark shown in the sidebar.
  The source of truth is `apps/desktop/app-icon.svg`; regenerate every platform
  format from it with:
  ```bash
  cd apps/desktop
  npx tauri icon app-icon.svg
  ```
  That writes `src-tauri/icons/` (ICO for Windows, ICNS for macOS, PNG sizes
  plus Appx logos). The window, the taskbar entry, the system tray, and both
  installers all use this artwork.
- The app installs a **system tray icon** on startup. Left-clicking it shows or
  hides the main window; right-clicking opens a menu with **Show VirtualSSD**,
  **Diagnostics**, and **Quit VirtualSSD**.
- Closing the main window hides the application to the tray instead of
  terminating it, so mounted volumes and the storage engine keep running. Use
  **Quit VirtualSSD** in the tray menu to exit completely.

> The desktop executable uses `asInvoker` so provider setup and diagnostics work
> without elevation. Mounting, dismounting, and restoring a VHDX requires
> administrator privileges; use **Restart as administrator** in the header or on
> the Settings page. Automated tests run without elevation; the elevated VHDX
> path is a manual verification gate.

---

## License

VirtualSSD is open-source software licensed under the [MIT License](LICENSE).
