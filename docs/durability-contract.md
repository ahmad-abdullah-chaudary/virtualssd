# VirtualSSD Write Durability & Commit Protocol

- **Status**: Implemented behavior
- **Revision**: 2.1 (2026-11-07)

---

## 1. Current Durability Model

VirtualSSD acknowledges a block-engine write after it has been recorded in the
local write-ahead journal and fsynced. Remote publication is separate.

Scope: the sequence below is the **storage-core block-engine API** path. The
desktop application mounts a Windows VHDX directly, so Windows reads and writes
to the mounted volume do **not** pass through this journal or cache. Cloud
replication of a mounted disk is the unmounted snapshot mirror described in
`docs/architecture.md`.

Implemented path:

1. The write is sliced into 4 MiB chunks.
2. A WAL transaction is started.
3. Each chunk slice is recorded in the append-only journal.
4. The transaction is committed; the journal is flushed and `sync_data()` is called.
5. The chunk is written to the persistent cache with `DIRTY` state.
6. Reads are served from the cache.
7. `VirtualDiskEngine::sync_dirty_chunks` uploads dirty chunks, marks them
   committed, saves the manifest atomically, and checkpoints the journal. This
   engine sync exists in `storage-core` but is **not exposed as a desktop
   command**; the desktop UI only performs VHDX snapshot mirroring.

Not implemented in this build:

- A strict synchronous remote mode that blocks the write until the cloud object and
  manifest are acknowledged.
- A background transfer queue with bounded concurrency and exponential backoff.
- A flush ioctl that blocks until all journal entries are persisted remotely.
- Conversion of an existing unencrypted disk to encrypted chunks (encryption is
  fixed at creation, and changing it would require re-uploading the snapshot).

---

## 2. Commit Sequence

```mermaid
sequenceDiagram
    participant OS as Block-Engine API Caller
    participant Engine as Storage Engine
    participant WAL as Local WAL
    participant Cache as Persistent Cache
    participant Remote as Cloud Provider
    participant Manifest as manifest.json

    OS->>Engine: write_blocks (offset, length, data)
    Engine->>Engine: Slice into 4 MiB chunks
    Engine->>WAL: Begin transaction
    Engine->>WAL: Record chunk writes
    Engine->>WAL: Commit + flush + fsync
    Engine->>Cache: Store chunk as DIRTY
    Engine-->>OS: Acknowledge write
    Note over Engine,Remote: Desktop UI exposes no engine sync; only unmounted VHDX snapshot mirroring
    Engine->>Remote: Upload dirty chunk (Google resumable / Telegram document)
    Remote-->>Engine: Object id + version/file id
    Engine->>Cache: Mark chunk committed
    Engine->>Manifest: Save atomically (temp rename + .prev)
    Engine->>WAL: Checkpoint and truncate
```

---

## 3. Crash Consistency

- **Crash before WAL commit**: the write was not acknowledged; no committed record
  exists.
- **Crash after WAL commit, before cache update**: recovery scans the journal when
  the disk is opened and replays committed writes into the cache.
- **Crash during upload**: the existing manifest still references the last
  committed chunks. An uploaded but unrecorded object is an orphan and is not
  treated as committed.
- **Crash during manifest save**: `manifest.json` is written to a temporary file and
  renamed; the previous manifest is retained as `manifest.prev` for fallback.
- **Crash during checkpoint**: replay is idempotent; committed chunks are not
  duplicated by the engine.

---

## 4. Recovery Entry Point

`VirtualDiskEngine::open` runs `recover_after_crash()` before returning the engine.
The recovery scan reads the WAL, logs the number of replayed writes, and writes
those records into the persistent cache. There is no manual WAL verification button
in the desktop UI; recovery is part of opening the disk.
