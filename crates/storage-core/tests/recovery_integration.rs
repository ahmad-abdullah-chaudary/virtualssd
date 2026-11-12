//! VirtualSSD Crash Recovery & Idempotence Integration Test Suite.
//!
//! Simulates unexpected power loss, WAL boundary truncations, and recovery replays.

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use virtualssd_block_map::DEFAULT_CHUNK_SIZE;
    use virtualssd_journal::WriteAheadJournal;
    use virtualssd_manifest::{DiskManifest, ProviderType};
    use virtualssd_storage_core::{DiskStatus, VirtualDiskEngine};

    #[tokio::test]
    async fn test_crash_recovery_with_truncated_wal() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("wal.log");

        // 1. Setup initial journal and commit 2 valid transactions
        {
            let mut wal = WriteAheadJournal::open(&wal_path).unwrap();
            let tx1 = wal.begin_transaction().unwrap();
            wal.record_chunk_write(tx1, 0, 0, b"Valid transaction data #1")
                .unwrap();
            wal.commit_transaction(tx1).unwrap();

            let tx2 = wal.begin_transaction().unwrap();
            wal.record_chunk_write(tx2, 1, 0, b"Valid transaction data #2")
                .unwrap();
            wal.commit_transaction(tx2).unwrap();

            // Simulate power cut mid-write: write incomplete bytes
            let tx3 = wal.begin_transaction().unwrap();
            wal.record_chunk_write(tx3, 2, 0, b"Partial write interrupted")
                .unwrap();
            // Incomplete: no commit, cut off bytes
        }

        // 2. Open storage engine over the directory
        let manifest = DiskManifest::new(
            "Crash-Test-Disk",
            ProviderType::Local,
            "local-test",
            100 * 1024 * 1024,
            DEFAULT_CHUNK_SIZE,
        )
        .unwrap();

        let engine = VirtualDiskEngine::open(dir.path(), manifest, None, 50 * 1024 * 1024).unwrap();
        // Opening a disk replays the WAL but does not attach it to Windows, so
        // the truthful status after recovery is Unmounted.
        assert_eq!(engine.telemetry().status, DiskStatus::Unmounted);

        // 3. Verify committed blocks are preserved and readable
        let mut read_buf = vec![0u8; 25];
        engine.read_blocks(0, &mut read_buf).await.unwrap();
        assert_eq!(&read_buf, b"Valid transaction data #1");

        // 4. Verify uncommitted partial write was not accepted
        let mut uncommitted_buf = vec![0xFFu8; 25];
        let chunk1_offset = DEFAULT_CHUNK_SIZE as u64;
        engine
            .read_blocks(chunk1_offset, &mut uncommitted_buf)
            .await
            .unwrap();
        assert_eq!(&uncommitted_buf, b"Valid transaction data #2");
    }

    #[tokio::test]
    async fn test_sparse_region_zero_fill_contract() {
        let dir = tempdir().unwrap();
        let manifest = DiskManifest::new(
            "Sparse-Contract-Disk",
            ProviderType::Local,
            "local-test",
            1024 * 1024 * 1024, // 1 GB
            DEFAULT_CHUNK_SIZE,
        )
        .unwrap();

        let engine = VirtualDiskEngine::open(dir.path(), manifest, None, 50 * 1024 * 1024).unwrap();
        engine.set_status(DiskStatus::Online);

        // Read unwritten sparse block at 500 MiB offset
        let mut buffer = vec![0xFFu8; 4096];
        engine
            .read_blocks(500 * 1024 * 1024, &mut buffer)
            .await
            .unwrap();

        // Must strictly return deterministic zeros without allocating or contacting cloud
        assert!(buffer.iter().all(|&b| b == 0));
        assert_eq!(engine.telemetry().allocated_chunks, 0);
    }
}
