//! Write-Ahead Journal (WAL) and Crash-Consistency Engine for VirtualSSD.
//!
//! Enforces transactional durability, crash recovery, and idempotent replay.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use virtualssd_integrity::ChunkChecksum;

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("I/O error during journal access: {0}")]
    Io(#[from] std::io::Error),

    #[error("Corrupt journal entry at offset {offset}: {reason}")]
    CorruptEntry { offset: u64, reason: String },

    #[error("Journal transaction error: {0}")]
    TransactionError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalEntry {
    Begin {
        tx_id: u64,
        timestamp: i64,
    },
    ChunkWrite {
        tx_id: u64,
        chunk_index: u64,
        offset_in_chunk: usize,
        data: Vec<u8>,
        checksum: ChunkChecksum,
    },
    Commit {
        tx_id: u64,
        timestamp: i64,
    },
    Abort {
        tx_id: u64,
    },
}

pub struct WriteAheadJournal {
    file: BufWriter<File>,
    path: PathBuf,
    current_tx_id: u64,
}

impl WriteAheadJournal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        let mut journal = Self {
            file: BufWriter::new(file),
            path,
            current_tx_id: 1,
        };

        // Determine current max tx_id
        let recovery = journal.scan_recovery()?;
        journal.current_tx_id = recovery.max_tx_id + 1;

        Ok(journal)
    }

    pub fn begin_transaction(&mut self) -> Result<u64, JournalError> {
        let tx_id = self.current_tx_id;
        self.current_tx_id += 1;

        let entry = JournalEntry::Begin {
            tx_id,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        self.append_entry(&entry)?;
        Ok(tx_id)
    }

    pub fn record_chunk_write(
        &mut self,
        tx_id: u64,
        chunk_index: u64,
        offset_in_chunk: usize,
        data: &[u8],
    ) -> Result<(), JournalError> {
        let checksum = ChunkChecksum::blake3(data);
        let entry = JournalEntry::ChunkWrite {
            tx_id,
            chunk_index,
            offset_in_chunk,
            data: data.to_vec(),
            checksum,
        };
        self.append_entry(&entry)
    }

    pub fn commit_transaction(&mut self, tx_id: u64) -> Result<(), JournalError> {
        let entry = JournalEntry::Commit {
            tx_id,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        self.append_entry(&entry)?;
        self.file.flush()?;
        self.file.get_ref().sync_data()?;
        Ok(())
    }

    pub fn abort_transaction(&mut self, tx_id: u64) -> Result<(), JournalError> {
        let entry = JournalEntry::Abort { tx_id };
        self.append_entry(&entry)?;
        self.file.flush()?;
        Ok(())
    }

    fn append_entry(&mut self, entry: &JournalEntry) -> Result<(), JournalError> {
        let encoded = serde_json::to_vec(entry).map_err(|e| {
            JournalError::TransactionError(format!("Failed to encode journal entry: {e}"))
        })?;
        let len = encoded.len() as u32;

        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&encoded)?;
        Ok(())
    }

    /// Replay recovery scanner
    pub fn scan_recovery(&self) -> Result<RecoveryReport, JournalError> {
        if !self.path.exists() {
            return Ok(RecoveryReport::default());
        }

        let mut file = BufReader::new(File::open(&self.path)?);
        let mut report = RecoveryReport::default();
        let mut active_txs = std::collections::HashMap::new();

        let mut len_buf = [0u8; 4];

        while let Ok(()) = file.read_exact(&mut len_buf) {
            let len = u32::from_le_bytes(len_buf) as usize;
            if len == 0 || len > 64 * 1024 * 1024 {
                break; // Corrupt or truncated tail
            }

            let mut data_buf = vec![0u8; len];
            if file.read_exact(&mut data_buf).is_err() {
                // Truncated tail (unclean power loss during write)
                report.corrupt_tail_bytes = len as u64 + 4;
                break;
            }

            match serde_json::from_slice::<JournalEntry>(&data_buf) {
                Ok(entry) => {
                    match entry {
                        JournalEntry::Begin { tx_id, .. } => {
                            report.max_tx_id = report.max_tx_id.max(tx_id);
                            active_txs.insert(tx_id, Vec::new());
                        }
                        JournalEntry::ChunkWrite {
                            tx_id,
                            chunk_index,
                            offset_in_chunk,
                            data,
                            checksum,
                        } => {
                            if let Some(writes) = active_txs.get_mut(&tx_id) {
                                writes.push((chunk_index, offset_in_chunk, data, checksum));
                            }
                        }
                        JournalEntry::Commit { tx_id, .. } => {
                            if let Some(writes) = active_txs.remove(&tx_id) {
                                report.committed_transactions += 1;
                                for (chunk_index, offset_in_chunk, data, checksum) in writes {
                                    report.replayed_writes.push(ReplayedWrite {
                                        tx_id,
                                        chunk_index,
                                        offset_in_chunk,
                                        data,
                                        checksum,
                                    });
                                }
                            }
                        }
                        JournalEntry::Abort { tx_id } => {
                            active_txs.remove(&tx_id);
                            report.aborted_transactions += 1;
                        }
                    }
                }
                Err(_) => {
                    report.corrupt_tail_bytes += (len + 4) as u64;
                    break;
                }
            }
        }

        report.uncommitted_transactions = active_txs.len();
        Ok(report)
    }

    /// Truncate cleanly after safe checkpointing
    pub fn checkpoint_truncate(&mut self) -> Result<(), JournalError> {
        self.file.flush()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.file = BufWriter::new(file);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct RecoveryReport {
    pub max_tx_id: u64,
    pub committed_transactions: usize,
    pub uncommitted_transactions: usize,
    pub aborted_transactions: usize,
    pub corrupt_tail_bytes: u64,
    pub replayed_writes: Vec<ReplayedWrite>,
}

#[derive(Debug)]
pub struct ReplayedWrite {
    pub tx_id: u64,
    pub chunk_index: u64,
    pub offset_in_chunk: usize,
    pub data: Vec<u8>,
    pub checksum: ChunkChecksum,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wal_commit_and_recovery() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("wal.log");

        {
            let mut wal = WriteAheadJournal::open(&log_path).unwrap();
            let tx1 = wal.begin_transaction().unwrap();
            let data = b"Sector data for crash recovery test";
            wal.record_chunk_write(tx1, 0, 512, data).unwrap();
            wal.commit_transaction(tx1).unwrap();

            // Uncommitted transaction to test clean discarding
            let tx2 = wal.begin_transaction().unwrap();
            wal.record_chunk_write(tx2, 1, 0, b"uncommitted garbage").unwrap();
            // Don't commit tx2!
        }

        // Reopen and inspect recovery
        let wal2 = WriteAheadJournal::open(&log_path).unwrap();
        let report = wal2.scan_recovery().unwrap();

        assert_eq!(report.committed_transactions, 1);
        assert_eq!(report.uncommitted_transactions, 1);
        assert_eq!(report.replayed_writes.len(), 1);
        assert_eq!(report.replayed_writes[0].chunk_index, 0);
        assert_eq!(report.replayed_writes[0].offset_in_chunk, 512);
        assert_eq!(report.replayed_writes[0].data, b"Sector data for crash recovery test");
    }
}
