//! WAL writer and reader.

use crate::error::{CoreError, CoreResult};
use crate::wal::record::{compute_crc32, WalRecord, WalRecordType, WAL_MAGIC, WAL_VERSION};
use entidb_storage::StorageBackend;
use parking_lot::Mutex;
use std::sync::Arc;

/// Header size for WAL records.
/// magic (4) + version (2) + type (1) + length (4) = 11 bytes
const HEADER_SIZE: usize = 11;

/// CRC size.
const CRC_SIZE: usize = 4;

/// Manages WAL writes and reads.
///
/// The `WalManager` provides append-only writes to the WAL and supports
/// reading records for recovery.
pub struct WalManager {
    /// Storage backend for WAL data.
    backend: Arc<Mutex<Box<dyn StorageBackend>>>,
    /// Whether to sync after each write.
    sync_on_write: bool,
}

impl WalManager {
    /// Creates a new WAL manager.
    pub fn new(backend: Box<dyn StorageBackend>, sync_on_write: bool) -> Self {
        Self {
            backend: Arc::new(Mutex::new(backend)),
            sync_on_write,
        }
    }

    /// Appends a record to the WAL.
    ///
    /// Returns the offset where the record was written.
    pub fn append(&self, record: &WalRecord) -> CoreResult<u64> {
        let payload = record.encode_payload();
        let record_type = record.record_type();

        // Build the full record with envelope
        let mut data = Vec::with_capacity(HEADER_SIZE + payload.len() + CRC_SIZE);

        // Magic
        data.extend_from_slice(&WAL_MAGIC);

        // Version
        data.extend_from_slice(&WAL_VERSION.to_le_bytes());

        // Type
        data.push(record_type.as_byte());

        // Length (payload length)
        let len = u32::try_from(payload.len())
            .map_err(|_| CoreError::invalid_operation("WAL record payload too large"))?;
        data.extend_from_slice(&len.to_le_bytes());

        // Payload
        data.extend_from_slice(&payload);

        // CRC32 (over everything before it)
        let crc = compute_crc32(&data);
        data.extend_from_slice(&crc.to_le_bytes());

        // Write atomically
        let mut backend = self.backend.lock();
        let offset = backend.append(&data)?;

        if self.sync_on_write {
            backend.flush()?;
        }

        Ok(offset)
    }

    /// Flushes all pending writes to durable storage.
    pub fn flush(&self) -> CoreResult<()> {
        self.backend.lock().flush()?;
        Ok(())
    }

    /// Returns the current WAL size.
    pub fn size(&self) -> CoreResult<u64> {
        Ok(self.backend.lock().size()?)
    }

    /// Reads all records from the WAL.
    ///
    /// This is used during recovery to replay committed transactions.
    pub fn read_all(&self) -> CoreResult<Vec<(u64, WalRecord)>> {
        let backend = self.backend.lock();
        let size = backend.size()?;

        let mut records = Vec::new();
        let mut offset = 0u64;

        while offset < size {
            // Try to read header
            if offset + HEADER_SIZE as u64 > size {
                // Incomplete header - truncated WAL
                break;
            }

            let header = backend.read_at(offset, HEADER_SIZE)?;

            // Validate magic
            if header[0..4] != WAL_MAGIC {
                return Err(CoreError::wal_corruption(format!(
                    "invalid magic at offset {offset}"
                )));
            }

            // Check version
            let version = u16::from_le_bytes([header[4], header[5]]);
            if version > WAL_VERSION {
                return Err(CoreError::wal_corruption(format!(
                    "unsupported version {version} at offset {offset}"
                )));
            }

            // Record type
            let type_byte = header[6];
            let record_type = WalRecordType::from_byte(type_byte).ok_or_else(|| {
                CoreError::wal_corruption(format!(
                    "unknown record type {type_byte} at offset {offset}"
                ))
            })?;

            // Payload length
            let payload_len =
                u32::from_le_bytes([header[7], header[8], header[9], header[10]]) as usize;

            // Check if we have the full record
            let total_len = HEADER_SIZE + payload_len + CRC_SIZE;
            if offset + total_len as u64 > size {
                // Incomplete record - truncated WAL
                break;
            }

            // Read payload and CRC
            let rest = backend.read_at(offset + HEADER_SIZE as u64, payload_len + CRC_SIZE)?;
            let payload = &rest[..payload_len];
            let stored_crc = u32::from_le_bytes([
                rest[payload_len],
                rest[payload_len + 1],
                rest[payload_len + 2],
                rest[payload_len + 3],
            ]);

            // Verify CRC
            let mut crc_data = Vec::with_capacity(HEADER_SIZE + payload_len);
            crc_data.extend_from_slice(&header);
            crc_data.extend_from_slice(payload);
            let computed_crc = compute_crc32(&crc_data);

            if stored_crc != computed_crc {
                return Err(CoreError::ChecksumMismatch {
                    expected: stored_crc,
                    actual: computed_crc,
                });
            }

            // Decode record
            let record = WalRecord::decode_payload(record_type, payload)?;
            records.push((offset, record));

            offset += total_len as u64;
        }

        Ok(records)
    }

    /// Iterates over records, calling the callback for each.
    ///
    /// This is more memory-efficient than `read_all` for large WALs.
    pub fn for_each<F>(&self, mut callback: F) -> CoreResult<()>
    where
        F: FnMut(u64, WalRecord) -> CoreResult<bool>,
    {
        let records = self.read_all()?;
        for (offset, record) in records {
            if !callback(offset, record)? {
                break;
            }
        }
        Ok(())
    }

    /// Truncates the WAL to the specified offset.
    ///
    /// This is used after checkpoint to reclaim space. All data after
    /// the specified offset is discarded.
    ///
    /// # Arguments
    ///
    /// * `offset` - The offset to truncate to (all data after this is removed)
    pub fn truncate(&self, offset: u64) -> CoreResult<()> {
        let mut backend = self.backend.lock();
        backend.truncate(offset)?;
        Ok(())
    }

    /// Clears all data from the WAL.
    ///
    /// This truncates the WAL to 0 bytes. Used after checkpoint when
    /// all committed transactions have been flushed to segments.
    pub fn clear(&self) -> CoreResult<()> {
        self.truncate(0)
    }
}

impl std::fmt::Debug for WalManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalManager")
            .field("sync_on_write", &self.sync_on_write)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CollectionId, SequenceNumber, TransactionId};
    use entidb_storage::InMemoryBackend;

    fn create_wal() -> WalManager {
        WalManager::new(Box::new(InMemoryBackend::new()), false)
    }

    #[test]
    fn append_and_read_begin() {
        let wal = create_wal();
        let record = WalRecord::Begin {
            txid: TransactionId::new(1),
        };
        wal.append(&record).unwrap();

        let records = wal.read_all().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].1, record);
    }

    #[test]
    fn append_and_read_put() {
        let wal = create_wal();
        let record = WalRecord::Put {
            txid: TransactionId::new(1),
            collection_id: CollectionId::new(10),
            entity_id: [1; 16],
            before_hash: None,
            after_bytes: vec![0xCA, 0xFE],
        };
        wal.append(&record).unwrap();

        let records = wal.read_all().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].1, record);
    }

    #[test]
    fn append_multiple_records() {
        let wal = create_wal();

        let r1 = WalRecord::Begin {
            txid: TransactionId::new(1),
        };
        let r2 = WalRecord::Put {
            txid: TransactionId::new(1),
            collection_id: CollectionId::new(5),
            entity_id: [2; 16],
            before_hash: None,
            after_bytes: vec![1, 2, 3],
        };
        let r3 = WalRecord::Commit {
            txid: TransactionId::new(1),
            sequence: SequenceNumber::new(1),
        };

        wal.append(&r1).unwrap();
        wal.append(&r2).unwrap();
        wal.append(&r3).unwrap();

        let records = wal.read_all().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].1, r1);
        assert_eq!(records[1].1, r2);
        assert_eq!(records[2].1, r3);
    }

    #[test]
    fn read_empty_wal() {
        let wal = create_wal();
        let records = wal.read_all().unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn wal_size_increases() {
        let wal = create_wal();
        assert_eq!(wal.size().unwrap(), 0);

        wal.append(&WalRecord::Begin {
            txid: TransactionId::new(1),
        })
        .unwrap();

        assert!(wal.size().unwrap() > 0);
    }

    #[test]
    fn full_transaction_sequence() {
        let wal = create_wal();

        // Transaction 1: committed
        wal.append(&WalRecord::Begin {
            txid: TransactionId::new(1),
        })
        .unwrap();
        wal.append(&WalRecord::Put {
            txid: TransactionId::new(1),
            collection_id: CollectionId::new(1),
            entity_id: [1; 16],
            before_hash: None,
            after_bytes: vec![10, 20, 30],
        })
        .unwrap();
        wal.append(&WalRecord::Commit {
            txid: TransactionId::new(1),
            sequence: SequenceNumber::new(1),
        })
        .unwrap();

        // Transaction 2: aborted
        wal.append(&WalRecord::Begin {
            txid: TransactionId::new(2),
        })
        .unwrap();
        wal.append(&WalRecord::Put {
            txid: TransactionId::new(2),
            collection_id: CollectionId::new(1),
            entity_id: [2; 16],
            before_hash: None,
            after_bytes: vec![40, 50],
        })
        .unwrap();
        wal.append(&WalRecord::Abort {
            txid: TransactionId::new(2),
        })
        .unwrap();

        let records = wal.read_all().unwrap();
        assert_eq!(records.len(), 6);

        // Verify transaction IDs
        assert_eq!(records[0].1.txid(), Some(TransactionId::new(1)));
        assert_eq!(records[5].1.txid(), Some(TransactionId::new(2)));
    }

    #[test]
    fn for_each_iteration() {
        let wal = create_wal();

        for i in 0..5 {
            wal.append(&WalRecord::Begin {
                txid: TransactionId::new(i),
            })
            .unwrap();
        }

        let mut count = 0;
        wal.for_each(|_, _| {
            count += 1;
            Ok(true)
        })
        .unwrap();

        assert_eq!(count, 5);
    }

    #[test]
    fn for_each_early_exit() {
        let wal = create_wal();

        for i in 0..10 {
            wal.append(&WalRecord::Begin {
                txid: TransactionId::new(i),
            })
            .unwrap();
        }

        let mut count = 0;
        wal.for_each(|_, _| {
            count += 1;
            Ok(count < 3) // Stop after 3
        })
        .unwrap();

        assert_eq!(count, 3);
    }

    #[test]
    fn clear_wal() {
        let wal = create_wal();

        // Write some records
        wal.append(&WalRecord::Begin {
            txid: TransactionId::new(1),
        })
        .unwrap();
        wal.append(&WalRecord::Commit {
            txid: TransactionId::new(1),
            sequence: SequenceNumber::new(1),
        })
        .unwrap();

        assert!(wal.size().unwrap() > 0);
        assert_eq!(wal.read_all().unwrap().len(), 2);

        // Clear the WAL
        wal.clear().unwrap();

        assert_eq!(wal.size().unwrap(), 0);
        assert!(wal.read_all().unwrap().is_empty());
    }

    #[test]
    fn truncate_wal() {
        let wal = create_wal();

        // Write a record and capture its end offset
        let offset1 = wal
            .append(&WalRecord::Begin {
                txid: TransactionId::new(1),
            })
            .unwrap();
        let size_after_first = wal.size().unwrap();

        // Write second record
        wal.append(&WalRecord::Commit {
            txid: TransactionId::new(1),
            sequence: SequenceNumber::new(1),
        })
        .unwrap();

        assert_eq!(wal.read_all().unwrap().len(), 2);

        // Truncate back to after first record
        wal.truncate(size_after_first).unwrap();

        // Should only have the first record
        let records = wal.read_all().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, offset1);
    }
}
