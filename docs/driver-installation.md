# VirtualSSD Windows Storage Setup

- **Status**: Implemented path
- **Revision**: 3.0 (2026-11-06)

---

## 1. Implemented Storage Path

VirtualSSD uses the documented Windows Virtual Disk API (`virtdisk.dll`) with a
dynamic VHDX container. It does **not** require a custom kernel driver, driver
test-signing, or disabling Secure Boot.

1. The desktop backend creates `disk.vhdx` with `CreateVirtualDisk`.
2. It attaches the image with `AttachVirtualDisk` and reads the real device path
   returned by `GetVirtualDiskPhysicalPath` (`\\.\PhysicalDriveN`).
3. Windows exposes the volume in Device Manager, Disk Management
   (`diskmgmt.msc`), and `diskpart`.
4. You can initialize it with GPT/MBR, create a partition, format it as NTFS, and
   assign a drive letter (for example `V:`) using Disk Management or
   `diskpart`.
5. Detach uses `DetachVirtualDisk`.

### Elevation model

The desktop executable declares `asInvoker` in `app.manifest`. It starts without
elevation so provider setup, settings, diagnostics, recovery-key management, and
remote manifest import work for a standard user. Attaching, detaching, and
restoring a VHDX require administrator privileges, so the backend checks
elevation at runtime and returns a clear error when it is missing. Use the
**Restart as administrator** button in the header or on the Settings page to
relaunch through UAC.

The VHDX file itself is still an ordinary local file while Windows owns the
mount. Client-side encryption protects mirrored chunk payloads; it does **not**
encrypt the live local `disk.vhdx`.

---

## 2. Snapshot Mirroring and Restore

Cloud replication is snapshot-based and explicit:

1. Unmount the disk in VirtualSSD.
2. Select **Mirror Snapshot** on the Disks page (or **Sync** elsewhere).
3. The backend reads `disk.vhdx`, splits it into 4 MiB chunks, and uploads
   changed non-zero chunks under the disk's VHDX namespace
   (`UUID v5` of `"{disk_id}:vhdx-file-v1"`). All-zero regions are omitted and
   their previously uploaded objects are deleted.
4. The manifest records each chunk's plaintext checksum, optional ciphertext
   checksum, length, remote object id, and the total snapshot size. The manifest
   is published only after every chunk update succeeds.
5. On mount, if `disk.vhdx` is missing and the manifest contains a snapshot, the
   backend downloads every chunk to `disk.vhdx.restore.tmp`, verifies each
   checksum (and decrypts if needed), and atomically renames the file. A partial
   restore is deleted.

Mirroring is point-in-time. Windows I/O is not intercepted, so changes made while
the disk is mounted are captured only by the next unmounted sync.

---

## 3. Encryption and Recovery Keys

Encryption is opted in per disk at creation (the default comes from Settings) and
is fixed for the life of the disk.

- Cipher: AES-256-GCM. Format: `version(1) || nonce(12) || ciphertext+tag`.
- Key: random 256-bit value stored in `disk_dir/encryption.key`, protected with
  Windows DPAPI in the current-user scope.
- Recovery key: 64 hexadecimal characters, exportable from the Disks page.
- The key is never uploaded. Losing every copy of it makes encrypted remote
  chunks unrecoverable.
- A missing key blocks mount and sync with an explicit error. VirtualSSD does not
  silently generate a replacement key, because that would make existing remote
  data undecryptable.

---

## 4. Quarantined Prototypes

`archive/windows-virtual-disk` contains an early Storport miniport prototype and
`archive/storage-service` contains an early service scaffold. Neither is a
workspace member. Neither is built, shipped, or supported. Do not register the
service with `sc.exe` and do not install the driver; the miniport's I/O path is
incomplete and the service has no IPC listener.

---

## 5. Requirements

- **OS**: Windows 11 x64 or Windows 10 x64 (Build 19044+).
- **Permissions**: Administrator for VHDX attach/detach/restore.
- **Subsystem**: `virtdisk.dll` (present in standard Windows 10/11 installations).

---

## 6. Manual Formatting After Attach

After VirtualSSD attaches a new VHDX and assigns a drive letter, Windows may show
the volume as uninitialized. To make it usable:

```powershell
# Run from an Administrator PowerShell. Replace N with the real disk number
# reported by Get-Disk after VirtualSSD mounts the VHDX.
Get-Disk
Initialize-Disk -Number N -PartitionStyle GPT
New-Partition -DiskNumber N -UseMaximumSize -AssignDriveLetter |
    Format-Volume -FileSystem NTFS -NewFileSystemLabel "VirtualSSD"
```

VirtualSSD does not run these partitioning or formatting commands automatically;
verify the disk number before running them.
