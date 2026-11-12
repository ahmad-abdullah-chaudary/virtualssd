<div align="center">
  <img src="logo.png" alt="VirtualSSD logo" width="128" height="128" />
</div>

<div align="center">

# VirtualSSD

### Your cloud account, mounted as a real Windows disk.

[![Windows 11](https://img.shields.io/badge/Windows-11%20x64-0078D6.svg?logo=windows)](https://microsoft.com)
[![Rust 1.82+](https://img.shields.io/badge/Rust-1.82+-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Tauri 2.0](https://img.shields.io/badge/Tauri-2.0-blue.svg?logo=tauri)](https://tauri.app)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Releases](https://img.shields.io/github/v/release/ahmad-abdullah-chaudary/virtualssd.svg)](https://github.com/ahmad-abdullah-chaudary/virtualssd/releases)

</div>

**VirtualSSD turns the storage you already pay for into a Windows block device.**

You point it at your Google Drive or your Telegram storage chat. It creates a
real VHDX volume that Windows mounts as `V:\`, formats as NTFS, and treats
like any other drive. Unmount it and the volume is mirrored into your own
cloud account, chunk by chunk, encrypted if you want it. Mount it on another
machine and the exact same volume comes back.

It is a **sidecar SSD that lives in the cloud**: local when you use it, and
backed by storage you already own, with no separate backup appliance, no
syncing client, and no second subscription.

<div align="center">

**⭐ If VirtualSSD is useful to you, please [star this repository](https://github.com/ahmad-abdullah-chaudary/virtualssd/stargazers). ⭐**

*It is the fastest way to help an open-source storage project reach the people
who need it.*

</div>

<div align="center">

**Company** Chaudary Global &nbsp;·&nbsp;
**Developer** Ahmad Abdullah Chaudary &nbsp;·&nbsp;
**Country** PK &nbsp;·&nbsp;
**Version** 1.0.0 (the build number changes with every build) &nbsp;·&nbsp;
**License** MIT &nbsp;·&nbsp;
[**Releases**](https://github.com/ahmad-abdullah-chaudary/virtualssd/releases) &nbsp;·&nbsp;
[**Issues**](https://github.com/ahmad-abdullah-chaudary/virtualssd/issues)

</div>


---

## Table of contents

- [What problem this solves](#what-problem-this-solves)
- [What you can actually do with it](#what-you-can-actually-do-with-it)
- [How it works](#how-it-works)
- [How fast is it](#how-fast-is-it)
- [Security and privacy](#security-and-privacy)
- [Install](#install)
- [Your first disk, in five minutes](#your-first-disk-in-five-minutes)
- [Repository map](#repository-map)
- [Build from source](#build-from-source)
- [Versioning and code signing](#versioning-and-code-signing)
- [Troubleshooting](#troubleshooting)
- [Google Drive setup](docs/google-drive-setup.md)
- [Known limitations](#known-limitations)
- [Contributing](#contributing)
- [Roadmap](#roadmap)
- [License](#license)

---

## What problem this solves

Most "cloud storage" products are folders you browse. A few are sync clients
that mirror a local folder. None of them give you a **block device** that
Windows treats as a physical disk, that you can format, partition, run a
database on, or attach a scanner's image to.

VirtualSSD sits between the two:

- **It is a real disk.** Windows Disk Management sees it. `diskpart` sees it.
  Device Manager sees it. It gets a drive letter and it holds a real
  filesystem.
- **It is backed by storage you own.** The bytes live in your own Google Drive
  or your own Telegram storage chat. There is no VirtualSSD data centre, no
  subscription, and no lock-in — you can pull the chunks down with a script
  and read the manifest yourself.

If you have 100 GB of unused Google Drive space and you want a scratch disk
for a build, a database, or a video project — that is what this is.

## What you can actually do with it

- **A portable scratch disk.** Create a volume, mount it, install your tool
  chain, unmount, and the whole thing is mirrored to your Drive. Pick it up
  on the laptop. The files are already there.
- **A disk that stays usable while it backs up.** *Mirror While Mounted*
  takes a Volume Shadow Copy of the volume and mirrors that image, so the
  volume keeps working and stays mounted for the whole transfer.
- **A private encrypted volume.** Per-disk AES-256-GCM with a random 256-bit
  key protected by Windows DPAPI. Export a 64-character recovery key and
  import it on another machine. The provider only ever sees ciphertext.
- **An archive that is still a disk.** Format it NTFS, put whatever you
  like in it. It stays mountable on any Windows machine with the recovery
  key — no special client, no proprietary container format.
- **Offsite copies of throwaway VMs and build caches.** Machines get rebuilt;
  the disk does not have to.
- **A second drive letter for a project.** No repartitioning the system disk,
  no separate hardware, no second cloud subscription.
- **Recoverable test data.** Every chunk is BLAKE3-verified on the way in and
  on the way out. A corrupted remote object is detected, not silently
  restored.

What it is **not**: it is not a live write-through cloud disk yet. Writes go
into the local VHDX, and the mirror happens when you ask for it. See
[Known limitations](#known-limitations) — and the
[performance and robustness analysis](docs/performance-and-robustness.md) for
exactly what is implemented and what is next.

## How it works

```
   ┌─────────────────────────────── Windows ───────────────────────────────┐
   │                                                                        │
   │   Explorer, SQL Server, a game, a compiler                           │
   │            │                                                          │
   │            ▼                                                          │
   │   ┌────────────────────┐                                              │
   │   │  V:\  (NTFS volume)│   ← a real VHDX, a real block device        │
   │   └─────────┬──────────┘                                              │
   │             │                                                         │
   │             ▼                                                         │
   │   ┌─────────────────────────────────────────────┐                     │
   │   │  virtdisk.dll  (Windows Virtual Disk API)   │  ← Microsoft's      │
   │   └─────────┬───────────────────────────────────┘    signed stack     │
   │             │                                                         │
   └─────────────┼──────────────────────────────────────────────────────────┘
                 │  disk.vhdx  (4 MiB chunks, BLAKE3, AES-256-GCM)
                 ▼
   ┌─────────────────────────── VirtualSSD ──────────────────────────────┐
   │   storage-core: chunk the file → hash → encrypt → upload → manifest   │
   └──────────┬───────────────────────────────────┬───────────────────────┘
              │                                   │
              ▼                                   ▼
      ┌───────────────┐                  ┌──────────────────┐
      │ Google Drive  │                  │  Telegram Bot    │
      │  REST API v3  │                  │  API storage chat│
      │  resumable    │                  │  checksum dedupe │
      └───────────────┘                  └──────────────────┘
```

**The mirror is a snapshot, and it is honest about that.** When you press
**Mirror Snapshot**, VirtualSSD reads the VHDX file, splits it into 4 MiB
chunks, hashes each one with BLAKE3, encrypts it if the disk is encrypted,
and uploads it to your provider under a dedicated namespace. All zero regions
are skipped entirely — a sparse volume does not cost storage for the holes.
The manifest is published last, so a published manifest always describes a
complete snapshot.

**A mounted disk does not have to be unmounted.** *Mirror While Mounted*
takes a **Volume Shadow Copy** of the volume that holds the VHDX and mirrors
that image, so the volume stays mounted and usable for the whole transfer.
The guest file system is flushed into the VHDX first with
`FSCTL_FLUSH_VOLUME`, and the VSS service flushes the file system cache
before it images the volume, so the copy is a faithful point-in-time image.
This is a documented Windows API — no kernel driver, no EV certificate, no
WHQL attestation.

**Restore is verified, not trusted.** On a fresh machine you import the
manifest by disk UUID, VirtualSSD rebinds it to your local provider, and the
VHDX is rebuilt into `disk.vhdx.restore.tmp`. Every chunk is downloaded,
decrypted if needed, and checksum-verified. Only when all of them pass is the
file renamed into place. A partial restore is deleted, never left behind.

**The provider is a peer, not a master.** Google Drive and Telegram are
equally first-class. Google Drive gets protocol-correct resumable uploads
with a persisted session, so an interrupted chunk continues from the byte
offset Drive already has. Telegram gets checksum dedupe, so identical content
is never sent twice.

## How fast is it

We will not publish a benchmark we have not run. Here is exactly what the
implementation does, and what bounds the result.

| Property | Value | Where it comes from |
|---|---|---|
| Chunk size | 4 MiB | `crates/manifest` geometry; fixed in this build |
| Concurrent transfers | 4 in flight | `transfer::DEFAULT_UPLOAD_CONCURRENCY` |
| Upload / download overlap | Yes — the file is read and hashed while earlier chunks are still in flight | `crates/storage-core/src/transfer.rs` |
| Resumable uploads | Yes — a chunk continues from the offset the provider already holds | Google Drive session state |
| Duplicate suppression | Yes — an unchanged chunk is never re-uploaded | Manifest checksum comparison |
| Sparse regions | Never uploaded, and deleted remotely when they appear | Zero-region detection |
| Manifest writes | One checkpoint per 64 chunks, not one per chunk | `transfer::MANIFEST_CHECKPOINT_INTERVAL` |
| Integrity | BLAKE3-256 per chunk, plus a ciphertext checksum for encrypted chunks | `crates/integrity` |

**What actually limits throughput is your uplink and the provider's API.** For
a first mirror the whole file has to cross the network once. For a later
mirror only the changed chunks cross it, so the cost tracks the size of the
*change*, not the size of the disk. That is the property that matters day to
day: re-mirroring a 100 GB volume that changed by 200 MB moves about 200 MB.

Three honest caveats:

- **The first mirror is a full upload.** Nothing can avoid that.
- **Telegram caps a single document at 50 MiB.** A 4 MiB chunk is well
  inside it, but a very slow link on the Telegram Bot API will still be
  slower than Google Drive's.
- **Every mirror re-reads and re-hashes the whole VHDX** to find out which
  chunks changed. Reading the file is cheap; it is the *upload* that costs.
  A future change parses the VHDX Block Allocation Table so unallocated
  regions are never read at all.

## Security and privacy

- **Your provider credentials never leave your machine in plain text.** They
  are encrypted with Windows DPAPI for the current user and stored under
  `%LOCALAPPDATA%\VirtualSSD\providers\`. The provider index contains no
  secrets, and credential `Debug` output is redacted and zeroized on drop.
- **Chunk payloads can be encrypted.** AES-256-GCM with a random per-disk
  key. The provider stores ciphertext; the plaintext checksum is never
  published. A lost key means lost data — the app refuses to pretend
  otherwise.
- **Every chunk is verified.** BLAKE3-256 on the way up and on the way down.
  A remote object that does not match is a failed restore, not a silent
  corruption.
- **Recovery keys are never logged.** The Diagnostics page records that a key
  was exported or imported, never its value.
- **The application is Authenticode-signed**, so Windows shows a real
  publisher instead of "unknown publisher".
- **VirtualSSD has no servers.** There is no account, no telemetry endpoint,
  and no phone-home. Your data goes to your provider and nowhere else.

See [SECURITY.md](SECURITY.md) for the full threat model and
[docs/threat-model.md](docs/threat-model.md) for the analysis.

## Install

**Windows 11 x64 or Windows 10 x64 (build 19044+).** The Microsoft Edge
WebView2 Runtime is preinstalled on current Windows.

Grab the latest release:

| Asset | Use it when |
|---|---|
| [`VirtualSSD-setup.exe`](https://github.com/ahmad-abdullah-chaudary/virtualssd/releases/latest) | You want the installer. It registers the Start Menu shortcut and the uninstaller. |
| [`VirtualSSD.msi`](https://github.com/ahmad-abdullah-chaudary/virtualssd/releases/latest) | You are deploying to managed or enterprise machines. |
| [`VirtualSSD-portable.zip`](https://github.com/ahmad-abdullah-chaudary/virtualssd/releases/latest) | You want a portable copy with no installation. Unpack it and run `VirtualSSD.exe`. |
| `VirtualSSD.exe` | The bare executable. It **needs `WebView2Loader.dll` next to it** — use the portable zip instead of downloading this on its own. |

> **"WebView2Loader.dll was not found"** — the application links the WebView2
> loader dynamically, so Windows looks for that DLL next to the executable. The
> installers include it automatically. If you copied `VirtualSSD.exe` on its
> own, copy `WebView2Loader.dll` into the same folder, or use
> `VirtualSSD-portable.zip`. The DLL is also in this repository's `dist`
> folder next to the portable executable.

Check the signature before you run it:

```powershell
Get-AuthenticodeSignature .\VirtualSSD-setup.exe | Format-List Status, SignerCertificate
```

> **About SmartScreen.** Windows may show "Windows protected your PC" the
> first time you run a newly published binary. That warning is
> reputation-based, not a tamper warning: click **More info → Run anyway**
> after confirming the signature above. It disappears on its own once Windows
> has seen enough real downloads.

## Your first disk, in five minutes

1. **Launch VirtualSSD.** It starts unelevated, so you can add a provider
   and read the diagnostics without administrator rights.
2. **Open Cloud Providers** and add one:
   - **Google Drive** — your OAuth 2.0 client id and refresh token. Follow
     [the Google Drive setup guide](docs/google-drive-setup.md); the helper
     script `scripts\get-google-drive-token.ps1` obtains both for you.
   - **Telegram** — your bot token and the numeric storage chat id. Use a
     **channel** where the bot is an administrator, so disks can be
     discovered from another machine automatically.
3. **Open Disks & Volumes → New Disk.** Name it, pick a capacity, choose the
   provider. The manifest is published immediately, so the disk is
   discoverable from another machine right away.
4. **Restart as administrator** (button in the header or on the Settings
   page). Attaching a VHDX is a privileged Windows operation.
5. **Mount.** VirtualSSD creates the VHDX, attaches it, initializes GPT,
   formats NTFS, and assigns a drive letter.
6. **Fill it up**, then unmount and press **Mirror Snapshot**. Watch the live
   engine log on the Diagnostics page to see the real chunk transfers. If the
   volume is still mounted, use **Mirror While Mounted** instead: it takes a
   volume shadow copy and mirrors that, so the volume stays usable.
7. **Go to another machine**: install VirtualSSD, add the same provider,
   open **Cloud Providers → Import a Remote Disk Manifest**, and enter the
   disk UUID. The volume comes back on first mount.

Data lives under `%LOCALAPPDATA%\VirtualSSD`.

## Repository map

```
virtualssd/
  apps/
    desktop/               # Tauri + React + TypeScript + Tailwind (light theme)
      app-icon.svg         # The brand mark — the source for every icon format
      src-tauri/
        src/engine_log.rs  # Bounded ring buffer + tracing layer
        src/tray.rs        # System tray icon and menu
        capabilities/      # The ACL the frontend is allowed to use
  crates/
    storage-core/          # Block engine, VHDX mirror/restore, encryption, WAL
      src/transfer.rs      # Concurrent producer/consumer transfer pipeline
    block-map/             # LBA to chunk indexing and sparse extent tracking
    manifest/              # Versioned disk manifest and commit records
    journal/               # Write-ahead log with crash-recovery replay
    cache-manager/         # Bounded local cache with dirty tracking
    integrity/             # BLAKE3 and SHA-256 verifiers
    provider-contract/     # Unified cloud backend traits and lifecycle
    provider-gdrive/       # Google Drive REST v3 adapter, OAuth 2.0 PKCE
    provider-telegram/     # Telegram Bot API adapter
    security/              # Windows DPAPI secret protection, path sanitization
  scripts/                 # Version, build, signing, and release scripts
  docs/                    # Architecture, performance, recovery, release, security
  archive/                 # Quarantined prototypes — not built, not shipped
```

## Build from source

```bash
# Compile every workspace target (including the desktop backend)
cargo check --workspace --all-targets

# Run the whole Rust test suite in a standard user session
cargo test --workspace

# Frontend type check
cd apps/desktop && npx tsc --noEmit

# Start the desktop control center
pnpm install && pnpm tauri dev
```

The full release pipeline — version resolution, checks, tests, build, signing,
and packaging — is one script:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

### Versioning

Every build gets a unique version. The marketing version is `1.0.0` and the
build number is appended as semver build metadata (`1.0.0+N`), where `N` is
the number of commits on the branch being built. The same number appears in:

- the Windows `FILEVERSION`/`PRODUCTVERSION` resource,
- the `FileVersion`/`ProductVersion` strings in Windows Explorer,
- the installer filename,
- the application footer in the sidebar,
- the release tag (`v1.0.0+N`).

Because the build number is the commit count, a given source tree always
produces the same version, and every committed change produces a new one.

### Code signing

Release binaries are Authenticode-signed (SHA-256, RFC 3161 timestamped) so
Windows reports a real publisher. `scripts\build.ps1` resolves the
certificate in this order:

1. `VIRTUALSSD_CERT_THUMBPRINT` — a certificate in `Cert:\CurrentUser\My`.
2. `VIRTUALSSD_CERT_SUBJECT` — a substring of the certificate subject.
3. The only code-signing certificate in `Cert:\CurrentUser\My`.

If no certificate is available the build still completes, but it reports
**UNSIGNED** rather than silently shipping unsigned binaries.

```powershell
# One-time: create a local development certificate
powershell -ExecutionPolicy Bypass -File .\scripts\create-dev-certificate.ps1
```

> A self-signed certificate removes the "unknown publisher" prompt on
> machines that trust it, but it does **not** clear the SmartScreen warning
> for the public. That needs a certificate issued by a trusted CA — DigiCert,
> Sectigo, or GlobalSign — plus real download history.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| **Access is denied when mounting** | Attaching a VHDX needs administrator rights. Use **Restart as administrator** in the header or on the Settings page. |
| **The disk shows as Offline in Disk Management** | Windows storage policy set the new disk offline. In an elevated PowerShell: `Get-Disk \| Where-Object FriendlyName -like "*VirtualSSD*" \| Set-Disk -IsOffline $false -IsReadOnly $false` |
| **Google Drive rate limit (403)** | Too many requests in a short window. The error is classified as `RateLimited`, not as a generic failure. |
| **Telegram says "not exposed by Bot API" for quota** | The Bot API genuinely does not expose account storage quota. The UI reports that instead of inventing a number. |
| **"Windows protected your PC"** | SmartScreen reputation, not tampering. Verify the signature, then **More info → Run anyway**. |
| **The app closed but it is still running** | Closing the window hides it to the system tray. Use **Quit VirtualSSD** in the tray menu to exit. |
| **A disk was skipped at startup** | Its manifest could not be read. The reason is in the live log on the Diagnostics page. |

More in [docs/troubleshooting.md](docs/troubleshooting.md).

## Known limitations

- **Mirroring is on demand, not continuous.** A change is mirrored when you
  ask for it, not the instant it is written. **The disk does not have to be
  unmounted**: *Mirror While Mounted* takes a Volume Shadow Copy of the volume
  and mirrors that image while the volume stays usable. A background timer
  that mirrors automatically is not implemented yet.
- **Encryption is fixed at disk creation.** Converting an existing
  unencrypted disk means re-uploading a new snapshot.
- **No background auto-sync.** Sync and audits are manual.
- **Integrity audits verify locally present chunks.** Remote chunks are
  verified during restore, not downloaded for an audit.
- **Telegram Bot API cannot report account storage quota.** The UI says
  "Not exposed by Bot API" and stores `0`.
- **No kernel-mode virtual disk driver or Windows service is shipped.** VHDX
  attachment uses the Windows Virtual Disk API and requires administrator
  privileges. The prototypes under `archive/` are quarantined and excluded
  from the workspace and the release.
- **A damaged manifest is recovered from `manifest.json.prev`** when
  possible. If both files are unreadable the disk is logged and skipped
  rather than guessed at.

The full picture, including what was measured and what is planned, is in
[docs/performance-and-robustness.md](docs/performance-and-robustness.md).

## Contributing

Contributions are welcome — code, documentation, provider adapters, and
especially **real-world reports from users with live Google Drive or Telegram
accounts**, which is the one thing we cannot generate ourselves.

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. It covers the development
setup, the checks every change must pass, the commit conventions, and how to
open a pull request.

## Roadmap

- [x] Real Windows block device through the Virtual Disk API
- [x] VHDX snapshot mirroring to Google Drive and Telegram
- [x] Client-side AES-256-GCM encryption with DPAPI-protected keys
- [x] Verified restore, remote import, and remote deletion
- [x] Concurrent transfer pipeline with bounded concurrency
- [x] **Volume Shadow Copy mirroring** so a mounted disk can be mirrored without unmounting
- [x] Live backend engine log stream
- [x] Branded application icon, system tray, and Authenticode signing
- [ ] **Background auto-mirror** on a timer, using the same shadow-copy path
- [ ] **Cross-process disk locking** so two instances cannot open one disk
- [ ] **VHDX Block Allocation Table parsing** to skip unallocated regions
- [ ] **Remote integrity scrubbing** of a sampled subset of chunks
- [ ] **S3 / OneDrive / Backblaze** provider adapters

## License

MIT — see [LICENSE](LICENSE).

Copyright © 2026 Ahmad Abdullah Chaudary, Chaudary Global.

---

<div align="center">

### ⭐ Star this repository

VirtualSSD is a lot of work for a small project. If it saved you disk space,
helped you recover a volume, or just interested you, a star is the single
most useful thing you can do for it.

**[⭐ Star VirtualSSD on GitHub](https://github.com/ahmad-abdullah-chaudary/virtualssd/stargazers)**
&nbsp;&nbsp;·&nbsp;&nbsp;
**[🐛 Report an issue](https://github.com/ahmad-abdullah-chaudary/virtualssd/issues/new)**
&nbsp;&nbsp;·&nbsp;&nbsp;
**[🍴 Contribute](CONTRIBUTING.md)**

Built with Rust, Tauri, React, and the Windows Virtual Disk API.

</div>
