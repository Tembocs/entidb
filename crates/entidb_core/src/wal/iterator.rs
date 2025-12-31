//! Streaming WAL record iterator.
//!
//! Provides O(1) memory streaming over WAL records, reading records one-by-one
//! from the storage backend without loading the entire WAL into memory.
//!
//! This is essential for handling large WALs during recovery without
//! risking out-of-memory conditions.
//!
//! # Recovery Policy
//!
//! The iterator implements EntiDB's recovery policy which distinguishes between
//! **tolerated** conditions (crash mid-write) and **fatal** conditions (corruption):
//!
//! ## Tolerated (returns `Ok(None)`)
//!
//! - Truncated header: fewer than 11 bytes remain at end of WAL
//! - Truncated payload: record length exceeds available bytes
//!
//! These represent incomplete writes that were interrupted before fsync.
//! The partial record is safely discarded.
//!
//! ## Fatal (returns `Err(...)`)
//!
//! - CRC mismatch: `Err(ChecksumMismatch)` - data corruption detected
//! - Invalid magic: `Err(WalCorruption)` - not a valid WAL record
//! - Unknown type: `Err(WalCorruption)` - unrecognized record type
//! - Future version: `Err(WalCorruption)` - unsupported format version
//!
//! Fatal errors abort database open to prevent silent data loss.

use crate::error::{CoreError, CoreResult};
use crate::wal::record::{compute_crc32, WalRecord, WalRecordType, WAL_MAGIC, WAL_VERSION};
use entidb_storage::StorageBackend;
use parking_lot::MutexGuard;

/// Header size for WAL records.
/// magic (4) + version (2) + type (1) + length (4) = 11 bytes
const HEADER_SIZE: usize = 11;

/// CRC size.
const CRC_SIZE: usize = 4;

/// Read buffer size for streaming iteration.
/// We read in chunks to minimize I/O syscalls while keeping memory bounded.
const READ_BUFFER_SIZE: usize = 64 * 1024; // 64 KB

/// A streaming iterator over WAL records.
///
/// This iterator reads WAL records one-by-one from the storage backend,
/// keeping memory usage constant regardless of WAL size.
///
/// # Design
///
/// - Uses a fixed-size read buffer to minimize I/O operations
/// - Parses records incrementally from the buffer
/// - Refills buffer only when needed
/// - Returns `(offset, WalRecord)` pairs for each valid record
///
/// # Error Handling
///
/// - CRC mismatches return an error immediately
/// - Truncated records (incomplete header or payload) are treated as WAL end
/// - Invalid magic bytes return a corruption error
/// - Unknown record types return a corruption error
///
/// # Example
///
/// ```ignore
/// let iter = WalRecordIterator::new(&backend, 0)?;
/// for result in iter {
///     let (offset, record) = result?;
///     // Process record...
/// }
/// ```
pub struct WalRecordIterator<'a> {
    /// Reference to the storage backend.
    backend: MutexGuard<'a, Box<dyn StorageBackend>>,
    /// Total size of the WAL.
    total_size: u64,
    /// Current read position in the WAL.
    current_offset: u64,
    /// Read buffer for reducing I/O syscalls.
    buffer: Vec<u8>,
    /// Current position within the buffer.
    buffer_pos: usize,
    /// Number of valid bytes in the buffer.
    buffer_len: usize,
    /// Starting offset of the buffer in the WAL.
    buffer_start_offset: u64,
    /// Whether we've encountered an error or reached the end.
    finished: bool,
}

impl<'a> WalRecordIterator<'a> {
    /// Creates a new streaming iterator starting at the given offset.
    ///
    /// # Arguments
    ///
    /// * `backend` - Locked storage backend to read from
    /// * `start_offset` - Offset to start reading from (usually 0)
    ///
    /// # Errors
    ///
    /// Returns an error if the backend size cannot be determined.
    pub fn new(
        backend: MutexGuard<'a, Box<dyn StorageBackend>>,
        start_offset: u64,
    ) -> CoreResult<Self> {
        let total_size = backend.size()?;
        Ok(Self {
            backend,
            total_size,
            current_offset: start_offset,
            buffer: vec![0u8; READ_BUFFER_SIZE],
            buffer_pos: 0,
            buffer_len: 0,
            buffer_start_offset: start_offset,
            finished: false,
        })
    }

    /// Ensures at least `min_bytes` are available in the buffer from current position.
    ///
    /// Returns `true` if the requested bytes are available, `false` if EOF.
    ///
    /// # Dynamic Buffer Resizing
    ///
    /// If a record is larger than the default buffer size, the buffer is
    /// dynamically resized to accommodate it. This ensures that large records
    /// can be read while maintaining O(1) memory for typical small records.
    fn ensure_buffered(&mut self, min_bytes: usize) -> CoreResult<bool> {
        let available = self.buffer_len - self.buffer_pos;
        if available >= min_bytes {
            return Ok(true);
        }

        // Calculate how many more bytes we need from the WAL
        let bytes_needed_from_wal = min_bytes - available;
        let remaining_in_wal = (self.total_size - self.current_offset) as usize - available;
        
        // Check if there's enough data in the WAL
        if remaining_in_wal < bytes_needed_from_wal {
            return Ok(false);
        }

        // Move any remaining data to the start of the buffer
        if self.buffer_pos > 0 && available > 0 {
            self.buffer.copy_within(self.buffer_pos..self.buffer_len, 0);
        }
        self.buffer_len = available;
        self.buffer_pos = 0;
        self.buffer_start_offset = self.current_offset;

        // If we need more space than the buffer can hold, resize it
        if min_bytes > self.buffer.len() {
            // Round up to next power of 2 for efficiency
            let new_size = min_bytes.next_power_of_two();
            self.buffer.resize(new_size, 0);
        }

        // Read enough data to satisfy the request
        let bytes_to_read = std::cmp::min(
            self.buffer.len() - self.buffer_len,
            remaining_in_wal,
        );

        if bytes_to_read > 0 {
            let read_offset = self.current_offset + self.buffer_len as u64;
            let data = self.backend.read_at(read_offset, bytes_to_read)?;
            self.buffer[self.buffer_len..self.buffer_len + data.len()].copy_from_slice(&data);
            self.buffer_len += data.len();
        }

        Ok(self.buffer_len - self.buffer_pos >= min_bytes)
    }

    /// Reads the next record from the WAL.
    ///
    /// Returns `Ok(Some((offset, record)))` for a valid record,
    /// `Ok(None)` at end of WAL or on truncated record,
    /// `Err(...)` on corruption or I/O error.
    fn read_next_record(&mut self) -> CoreResult<Option<(u64, WalRecord)>> {
        if self.finished {
            return Ok(None);
        }

        // Record the offset before we start
        let record_start_offset = self.current_offset;

        // Try to read header
        if !self.ensure_buffered(HEADER_SIZE)? {
            // Incomplete header - truncated WAL, treat as end
            self.finished = true;
            return Ok(None);
        }

        let header = &self.buffer[self.buffer_pos..self.buffer_pos + HEADER_SIZE];

        // Validate magic
        if header[0..4] != WAL_MAGIC {
            self.finished = true;
            return Err(CoreError::wal_corruption(format!(
                "invalid magic at offset {record_start_offset}"
            )));
        }

        // Check version
        let version = u16::from_le_bytes([header[4], header[5]]);
        if version > WAL_VERSION {
            self.finished = true;
            return Err(CoreError::wal_corruption(format!(
                "unsupported version {version} at offset {record_start_offset}"
            )));
        }

        // Record type
        let type_byte = header[6];
        let record_type = WalRecordType::from_byte(type_byte).ok_or_else(|| {
            self.finished = true;
            CoreError::wal_corruption(format!(
                "unknown record type {type_byte} at offset {record_start_offset}"
            ))
        })?;

        // Payload length
        let payload_len =
            u32::from_le_bytes([header[7], header[8], header[9], header[10]]) as usize;

        // Calculate total record length
        let total_len = HEADER_SIZE + payload_len + CRC_SIZE;

        // Check if we have the full record
        if !self.ensure_buffered(total_len)? {
            // Incomplete record - truncated WAL, treat as end
            self.finished = true;
            return Ok(None);
        }

        // Extract payload and CRC from buffer
        let payload_start = self.buffer_pos + HEADER_SIZE;
        let payload_end = payload_start + payload_len;
        let crc_start = payload_end;

        let payload = &self.buffer[payload_start..payload_end];
        let stored_crc = u32::from_le_bytes([
            self.buffer[crc_start],
            self.buffer[crc_start + 1],
            self.buffer[crc_start + 2],
            self.buffer[crc_start + 3],
        ]);

        // Verify CRC (over header + payload)
        let header_and_payload = &self.buffer[self.buffer_pos..payload_end];
        let computed_crc = compute_crc32(header_and_payload);

        if stored_crc != computed_crc {
            self.finished = true;
            return Err(CoreError::ChecksumMismatch {
                expected: stored_crc,
                actual: computed_crc,
            });
        }

        // Decode record
        let record = WalRecord::decode_payload(record_type, payload)?;

        // Advance position
        self.buffer_pos += total_len;
        self.current_offset += total_len as u64;

        Ok(Some((record_start_offset, record)))
    }
}

impl<'a> Iterator for WalRecordIterator<'a> {
    type Item = CoreResult<(u64, WalRecord)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.read_next_record() {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => None,
            Err(e) => {
                self.finished = true;
                Some(Err(e))
            }
        }
    }
}

/// A streaming recovery context for WAL replay.
///
/// This structure provides a memory-efficient way to replay WAL records
/// during database recovery. It tracks transaction states without storing
/// the entire WAL in memory.
///
/// # Design
///
/// For large WALs, we use a two-pass streaming approach:
/// 1. First pass: identify committed transactions (only store txids + sequences)
/// 2. Second pass: replay operations from committed transactions
///
/// This keeps memory usage proportional to the number of active transactions,
/// not the total WAL size.
pub struct StreamingRecovery {
    /// Committed transaction IDs and their sequence numbers.
    committed_txns: std::collections::HashMap<crate::types::TransactionId, crate::types::SequenceNumber>,
    /// Maximum transaction ID seen.
    max_txid: u64,
    /// Maximum sequence number seen.
    max_seq: u64,
    /// Committed sequence number (for MVCC visibility).
    committed_seq: u64,
}

impl StreamingRecovery {
    /// Creates a new streaming recovery context.
    ///
    /// # Arguments
    ///
    /// * `checkpoint_seq` - Sequence number from the last checkpoint (from manifest)
    pub fn new(checkpoint_seq: u64) -> Self {
        Self {
            committed_txns: std::collections::HashMap::new(),
            max_txid: 0,
            max_seq: checkpoint_seq,
            committed_seq: checkpoint_seq,
        }
    }

    /// First pass: scan WAL to identify committed transactions.
    ///
    /// This pass only stores transaction IDs and their commit sequences,
    /// not the actual operation data.
    pub fn scan_committed<'a, I>(&mut self, iter: I) -> CoreResult<()>
    where
        I: Iterator<Item = CoreResult<(u64, WalRecord)>>,
    {
        use crate::wal::WalRecord;

        for result in iter {
            let (_, record) = result?;

            if let Some(txid) = record.txid() {
                self.max_txid = self.max_txid.max(txid.as_u64());
            }

            match &record {
                WalRecord::Commit { txid, sequence } => {
                    self.committed_txns.insert(*txid, *sequence);
                    self.max_seq = self.max_seq.max(sequence.as_u64());
                    self.committed_seq = self.committed_seq.max(sequence.as_u64());
                }
                WalRecord::Checkpoint { sequence } => {
                    self.max_seq = self.max_seq.max(sequence.as_u64());
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Checks if a transaction was committed.
    ///
    /// This is part of the public recovery API for use by external tools
    /// such as diagnostics, debugging, or custom recovery logic.
    #[must_use]
    #[allow(dead_code)] // Public API for external use
    pub fn is_committed(&self, txid: &crate::types::TransactionId) -> bool {
        self.committed_txns.contains_key(txid)
    }

    /// Gets the commit sequence for a transaction.
    #[must_use]
    pub fn get_commit_sequence(&self, txid: &crate::types::TransactionId) -> Option<crate::types::SequenceNumber> {
        self.committed_txns.get(txid).copied()
    }

    /// Returns the next transaction ID to use.
    #[must_use]
    pub fn next_txid(&self) -> u64 {
        self.max_txid + 1
    }

    /// Returns the next sequence number to use.
    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.max_seq + 1
    }

    /// Returns the last committed sequence number (for MVCC).
    #[must_use]
    pub fn committed_seq(&self) -> u64 {
        self.committed_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CollectionId, SequenceNumber, TransactionId};
    use crate::wal::WalManager;
    use entidb_storage::InMemoryBackend;

    fn create_wal_with_records(records: &[WalRecord]) -> WalManager {
        let wal = WalManager::new(Box::new(InMemoryBackend::new()), false);
        for record in records {
            wal.append(record).unwrap();
        }
        wal
    }

    #[test]
    fn iterator_empty_wal() {
        let wal = WalManager::new(Box::new(InMemoryBackend::new()), false);
        let records: Vec<_> = wal.iter().unwrap().collect();
        assert!(records.is_empty());
    }

    #[test]
    fn iterator_single_record() {
        let record = WalRecord::Begin {
            txid: TransactionId::new(1),
        };
        let wal = create_wal_with_records(&[record.clone()]);

        let records: Vec<_> = wal.iter().unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].1, record);
    }

    #[test]
    fn iterator_multiple_records() {
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

        let wal = create_wal_with_records(&[r1.clone(), r2.clone(), r3.clone()]);

        let records: Vec<_> = wal.iter().unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].1, r1);
        assert_eq!(records[1].1, r2);
        assert_eq!(records[2].1, r3);
    }

    #[test]
    fn iterator_matches_read_all() {
        // Create a WAL with many records
        let mut records = Vec::new();
        for i in 0..100 {
            records.push(WalRecord::Begin {
                txid: TransactionId::new(i),
            });
            records.push(WalRecord::Put {
                txid: TransactionId::new(i),
                collection_id: CollectionId::new(1),
                entity_id: [i as u8; 16],
                before_hash: None,
                after_bytes: vec![i as u8; 50],
            });
            records.push(WalRecord::Commit {
                txid: TransactionId::new(i),
                sequence: SequenceNumber::new(i),
            });
        }

        let wal = create_wal_with_records(&records);

        // Compare iterator results with read_all
        let iter_records: Vec<_> = wal.iter().unwrap().map(|r| r.unwrap()).collect();
        let all_records = wal.read_all().unwrap();

        assert_eq!(iter_records.len(), all_records.len());
        for (iter_rec, all_rec) in iter_records.iter().zip(all_records.iter()) {
            assert_eq!(iter_rec.0, all_rec.0); // Same offset
            assert_eq!(iter_rec.1, all_rec.1); // Same record
        }
    }

    #[test]
    fn streaming_recovery_identifies_committed() {
        let records = vec![
            WalRecord::Begin {
                txid: TransactionId::new(1),
            },
            WalRecord::Put {
                txid: TransactionId::new(1),
                collection_id: CollectionId::new(1),
                entity_id: [1; 16],
                before_hash: None,
                after_bytes: vec![1, 2, 3],
            },
            WalRecord::Commit {
                txid: TransactionId::new(1),
                sequence: SequenceNumber::new(1),
            },
            // Uncommitted transaction
            WalRecord::Begin {
                txid: TransactionId::new(2),
            },
            WalRecord::Put {
                txid: TransactionId::new(2),
                collection_id: CollectionId::new(1),
                entity_id: [2; 16],
                before_hash: None,
                after_bytes: vec![4, 5, 6],
            },
            // No commit for txn 2
        ];

        let wal = create_wal_with_records(&records);
        let mut recovery = StreamingRecovery::new(0);
        recovery.scan_committed(wal.iter().unwrap()).unwrap();

        assert!(recovery.is_committed(&TransactionId::new(1)));
        assert!(!recovery.is_committed(&TransactionId::new(2)));
        assert_eq!(
            recovery.get_commit_sequence(&TransactionId::new(1)),
            Some(SequenceNumber::new(1))
        );
        assert_eq!(recovery.next_txid(), 3);
        assert_eq!(recovery.next_seq(), 2);
    }

    #[test]
    fn streaming_recovery_with_checkpoint() {
        let recovery = StreamingRecovery::new(100);
        
        // Even with empty WAL, checkpoint seq is preserved
        assert_eq!(recovery.committed_seq(), 100);
        assert_eq!(recovery.next_seq(), 101);
    }

    #[test]
    fn iterator_large_record() {
        // Test with a record larger than the read buffer
        let large_payload = vec![0xAB; 128 * 1024]; // 128 KB
        let record = WalRecord::Put {
            txid: TransactionId::new(1),
            collection_id: CollectionId::new(1),
            entity_id: [1; 16],
            before_hash: None,
            after_bytes: large_payload.clone(),
        };

        let wal = create_wal_with_records(&[record.clone()]);

        let records: Vec<_> = wal.iter().unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 1);
        
        if let WalRecord::Put { after_bytes, .. } = &records[0].1 {
            assert_eq!(after_bytes.len(), large_payload.len());
            assert_eq!(after_bytes, &large_payload);
        } else {
            panic!("Expected Put record");
        }
    }

    #[test]
    fn for_each_streaming_early_exit() {
        let mut records = Vec::new();
        for i in 0..100 {
            records.push(WalRecord::Begin {
                txid: TransactionId::new(i),
            });
        }

        let wal = create_wal_with_records(&records);

        let mut count = 0;
        wal.for_each_streaming(|_, _| {
            count += 1;
            Ok(count < 5)
        })
        .unwrap();

        assert_eq!(count, 5);
    }

    // ==========================================================================
    // Recovery policy tests: verify truncation vs corruption handling
    // ==========================================================================

    #[test]
    fn recovery_policy_truncated_header_tolerated() {
        // Regression test: truncated header (incomplete magic/version/type/length)
        // should be treated as clean end-of-log, not corruption.
        //
        // This represents a crash that occurred mid-write before the header
        // was fully written. The partial bytes are safely discarded.

        // Create a WAL with one complete record
        let record = WalRecord::Begin {
            txid: TransactionId::new(42),
        };
        let wal = create_wal_with_records(&[record.clone()]);

        // Get the raw bytes and append incomplete header bytes (5 bytes < 11 header)
        let backend = wal.get_backend_for_testing();
        {
            let mut guard = backend.lock();
            // Append partial header: just magic + 1 byte (5 bytes total, less than 11)
            guard.append(&[0xDB, 0xED, 0x01, 0x01, 0x00]).unwrap();
        }

        // Iterator should return the valid record then Ok(None), not an error
        let records: Vec<_> = wal.iter().unwrap().collect();
        assert_eq!(records.len(), 1, "Should return exactly one record");
        assert!(records[0].is_ok(), "The valid record should parse successfully");
        assert_eq!(records[0].as_ref().unwrap().1, record);
    }

    #[test]
    fn recovery_policy_truncated_payload_tolerated() {
        // Regression test: complete header but truncated payload
        // should be treated as clean end-of-log, not corruption.
        //
        // This represents a crash after the header was written but before
        // the full payload + CRC was fsynced.

        let record = WalRecord::Begin {
            txid: TransactionId::new(1),
        };
        let wal = create_wal_with_records(&[record.clone()]);

        // Append a valid header that claims 1000 bytes of payload, but only write 10
        let backend = wal.get_backend_for_testing();
        {
            let mut guard = backend.lock();
            // magic (4) + version (2) + type (1) + length (4) = 11 bytes header
            // Type 0x01 = Begin, length = 1000 (but we only write 10 payload bytes)
            // WAL_MAGIC = b"EWAL" = [0x45, 0x57, 0x41, 0x4C]
            #[rustfmt::skip]
            let incomplete_record: &[u8] = &[
                b'E', b'W', b'A', b'L',  // magic = "EWAL"
                0x01, 0x00,              // version = 1 (little-endian)
                0x01,                    // type = Begin
                0xE8, 0x03, 0x00, 0x00,  // length = 1000 (little-endian)
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, // only 10 bytes
            ];
            guard.append(incomplete_record).unwrap();
        }

        // Iterator should return the valid record then Ok(None) for the truncated one
        let records: Vec<_> = wal.iter().unwrap().collect();
        assert_eq!(records.len(), 1, "Should return exactly one record");
        assert!(records[0].is_ok(), "The valid record should parse successfully");
    }

    #[test]
    fn recovery_policy_crc_mismatch_is_fatal() {
        // Regression test: CRC mismatch MUST return an error, not be tolerated.
        //
        // Unlike truncation (crash mid-write), a CRC mismatch indicates actual
        // data corruption (bit rot, storage failure, etc.) and the database
        // MUST NOT open to prevent silent data loss.

        let record = WalRecord::Begin {
            txid: TransactionId::new(1),
        };
        let wal = create_wal_with_records(&[record]);

        // Append a record with invalid CRC
        let backend = wal.get_backend_for_testing();
        {
            let mut guard = backend.lock();
            // Complete record with wrong CRC:
            // magic (4) + version (2) + type (1) + length (4) + payload (8 for Begin) + CRC (4)
            // Begin payload: txid as u64 = 999
            // WAL_MAGIC = b"EWAL" = [0x45, 0x57, 0x41, 0x4C]
            #[rustfmt::skip]
            let bad_crc_record: &[u8] = &[
                b'E', b'W', b'A', b'L',  // magic = "EWAL"
                0x01, 0x00,              // version = 1
                0x01,                    // type = Begin
                0x08, 0x00, 0x00, 0x00,  // length = 8 (txid is u64)
                0xE7, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // txid = 999
                0xDE, 0xAD, 0xBE, 0xEF,  // WRONG CRC - this should fail
            ];
            guard.append(bad_crc_record).unwrap();
        }

        // Iterator should return the first valid record, then error on the corrupted one
        let mut iter = wal.iter().unwrap();

        // First record should be OK
        let first = iter.next();
        assert!(first.is_some());
        assert!(first.unwrap().is_ok(), "First record should be valid");

        // Second record should be an error (CRC mismatch)
        let second = iter.next();
        assert!(second.is_some(), "Should attempt to read second record");
        let err = second.unwrap();
        assert!(err.is_err(), "CRC mismatch must be an error, not tolerated");

        // Verify it's specifically a ChecksumMismatch error
        match err {
            Err(CoreError::ChecksumMismatch { .. }) => {
                // Expected - this is the correct behavior
            }
            Err(other) => {
                panic!("Expected ChecksumMismatch error, got: {:?}", other);
            }
            Ok(_) => {
                panic!("CRC mismatch was incorrectly tolerated - this is a bug!");
            }
        }
    }

    #[test]
    fn recovery_policy_invalid_magic_is_fatal() {
        // Invalid magic bytes should be a fatal error, not tolerated

        let record = WalRecord::Begin {
            txid: TransactionId::new(1),
        };
        let wal = create_wal_with_records(&[record]);

        let backend = wal.get_backend_for_testing();
        {
            let mut guard = backend.lock();
            // Record with invalid magic (not "EWAL")
            #[rustfmt::skip]
            let bad_magic: &[u8] = &[
                0xBA, 0xD0, 0x00, 0x00,  // WRONG magic (should be b"EWAL")
                0x01, 0x00,              // version = 1
                0x01,                    // type = Begin
                0x08, 0x00, 0x00, 0x00,  // length = 8
                0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // txid = 1
                0x00, 0x00, 0x00, 0x00,  // some CRC (doesn't matter, magic check first)
            ];
            guard.append(bad_magic).unwrap();
        }

        let mut iter = wal.iter().unwrap();
        let _ = iter.next(); // First valid record

        let second = iter.next();
        assert!(second.is_some());
        assert!(second.unwrap().is_err(), "Invalid magic must be an error");
    }

    #[test]
    fn recovery_uncommitted_txn_not_applied_after_truncation() {
        // Integration test: verify that truncation at the tail correctly
        // results in uncommitted transactions being excluded from recovery.
        //
        // Scenario:
        // 1. Write: BEGIN(1), PUT(1), COMMIT(1) - complete transaction
        // 2. Write: BEGIN(2), PUT(2) - incomplete transaction (no COMMIT)
        // 3. Truncate the WAL mid-way through transaction 2
        //
        // Recovery should only see transaction 1 as committed.

        let records = vec![
            WalRecord::Begin {
                txid: TransactionId::new(1),
            },
            WalRecord::Put {
                txid: TransactionId::new(1),
                collection_id: CollectionId::new(1),
                entity_id: [1; 16],
                before_hash: None,
                after_bytes: vec![0xAA, 0xBB, 0xCC],
            },
            WalRecord::Commit {
                txid: TransactionId::new(1),
                sequence: SequenceNumber::new(1),
            },
            // Uncommitted transaction 2
            WalRecord::Begin {
                txid: TransactionId::new(2),
            },
            WalRecord::Put {
                txid: TransactionId::new(2),
                collection_id: CollectionId::new(1),
                entity_id: [2; 16],
                before_hash: None,
                after_bytes: vec![0xDD, 0xEE, 0xFF],
            },
            // No COMMIT for transaction 2
        ];

        let wal = create_wal_with_records(&records);

        // Append truncated bytes to simulate crash mid-write of next record
        let backend = wal.get_backend_for_testing();
        {
            let mut guard = backend.lock();
            // Partial header
            guard.append(&[0xDB, 0xED, 0x01]).unwrap();
        }

        // Use StreamingRecovery to identify committed transactions
        let mut recovery = StreamingRecovery::new(0);
        recovery.scan_committed(wal.iter().unwrap()).unwrap();

        // Transaction 1 should be committed, transaction 2 should NOT
        assert!(
            recovery.is_committed(&TransactionId::new(1)),
            "Transaction 1 should be committed"
        );
        assert!(
            !recovery.is_committed(&TransactionId::new(2)),
            "Transaction 2 should NOT be committed (missing COMMIT)"
        );

        // Verify we can determine the correct next transaction ID
        assert_eq!(recovery.next_txid(), 3, "Next txid should be 3");
        assert_eq!(recovery.next_seq(), 2, "Next sequence should be 2");
    }
}
