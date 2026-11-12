# VirtualSSD Production Release Process

- **Revision**: 3.0 (2026-11-06)

---

## 1. Release Gates & Verification

Every production release candidate must satisfy the checks below before distribution.

1. **Gate 1 — Core Storage Tests**:
   - `cargo check --workspace --all-targets` succeeds.
   - `cargo test --workspace` passes with 0 failures. The desktop manifest is
     `asInvoker`, so its settings tests run without elevation.
   - BLAKE3 checksum, journal, cache, manifest, storage-core, crypto, and settings
     tests pass.
   - The remaining elevated work (VHDX attach/detach, restore, real-provider
     transfer) is a manual gate, not a substitute for the automated suites.

2. **Gate 2 — Windows Block Device Conformance**:
   - VHDX attach/detach succeeds through the Windows Virtual Disk API.
   - Partition initialized with GPT, formatted with NTFS, drive letter mounted.
   - NTFS file creation, read, write, rename, and deletion verified.
   - Mount/dismount is attempted only after the runtime elevation check passes.

3. **Gate 3 — Provider & Snapshot Conformance**:
   - Google Drive: OAuth PKCE, resumable chunk upload, persisted session resume,
     named manifest publish/fetch, quota parsing, and 403 quota classification verified.
   - Telegram: Bot API `sendDocument` chunk upload, checksum dedupe, superseded
     message cleanup, 50 MiB limit handling, pinned catalog discovery, and legacy
     pinned manifest fallback verified.
   - Create an unmounted disk, mirror its VHDX snapshot, delete the local copy,
     import the remote manifest, restore the VHDX on mount, and verify the file.
   - Encrypted disk: export the recovery key, import it on a second installation,
     and confirm the restored VHDX matches before trusting the backup.
   - Remote deletion: verify that chunks and the manifest disappear from the
     provider and that a failed delete leaves the local disk retryable.
   - Provider tests: `cargo test -p virtualssd-provider-gdrive -p virtualssd-provider-telegram`.

4. **Gate 4 — UI & Security**:
   - `npx tsc --noEmit` succeeds in `apps/desktop`.
   - Light-only interface renders without runtime console errors.
   - DPAPI provider credential round-trip test passes.
   - `providers.json` contains no secrets; credential files are unreadable as plaintext.
   - Provider Debug output and error strings redact tokens.
   - A damaged `settings.json` is preserved as `settings.json.corrupt-<timestamp>`
     and the warning is visible in the UI.
   - Remote deletion is attempted only after an explicit user choice in the delete
     dialog and only for an unmounted disk.
   - The Diagnostics page shows real backend `tracing` entries delivered over the
     `engine-log` event, and the browser preview shows an explicit
     "backend log stream unavailable" state rather than placeholder entries.

---

## 2. Release Build Commands

The full release pipeline is one script. It bumps the version, runs every
check, builds the application and installers, signs them, and assembles the
release artifacts:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

The individual steps it performs are:

```powershell
# 1. Resolve the per-build version (1.0.0+N, N = commit count)
powershell -ExecutionPolicy Bypass -File .\scripts\version.ps1

# 2. Compile all targets
cargo check --workspace --all-targets

# 3. Run every test suite (desktop included; non-elevated)
cargo test --workspace

# 4. Frontend type check
cd apps/desktop
npx tsc --noEmit

# 5. Build the Tauri desktop application and its installers
npx tauri build
```

Output locations:

- The compiled desktop executable: `<cargo-target-dir>\release\virtualssd-desktop.exe`
  (this workspace redirects Cargo's target directory to `C:\cargo-build`; the
  build script resolves the real path instead of assuming `./target`).
- The Windows installers (MSI and NSIS): `<cargo-target-dir>\release\bundle\`.
- The packaged copies: `dist/VirtualSSD.exe`, `dist/VirtualSSD-setup.exe`,
  `dist/VirtualSSD.msi`, and `dist/VERSION.txt`.

No Windows service or kernel driver is packaged. The prototypes under `archive/`
are excluded from the workspace and the release.

### Code signing

`scripts\build.ps1` signs the executable and both installers with an
Authenticode signature (SHA-256 digest, RFC 3161 timestamp).

The certificate is resolved in this order:

1. `VIRTUALSSD_CERT_THUMBPRINT` — a certificate in `Cert:\CurrentUser\My`.
2. `VIRTUALSSD_CERT_SUBJECT` — a substring of the certificate subject.
3. The only code-signing certificate in `Cert:\CurrentUser\My`, if there is
   exactly one. If there are several, the script lists them and refuses to
   guess.

If no certificate is found, the build completes but reports **UNSIGNED** and
exits with the artifacts clearly marked as such in `dist/VERSION.txt`. It never
silently ships unsigned binaries.

Before a public release:

```powershell
# One-time: create a local development certificate
powershell -ExecutionPolicy Bypass -File .\scripts\create-dev-certificate.ps1

# Or point the build at a CA-issued certificate
$env:VIRTUALSSD_CERT_THUMBPRINT = "<thumbprint>"
$env:VIRTUALSSD_TIMESTAMP_URL   = "http://timestamp.digicert.com"
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

**SmartScreen.** A self-signed certificate removes the "unknown publisher"
prompt on machines that trust it, but it does not clear the SmartScreen
"Windows protected your PC" warning for the public. That warning is
reputation-based and needs a certificate issued by a trusted CA plus real
download history. Verify the signature before publishing:

```powershell
Get-AuthenticodeSignature .\dist\VirtualSSD.exe | Format-List *
```

### Version metadata

The build number changes with every build and is written into the Windows
`FILEVERSION`/`PRODUCTVERSION` resource, the `FileVersion`/`ProductVersion`
strings, the installer filename, the sidebar footer, and the release tag.
`CompanyName` is `Chaudary Global` and `LegalCopyright` is
`Copyright (c) 2026 Ahmad Abdullah Chaudary, Chaudary Global`.

### Icon and tray checks

- `apps/desktop/app-icon.svg` is the single source of truth for the application
  artwork. After changing it, regenerate the platform formats and rebuild:
  ```bash
  cd apps/desktop
  npx tauri icon app-icon.svg
  npx tauri build
  ```
  The generated files live in `apps/desktop/src-tauri/icons/` and must be
  committed alongside the SVG.
- Confirm the shipped artifacts actually carry the artwork:
  - The window and taskbar show the mark when the app runs.
  - The system tray shows the mark; left-click toggles the window and
    right-click opens **Show VirtualSSD / Diagnostics / Quit VirtualSSD**.
  - The MSI and NSIS installers show the mark in Windows Explorer and in
    **Apps & features**.

---

## 3. Known Limitations for Release Notes

- VHDX mirroring is a point-in-time snapshot and requires the disk to be
  unmounted. Windows I/O is not intercepted, so there is no live write-through.
- Client-side encryption is fixed per disk at creation. Existing disks cannot be
  converted without re-uploading the snapshot.
- Telegram Bot API does not expose account storage quota, so the UI reports
  “Not exposed by Bot API” and stores `0`.
- Background auto-sync, scheduled scrubbing, automatic mount on startup, and
  parallel upload tuning are not implemented; sync and audits are manual.
- A kernel-mode virtual disk driver and a Windows service are not shipped. VHDX
  attachment uses the Windows Virtual Disk API and requires administrator
  privileges.
- Integrity audits verify chunks that are present locally. Remote chunks are not
  downloaded during an audit; they are verified when restored.
