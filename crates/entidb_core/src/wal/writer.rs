//! WAL writer and reader.

use crate::error::{CoreError, CoreResult};
use crate::wal::record::{compute_crc32, WalRecord, WAL_MAGIC, WAL_VERSION};
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
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The record payload exceeds the maximum size (4 GiB)
    /// - I/O errors occur during write
    pub fn append(&self, record: &WalRecord) -> CoreResult<u64> {
        let payload = record.encode_payload()?;
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

    /// Returns a streaming iterator over WAL records.
    ///
    /// This is the preferred method for reading WAL records as it uses
    /// O(1) memory regardless of WAL size. Records are read one-by-one
    /// from the storage backend.
    ///
    /// # Example
    ///
    /// ```ignore
    /// for result in wal.iter()? {
    ///     let (offset, record) = result?;
    ///     // Process record...
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot be accessed.
    pub fn iter(&self) -> CoreResult<super::WalRecordIterator<'_>> {
        let backend = self.backend.lock();
        super::WalRecordIterator::new(backend, 0)
    }

    /// Iterates over records with a streaming callback.
    ///
    /// This is more memory-efficient than `read_all` for large WALs.
    /// The callback receives each record and returns `Ok(true)` to continue
    /// or `Ok(false)` to stop iteration early.
    ///
    /// # Arguments
    ///
    /// * `callback` - Function called for each record. Returns `Ok(true)` to
    ///   continue, `Ok(false)` to stop, or `Err(...)` to abort with error.
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or the callback returns an error.
    pub fn for_each_streaming<F>(&self, mut callback: F) -> CoreResult<()>
    where
        F: FnMut(u64, WalRecord) -> CoreResult<bool>,
    {
        for result in self.iter()? {
            let (offset, record) = result?;
            if !callback(offset, record)? {
                break;
            }
        }
        Ok(())
    }

    /// Reads all records from the WAL.
    ///
    /// **Note:** For large WALs, prefer using `iter()` which streams records
    /// with O(1) memory usage.
    ///
    /// This method is retained for backwards compatibility and for cases where
    /// having all records in memory is acceptable (small WALs, testing).
    pub fn read_all(&self) -> CoreResult<Vec<(u64, WalRecord)>> {
        self.iter()?.collect()
    }

    /// Iterates over records, calling the callback for each.
    ///
    /// **Deprecated:** Use `for_each_streaming` instead for true streaming behavior.
    /// This method is retained for backwards compatibility.
    #[deprecated(
        since = "0.1.0",
        note = "Use for_each_streaming() for true streaming behavior"
    )]
    pub fn for_each<F>(&self, callback: F) -> CoreResult<()>
    where
        F: FnMut(u64, WalRecord) -> CoreResult<bool>,
    {
        self.for_each_streaming(callback)
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

    /// Returns the backend for testing purposes.
    ///
    /// This allows tests to directly manipulate the underlying storage
    /// to simulate crash scenarios like truncated writes or corruption.
    #[cfg(test)]
    pub(crate) fn get_backend_for_testing(&self) -> Arc<Mutex<Box<dyn StorageBackend>>> {
        Arc::clone(&self.backend)
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
        wal.for_each_streaming(|_, _| {
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
        wal.for_each_streaming(|_, _| {
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
