# VirtualSSD Recovery Manual

- **Status**: Implemented behavior
- **Revision**: 2.0 (2026-11-06)

---

## 1. Recovery Principles

1. **Idempotence**: Opening a disk replays the write-ahead journal. Replay can be
   repeated without duplicating committed writes.
2. **Never Fabricate Data**: A failed provider fetch or a checksum mismatch is
   returned as an error. VirtualSSD does not substitute zeros for a chunk that was
   expected to contain data.
3. **No Silent Key Regeneration**: If an encrypted disk's key is missing,
   mount and sync fail instead of generating a new key that could never decrypt
   the existing remote chunks.
4. **Manifest Rollback**: `DiskManifest::save_atomic` writes a temporary file and
   keeps the previous manifest as `manifest.prev`. `load_with_fallback`
   tries the current file and then the previous one.
5. **Partial Files Are Removed**: Remote restore writes to
   `disk.vhdx.restore.tmp` and deletes it if any chunk fails.

There is no quarantine directory, no automatic provider revision rollback, and no
out-of-process disk lock database in this build. A disk that cannot be opened is
logged and skipped on startup; it is not silently rewritten.

---

## 2. Failure Scenarios and Recovery Procedures

### Scenario A: Sudden Power Loss During Active Engine Writes
- **Symptoms**: Windows reboots while the block-engine API was writing. Mounted
  VHDX file writes are handled by Windows/NTFS and do not use this journal.
- **Recovery**:
  1. On startup, `VirtualDiskEngine::open` calls `recover_after_crash`.
  2. The WAL scanner reads `%LOCALAPPDATA%\VirtualSSD\disks\<disk_id>\wal.log`.
  3. Replayable writes are applied to the persistent cache.
  4. Truncated or corrupt trailing WAL bytes are discarded up to the last valid
     transaction barrier.
  5. The disk is left `Unmounted`. It becomes `Online` only after a real VHDX
     attach succeeds. Replayed engine dirty chunks remain local; the desktop UI
     does not expose an engine chunk upload. The next unmounted **Mirror
     Snapshot** captures the VHDX file itself.

### Scenario B: Missing Local VHDX (fresh installation or lost file)
- **Symptoms**: `disk.vhdx` does not exist, but the manifest is present.
- **Recovery**:
  1. If the manifest has a `vhdx_file_size` and `vhdx_chunks`, mounting triggers
     `restore_vhdx_from_remote`.
  2. Every referenced chunk is downloaded, decrypted when required, and verified
     against its plaintext checksum. The total file length is checked against the
     manifest.
  3. The file is written to `disk.vhdx.restore.tmp` and renamed to `disk.vhdx`
     only after all chunks succeed.
  4. If no snapshot exists, mounting creates a new blank dynamic VHDX of the
     recorded capacity. This is not a restore and the UI says so.

### Scenario C: Encrypted Disk Without Its Key
- **Symptoms**: The disk manifest has `encrypt_chunks = true`, but
  `disk_dir/encryption.key` is absent.
- **Recovery**:
  1. The disk list shows an amber warning and the mount button is disabled.
  2. Import the 64-character recovery key on the Disks page.
  3. If the key is lost, remote ciphertext cannot be recovered. Contact the
     installation that created the disk or restore the key from its offline
     backup.

### Scenario D: Remote Chunk Corruption or Tampering
- **Symptoms**: A restore fails with a checksum or AES-GCM authentication error.
- **Recovery**:
  1. The partial `disk.vhdx.restore.tmp` is deleted.
  2. The error is stored in the disk's `last_error` and returned to the UI.
  3. VirtualSSD does not query provider revision history or attempt an automatic
     repair. Re-publish a good snapshot from another copy, or restore the
     provider object from its version history manually.
  4. A local VHDX that still exists is not overwritten by a failed restore.

### Scenario E: Remote Deletion Fails
- **Symptoms**: The delete dialog reports a provider error.
- **Recovery**:
  1. Chunks are deleted before the manifest, and the local remote references are
     cleared only after every object and the manifest are deleted.
  2. On failure the local disk is kept, including its remote object ids, so the
     operation can be retried after the provider recovers.

### Scenario F: Damaged Settings File
- **Symptoms**: The UI shows a settings warning on startup.
- **Recovery**:
  1. Invalid JSON is preserved as `settings.json.corrupt-<timestamp>`.
  2. Defaults are used until the user saves settings again.
  3. Out-of-range values in valid JSON are rejected by validation; the original
     file is not overwritten until a successful save.

---

## 3. Recovery Entry Points

- **Journal replay**: automatic in `VirtualDiskEngine::open`.
- **Remote VHDX restore**: automatic on mount when the local file is missing and
  the manifest has a snapshot.
- **Remote manifest import**: Providers page → **Import a Remote Disk Manifest**.
- **Recovery key import**: Disks page → **Import Key**.
- **Integrity audit**: Disks page **Verify Hashes** or the Integrity page. This
  reads the local VHDX/cache; it does not download remote chunks.
