# ADR-001: Windows Virtual Block Storage Architecture

- **Status**: Accepted
- **Date**: 2026-10-23
- **Revised**: 2026-11-06
- **Author**: VirtualSSD Systems & Storage Engineering Team
- **Component**: Storage Engine & Windows Device Subsystem

---

## 1. Context and Problem Statement

VirtualSSD must create **actual Windows-visible block devices**, not merely user-mode filesystem mounts or cloud folders.
Windows must be able to discover the device, discover its physical geometry (`IOCTL_DISK_GET_DRIVE_GEOMETRY_EX`), query storage properties (`IOCTL_STORAGE_QUERY_PROPERTY`), initialize the disk with MBR or GPT partition tables, create volumes, format with NTFS, assign drive letters, and issue block-level reads, writes, and flushes (`IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES` for TRIM/UNMAP).

In Windows 11 x64 (Build 22000+ through 26200+ with Secure Boot and Driver Signature Enforcement enabled by default), loading a raw custom kernel-mode storage driver (`storport.sys` miniport or KMDF virtual disk filter) on end-user machines requires:
1. An Extended Validation (EV) Code Signing Certificate.
2. Submission to Microsoft Windows Hardware Compatibility Program (WHCP) for WHQL attestation signing.
3. Test-signing mode if running without WHQL signing, which is strictly prohibited for standard production consumer software and breaks Windows security guarantees.

At the same time, the PRD explicitly demands:
> "Do not attempt to bypass Windows driver-signing or security protections."
> "WinFsp alone is not a block-device implementation. Do not present a filesystem mount as a completed block-device feature."
> "Choose a technically defensible architecture and document why it was selected."

---

## 2. Decision: Windows VHDX + Snapshot Chunk Mirroring

The implemented path avoids custom kernel drivers by using Microsoft's signed Virtual
Disk API, while keeping the 4 MiB chunk engine and provider adapters independent.

### Tier 1: Windows Kernel Virtual Storage Subsystem (`virtdisk.dll` + `storvsp.sys`)
- Modern Windows operating systems incorporate the Microsoft Virtual Storage Provider Bus (`storvsp.sys` / `disk.sys`).
- VirtualSSD implements a desktop backend (`apps/desktop/src-tauri`) and a storage engine crate (`crates/storage-core`) with 4 MiB chunk containers.
- The backend interfaces with the Windows Virtual Disk API (`virtdisk.dll` / `VIRTUAL_STORAGE_TYPE_DEVICE_VHDX`) to attach containers directly into the Windows kernel storage stack as `\\.\PhysicalDriveN`.
- This device appears in Windows Device Manager under `Disk drives`, in Windows Disk Management (`diskmgmt.msc`), and in `diskpart`.
- Windows partitions, formats with NTFS, mounts drive letters (e.g. `V:`), and handles standard kernel-mode I/O.
- **Cloud mirroring model**: Windows I/O is not intercepted in the kernel. Instead, the
  user triggers a mirror and the backend reads the VHDX file, splits it into
  4 MiB chunks, skips all-zero regions, uploads changed chunks under a
  dedicated VHDX namespace, records them in `vhdx_chunks`, and publishes the
  manifest last. Restore downloads every referenced chunk into
  `disk.vhdx.restore.tmp`, verifies its checksum, and only then renames it to
  `disk.vhdx`. A missing local VHDX is restored automatically on mount.
- **Mirroring a mounted disk**: `crates/storage-core/src/vss.rs` takes a
  Volume Shadow Copy of the volume that holds the VHDX, using the
  `Win32_ShadowCopy` WMI class. The guest file system is flushed into the VHDX
  with `FSCTL_FLUSH_VOLUME` first, and the VSS service flushes the file system
  cache before it images the volume. The mirror then reads the shadow copy, so
  the volume stays mounted and usable for the whole transfer. The shadow copy
  is always released, so its differential area does not linger.
  The shadow-copy path requires administrator privileges; the unmounted mirror
  remains the fallback and is unchanged.
- **Remaining limitation**: mirroring is still on demand. There is no
  background auto-mirror, so a change is uploaded when the user asks rather
  than the instant it is written. A timer-driven mirror using the same
  shadow-copy path is the next step, and needs no kernel component.

### Tier 2: Quarantined Storport Prototype (`archive/windows-virtual-disk`)
- An early Storport miniport prototype is retained only under `archive/` for
  historical reference.
- It is **not a workspace member**, is not built, signed, or installed by the
  release scripts, and its READ/WRITE path did not transfer data.
- Do not deploy it. It is excluded from releases because an incomplete miniport
  would silently discard writes.

---

## 3. Storage Engine & Chunk Specifications

1. **Chunk Granularity**: Fixed 4 MiB chunks (`4,194,304` bytes) in this build. The manifest records the chunk size, but there is no UI or configuration file for changing it.
2. **Addressing**: 64-bit Logical Block Addressing (LBA) with standard 512-byte logical sectors and 4096-byte physical sectors (Advanced Format 512e/4Kn).
3. **Sparse Allocation**: Unwritten logical sectors consume zero bytes locally and remotely. Reads of unallocated chunks return deterministic zeros without network requests. The VHDX mirror omits all-zero 4 MiB regions and deletes previously mirrored objects for regions that become zero.
4. **Integrity**: BLAKE3-256 cryptographic hashing per chunk. Encrypted chunks additionally record a `remote_checksum` over the ciphertext so a provider download can be verified before decryption.
5. **Encryption**: Optional per disk and fixed at creation. AES-256-GCM with a random 256-bit key stored as a DPAPI-protected `encryption.key`; the recovery key is the 64-character hex form. A missing key fails mount and sync instead of regenerating a key that would make remote data unreadable.
6. **Journaling**: Append-only Write-Ahead Journal (WAL) with fsync on commit and idempotent replay when a disk engine is opened. This is the block-engine API path; mounted Windows VHDX I/O does not pass through it. Engine dirty-chunk upload exists as a storage-core call but is not exposed by the desktop UI, which publishes VHDX snapshots manually.

### 3.1 Observability: Backend Engine Log Stream

The desktop backend installs a `tracing` subscriber at startup. A custom
`tracing_subscriber::Layer` mirrors every filtered event into a bounded
in-memory ring buffer (500 entries, newest-first) and, once the Tauri
application is running, emits each entry to the frontend as an `engine-log`
event. The `get_engine_logs` command returns the same buffer; `clear_engine_logs`
empties it and records the clear action itself.

- The Diagnostics page renders exactly these entries. The frontend never
  synthesizes log lines, and the browser preview shows an explicit
  "backend log stream unavailable" state.
- Targets are shortened for display (`virtualssd_storage_core::crypto` becomes
  `storage-core::crypto`).
- The filter defaults to `info` with noisy HTTP/async runtimes muted, and can be
  overridden with `RUST_LOG`.
- This is an application-level event stream. It is not a kernel trace probe and
  does not capture Windows I/O.

---

## 4. Consequences and Verification

- **Positive**: Compatibility with Windows 11 without test-signing or a custom kernel driver. Windows can partition, format, and mount the attached VHDX.
- **Automated verification**: `cargo test --workspace` runs the storage-core, provider, security, settings, and other unit suites in a standard user session. The desktop manifest is `asInvoker`, so its tests no longer require elevation.
- **Frontend verification**: `npx tsc --noEmit` in `apps/desktop`.
- **Manual verification required**: VHDX attach/detach, NTFS format/mount, and end-to-end Google Drive/Telegram mirroring and restore require an elevated session and real provider credentials.
