# VirtualSSD — Performance, Persistence, and Robustness Analysis

- **Revision**: 2.0 (2026-11-12)
- **Scope**: the desktop application as built today, and the changes that make
  it fast and durable without becoming a messy cache.

This document is written against the actual implementation, not a design
sketch. Every item names the file and the mechanism involved.

---

## 1. What the application does today

1. The user creates a disk. A dynamic VHDX is created with
   `CreateVirtualDisk` and a manifest is published to the provider.
2. The user mounts it. Windows attaches the VHDX as `\\.\PhysicalDriveN`,
   GPT is initialized, NTFS is formatted, and a drive letter is assigned.
3. The user writes files. Windows writes them into the VHDX file directly.
   **VirtualSSD does not see this I/O.**
4. The user unmounts and presses **Mirror Snapshot**. The VHDX file is read,
   split into 4 MiB chunks, hashed with BLAKE3, optionally encrypted with
   AES-256-GCM, uploaded to the provider, and recorded in the manifest.
5. On another machine, the manifest is imported and the VHDX is restored by
   downloading every chunk, verifying it, and renaming the result into place.

Everything in steps 3 and 4 is where the limitations live.

---

## 2. Throughput work — implemented

### 2.1 Uploads are concurrent — implemented

`crates/storage-core/src/transfer.rs` replaces the old one-chunk-at-a-time
loop with a bounded producer/consumer pipeline:

- The **producer** (`ChunkProducer`) reads the VHDX sequentially, hashes each
  chunk with BLAKE3, skips chunks that are unchanged or entirely zero,
  encrypts the rest, and hands them to a bounded channel. Local disk I/O and
  hashing therefore overlap with the network transfers.
- The **consumers** upload chunks concurrently, bounded by a
  `tokio::sync::Semaphore` so the provider is not flooded and the in-flight
  chunk buffers stay bounded. The limit is
  `transfer::DEFAULT_UPLOAD_CONCURRENCY` (4).
- `crates/storage-core/tests/transfer_concurrency.rs` proves the behaviour
  deterministically: a `tokio::sync::Barrier` only opens once several
  transfers are genuinely in flight, and a second test asserts the peak
  in-flight count equals the configured limit — reached, never exceeded.

### 2.2 The manifest is checkpointed, not rewritten per chunk — implemented

`manifest.save_atomic()` used to run *inside* the chunk loop. Each call
serialized the whole manifest, wrote a temp file, **copied the previous
manifest to `manifest.json.prev`**, and renamed — quadratic in the chunk
count.

The manifest is now updated in memory by the consumers and written to disk on
a checkpoint interval (`transfer::MANIFEST_CHECKPOINT_INTERVAL`, 64 chunks)
plus once at the end. A crash costs at most one checkpoint window of
re-uploaded chunks instead of the whole manifest's write traffic.

### 2.3 Google Drive upload sessions are flushed on an interval — implemented

`persist_sessions()` used to clone and rewrite the entire session map after
every chunk, while holding a global mutex that serialized all upload
activity. The session map is now marked dirty and flushed on an interval
(`SESSION_FLUSH_INTERVAL_SECS`, 5 seconds) and once at the end of the
transfer via the new `CloudStorageProvider::flush_transfer_state()` hook.

### 2.4 Reads overlap with uploads — implemented

The producer reads and hashes chunk *n+1* while chunks *n* and earlier are in
flight, so the local disk is never idle waiting for the network and the
network is never idle waiting for the disk.

### 2.5 Downloads are concurrent — implemented

`restore_vhdx_from_remote()` awaits each `download_chunk()` in turn. It now
downloads with the same bounded-concurrency scheme and writes each chunk
straight to its offset in the preallocated temporary file, so the writes land
in the right place without a second pass.

### 2.6 The HTTP clients are tuned for throughput — implemented

Both providers build their client through a shared `build_client()`:

- `tcp_nodelay(true)` — disables Nagle's algorithm. With many small requests
  the delayed-ACK/Nagle interaction costs real latency.
- `pool_idle_timeout(120s)` and `pool_max_idle_per_host(16)` — keep-alive
  connections survive between chunks instead of being re-established.
- `connect_timeout(15s)` separated from the overall `timeout(300s)` — a single
  global timeout means one slow chunk aborts a long transfer.

---

## 3. Persistence work — implemented

### 3.1 Crash-resume is bounded and explicit

The checkpoint interval in 2.2 is the resume boundary. It is a deliberate
trade-off and it is documented rather than accidental.

### 3.2 The block-engine cache does not back the VHDX path

`crates/cache-manager/src/lib.rs` implements a bounded LRU cache with
clean/dirty tracking. It is used by the block-engine API
(`read_blocks`/`write_blocks`). The VHDX mirror path does **not** use it: it
reads the VHDX file directly.

This is not "messy caching" — it is the opposite. There is no duplicated
copy of the disk contents to keep coherent, and no risk of a stale cache
being served. The cache only holds data for the block-engine path.

**What this means for speed:** the OS page cache already caches the VHDX
file, so re-reading it is cheap. The engine cache adds nothing for the VHDX
path and should not be forced into it.

### 3.3 The manifest fallback is a single generation

`manifest.json.prev` holds one previous generation. Two consecutive failures
corrupt both. `load_with_fallback()` handles one level.

**Still to do (optional):** keep the last *known-good* generation separately
from the last-written one, so a bad write never destroys the last good
manifest.

### 3.4 The disk lock is a per-process file check

There is no cross-process lock. Two instances of VirtualSSD can open the same
disk directory and corrupt each other's manifest and WAL.

**Still to do:** an exclusive lock file on the disk directory, held for the
lifetime of the open engine, released on drop.

### 3.6 Mirroring a mounted disk — implemented

`crates/storage-core/src/vss.rs` implements a Volume Shadow Copy of the
volume that holds the VHDX, using the `Win32_ShadowCopy` WMI class.

`VirtualDiskEngine::sync_vhdx_snapshot_live()`:

1. **Flushes the guest file system** into the VHDX with
   `FSCTL_FLUSH_VOLUME` (`windows_vhd::flush_volume`). NTFS writes everything
   the guest has committed out to the virtual disk.
2. **Takes a volume shadow copy** of the volume that holds the VHDX. The VSS
   service asks the file system writer to flush its cache and then images the
   volume.
3. **Mirrors the shadow copy** with the same pipeline as an unmounted disk —
   the transfer reads `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopyN\...`
   instead of the live file.
4. **Releases the shadow copy** when the transfer ends, so its differential
   area does not linger on the volume.

The disk stays mounted and usable for the whole transfer. That is what turns
VirtualSSD from a backup tool into a persistent drive.

**Consistency.** The VHDX file is written by the Microsoft Virtual Disk
driver (`vhdmp`), which pushes its data blocks through the file system
cache. The NTFS VSS writer flushes that cache before the image is taken, so
the copy contains the data that was written. The driver keeps its block
allocation table in memory and writes it out lazily, so the copy can be
*slightly behind* the live volume — but it is never a mixture of two moments,
because the driver writes a data block before it marks the block allocated. A
shadow copy is therefore either consistent or a little stale, and in both
cases it is a valid VHDX. The guest flush in step 1 narrows that window
further.

**Still to verify.** The VSS path needs an elevated session, a real mounted
VHDX, and a volume that supports shadow copies. It has not been exercised
end to end in this environment. The unmounted mirror path remains the
supported fallback and is unchanged.

---

## 4. Limitations that cannot be fixed without a kernel component

### 4.1 No live write-through — the disk is not a real cloud drive while mounted

Windows writes go straight into the VHDX file. Nothing mirrors them to the
provider automatically.

**What has changed:** the disk no longer has to be unmounted to be mirrored.
`sync_vhdx_snapshot_live()` takes a volume shadow copy of the volume and
mirrors that image while the disk stays mounted (see 3.6). What is still
missing is the *push*: a change is mirrored when the user asks, not the
instant it is written.

The options for true write-through are:

| Approach | What it takes | Verdict |
|---|---|---|
| **Background mirroring on a timer** | The VSS path in 3.6, on a schedule | **Feasible now.** No kernel component. |
| Filter driver (`fltmgr`) | Signed, attested file-system minifilter | Needs EV certificate + WHQL. The same wall the miniport hit. |
| Kernel block driver | Storport miniport | Same wall, plus the data path was never completed. |
| Filesystem on a cloud backend | A real network filesystem (SMB, NFS, or a cloud block device) | Different product; abandons the VHDX model. |

A background worker that mirrors on a timer, using the shadow-copy path, is
the achievable next step. It needs no kernel component at all.

### 4.2 No background auto-sync

Synchronization is manual. There is no scheduler, no watch, no periodic
snapshot. A user who forgets to unmount never mirrors.

**Still to do (feasible):** a background worker that mirrors on a timer or on
VSS snapshot events.

### 4.3 No integrity scrub of remote chunks

Audits verify only chunks that are present locally. Remote chunks are
verified during restore, not proactively.

**Still to do (feasible):** a periodic scrub that downloads and verifies a
random sample of remote chunks and reports drift.

### 4.4 No space accounting before an upload

The engine checks that the remote manifest is coherent, but it does not check
whether the provider actually has room. Telegram's 50 MB per-document limit
and a Drive quota both surface only as a failure mid-transfer.

**Still to do (feasible):** pre-flight a quota check and report the
requirement before starting.

### 4.5 Every mirror re-reads the whole VHDX

To know whether a chunk changed, the engine must read and hash it. A mirror
of an unchanged 100 GB disk therefore reads all 100 GB.

**Still to do (advanced):** parse the VHDX **Block Allocation Table** to find
which blocks are actually allocated, and hash only those. This is a
VHDX-format-specific optimization and needs careful work, because a wrong
answer would silently skip a changed region.

---

## 5. What "no messy caching" means for this design

The rule the implementation follows, and should keep following:

1. **One copy of the truth.** The VHDX file is the truth. The mirror is a
   copy of it, and the manifest describes that copy. There is no third copy
   to invalidate.
2. **The cache never serves correctness.** The engine cache holds data for
   the block-engine API only. It is bounded, evicts only clean chunks, and
   is never consulted to decide whether a chunk is safe to upload.
3. **Sparse means absent.** All-zero regions are not uploaded and are deleted
   remotely when they appear. They are not represented by a placeholder
   object.
4. **Deduplicate by content.** Telegram skips an upload when the checksum
   already matches a stored chunk. Google Drive names objects by checksum, so
   identical content converges on one object.
5. **Reuse sessions, not copies.** The resumable upload session lets an
   interrupted chunk continue from the byte offset the provider already has.
   That is the opposite of caching: it avoids re-sending bytes the provider
   already holds.
6. **Bound everything.** Cache size, upload concurrency, download
   concurrency, manifest checkpoint interval, session flush interval, and the
   log ring buffer all have explicit limits.

---

## 6. Remaining order of work

| # | Change | File | Impact | Risk |
|---|---|---|---|---|
| 1 | Background auto-mirror on a timer, using the VSS path | `apps/desktop/src-tauri/src/main.rs` | High | Low |
| 2 | Cross-process disk lock | `crates/storage-core/src/lib.rs` | Medium | Low |
| 3 | Remote space pre-flight and quota report | `crates/storage-core/src/lib.rs` | Low | Low |
| 4 | VHDX Block Allocation Table to skip unallocated regions | new module | High | High |
| 5 | Last-known-good manifest generation | `crates/manifest/src/lib.rs` | Low | Low |
| 6 | Periodic remote integrity scrub | `crates/storage-core/src/lib.rs` | Low | Low |
| 7 | S3 / OneDrive / Backblaze provider adapters | new crates | Medium | Medium |

Items 1 and 4 are what turn VirtualSSD from a backup tool into a persistent
drive.

---

## 7. Explicitly out of scope

- A kernel block driver or file-system filter. It requires EV signing and
  WHQL attestation, and the prototype under `archive/` demonstrated why
  shipping an incomplete one is worse than shipping nothing.
- Windows services, named pipes, or a background daemon. The application is
  a single user-session process; the tray icon covers background presence.
- Replacing the VHDX model with a network filesystem.
