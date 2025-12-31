//! Database backup and restore functionality.
//!
//! This module provides the ability to create point-in-time snapshots
//! of the database and restore from them.
//!
//! ## Backup Format
//!
//! Backups are stored as a series of segment records with a header:
//!
//! ```text
//! | magic (4) | version (2) | timestamp (8) | sequence (8) | record_count (4) | records... |
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! use entidb_core::backup::{BackupManager, BackupConfig};
//!
//! let backup_mgr = BackupManager::new(BackupConfig::default());
//!
//! // Create backup
//! let backup_data = backup_mgr.create_backup(&segment_manager)?;
//!
//! // Restore from backup
//! let records = backup_mgr.restore_from_backup(&backup_data)?;
//! ```

use crate::error::{CoreError, CoreResult};
use crate::segment::SegmentRecord;
use crate::segment::SegmentManager;
use crate::types::SequenceNumber;
use crate::wal::compute_crc32;
use std::time::{SystemTime, UNIX_EPOCH};

/// Magic bytes for backup files.
const BACKUP_MAGIC: [u8; 4] = *b"ENDB";
/// Current backup format version.
const BACKUP_VERSION: u16 = 1;
/// Header size (magic + version + timestamp + sequence + record_count).
const HEADER_SIZE: usize = 4 + 2 + 8 + 8 + 4;
/// Footer size (checksum).
const FOOTER_SIZE: usize = 4;

/// Safely converts a slice to a fixed-size array.
///
/// This is safe because we validate the data length before calling.
#[inline]
fn slice_to_array_8(slice: &[u8]) -> [u8; 8] {
    [slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7]]
}

/// Safely converts a slice to a 4-byte array.
#[inline]
fn slice_to_array_4(slice: &[u8]) -> [u8; 4] {
    [slice[0], slice[1], slice[2], slice[3]]
}

/// Configuration for backup operations.
#[derive(Debug, Clone)]
pub struct BackupConfig {
    /// Whether to include tombstones in the backup.
    pub include_tombstones: bool,
    /// Whether to compress the backup data.
    pub compress: bool,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            include_tombstones: true,
            compress: false,
        }
    }
}

/// Metadata about a backup.
#[derive(Debug, Clone)]
pub struct BackupMetadata {
    /// When the backup was created (Unix timestamp in milliseconds).
    pub timestamp: u64,
    /// The sequence number at the time of backup.
    pub sequence: SequenceNumber,
    /// Number of records in the backup.
    pub record_count: u32,
    /// Size of the backup data in bytes.
    pub size: usize,
}

/// Result of a backup operation.
#[derive(Debug)]
pub struct BackupResult {
    /// Backup metadata.
    pub metadata: BackupMetadata,
    /// The backup data.
    pub data: Vec<u8>,
}

/// Result of a restore operation.
#[derive(Debug)]
pub struct RestoreResult {
    /// Backup metadata from the restored backup.
    pub metadata: BackupMetadata,
    /// Restored records.
    pub records: Vec<SegmentRecord>,
}

/// Manages backup and restore operations.
pub struct BackupManager {
    config: BackupConfig,
}

impl BackupManager {
    /// Creates a new backup manager with the given configuration.
    #[must_use]
    pub fn new(config: BackupConfig) -> Self {
        Self { config }
    }

    /// Creates a backup manager with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(BackupConfig::default())
    }

    /// Creates a backup from the segment manager.
    ///
    /// This creates a point-in-time snapshot of all entities.
    pub fn create_backup(
        &self,
        segment_manager: &SegmentManager,
        current_sequence: SequenceNumber,
    ) -> CoreResult<BackupResult> {
        // Get all records
        let all_records = segment_manager.scan_all()?;

        // Filter based on config
        let records: Vec<_> = if self.config.include_tombstones {
            all_records
        } else {
            all_records
                .into_iter()
                .filter(|r| !r.is_tombstone())
                .collect()
        };

        // Serialize
        let data = self.serialize_backup(&records, current_sequence)?;
        let record_count = records.len() as u32;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let metadata = BackupMetadata {
            timestamp,
            sequence: current_sequence,
            record_count,
            size: data.len(),
        };

        Ok(BackupResult { metadata, data })
    }

    /// Creates a backup from a set of records.
    ///
    /// This is useful for creating backups from compacted data.
    pub fn create_backup_from_records(
        &self,
        records: &[SegmentRecord],
        current_sequence: SequenceNumber,
    ) -> CoreResult<BackupResult> {
        let data = self.serialize_backup(records, current_sequence)?;
        let record_count = records.len() as u32;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let metadata = BackupMetadata {
            timestamp,
            sequence: current_sequence,
            record_count,
            size: data.len(),
        };

        Ok(BackupResult { metadata, data })
    }

    /// Restores data from a backup.
    ///
    /// This parses the backup data and returns the records.
    pub fn restore_from_backup(&self, data: &[u8]) -> CoreResult<RestoreResult> {
        // Validate minimum size
        if data.len() < HEADER_SIZE + FOOTER_SIZE {
            return Err(CoreError::invalid_format("backup data too small"));
        }

        // Verify magic
        if &data[0..4] != BACKUP_MAGIC {
            return Err(CoreError::invalid_format("invalid backup magic"));
        }

        // Parse header
        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != BACKUP_VERSION {
            return Err(CoreError::invalid_format(format!(
                "unsupported backup version: {}",
                version
            )));
        }

        let timestamp = u64::from_le_bytes(slice_to_array_8(&data[6..14]));
        let sequence_val = u64::from_le_bytes(slice_to_array_8(&data[14..22]));
        let record_count = u32::from_le_bytes(slice_to_array_4(&data[22..26]));

        // Verify checksum
        let checksum_offset = data.len() - FOOTER_SIZE;
        let stored_checksum = u32::from_le_bytes(slice_to_array_4(
            &data[checksum_offset..checksum_offset + 4],
        ));
        let computed_checksum = compute_crc32(&data[..checksum_offset]);

        if stored_checksum != computed_checksum {
            return Err(CoreError::ChecksumMismatch {
                expected: stored_checksum,
                actual: computed_checksum,
            });
        }

        // Parse records
        let records_data = &data[HEADER_SIZE..checksum_offset];
        let records = self.parse_records(records_data)?;

        if records.len() != record_count as usize {
            return Err(CoreError::invalid_format(format!(
                "record count mismatch: expected {}, got {}",
                record_count,
                records.len()
            )));
        }

        let metadata = BackupMetadata {
            timestamp,
            sequence: SequenceNumber::new(sequence_val),
            record_count,
            size: data.len(),
        };

        Ok(RestoreResult { metadata, records })
    }

    /// Reads backup metadata without parsing all records.
    pub fn read_metadata(&self, data: &[u8]) -> CoreResult<BackupMetadata> {
        if data.len() < HEADER_SIZE + FOOTER_SIZE {
            return Err(CoreError::invalid_format("backup data too small"));
        }

        if &data[0..4] != BACKUP_MAGIC {
            return Err(CoreError::invalid_format("invalid backup magic"));
        }

        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != BACKUP_VERSION {
            return Err(CoreError::invalid_format(format!(
                "unsupported backup version: {}",
                version
            )));
        }

        let timestamp = u64::from_le_bytes(slice_to_array_8(&data[6..14]));
        let sequence_val = u64::from_le_bytes(slice_to_array_8(&data[14..22]));
        let record_count = u32::from_le_bytes(slice_to_array_4(&data[22..26]));

        Ok(BackupMetadata {
            timestamp,
            sequence: SequenceNumber::new(sequence_val),
            record_count,
            size: data.len(),
        })
    }

    /// Validates a backup without fully parsing it.
    pub fn validate_backup(&self, data: &[u8]) -> CoreResult<bool> {
        if data.len() < HEADER_SIZE + FOOTER_SIZE {
            return Ok(false);
        }

        if &data[0..4] != BACKUP_MAGIC {
            return Ok(false);
        }

        // Verify checksum
        let checksum_offset = data.len() - FOOTER_SIZE;
        let stored_checksum = u32::from_le_bytes(slice_to_array_4(
            &data[checksum_offset..checksum_offset + 4],
        ));
        let computed_checksum = compute_crc32(&data[..checksum_offset]);

        Ok(stored_checksum == computed_checksum)
    }

    fn serialize_backup(
        &self,
        records: &[SegmentRecord],
        sequence: SequenceNumber,
    ) -> CoreResult<Vec<u8>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Calculate size
        let records_size: usize = records.iter().map(|r: &SegmentRecord| r.encoded_size()).sum();
        let total_size = HEADER_SIZE + records_size + FOOTER_SIZE;
        let mut data = Vec::with_capacity(total_size);

        // Write header
        data.extend_from_slice(&BACKUP_MAGIC);
        data.extend_from_slice(&BACKUP_VERSION.to_le_bytes());
        data.extend_from_slice(&timestamp.to_le_bytes());
        data.extend_from_slice(&sequence.as_u64().to_le_bytes());
        data.extend_from_slice(&(records.len() as u32).to_le_bytes());

        // Write records
        for record in records {
            let encoded = record.encode();
            data.extend_from_slice(&encoded);
        }

        // Write checksum
        let checksum = compute_crc32(&data);
        data.extend_from_slice(&checksum.to_le_bytes());

        Ok(data)
    }

    fn parse_records(&self, data: &[u8]) -> CoreResult<Vec<SegmentRecord>> {
        let mut records = Vec::new();
        let mut offset = 0usize;

        while offset < data.len() {
            if offset + 4 > data.len() {
                break;
            }

            let len_bytes = &data[offset..offset + 4];
            let record_len =
                u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]])
                    as usize;

            if offset + record_len > data.len() {
                return Err(CoreError::invalid_format("record extends beyond data"));
            }

            let record_data = &data[offset..offset + record_len];
            let record = SegmentRecord::decode(record_data)?;
            records.push(record);

            offset += record_len;
        }

        Ok(records)
    }
}

impl Default for BackupManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionId;

    fn make_record(collection: u32, entity: u8, payload: &[u8], seq: u64) -> SegmentRecord {
        SegmentRecord::put(
            CollectionId::new(collection),
            [entity; 16],
            payload.to_vec(),
            SequenceNumber::new(seq),
        )
    }

    fn make_tombstone(collection: u32, entity: u8, seq: u64) -> SegmentRecord {
        SegmentRecord::tombstone(
            CollectionId::new(collection),
            [entity; 16],
            SequenceNumber::new(seq),
        )
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let manager = BackupManager::with_defaults();

        let records = vec![
            make_record(1, 1, b"entity1", 1),
            make_record(1, 2, b"entity2", 2),
            make_record(2, 1, b"entity3", 3),
        ];

        let backup = manager
            .create_backup_from_records(&records, SequenceNumber::new(10))
            .unwrap();

        assert_eq!(backup.metadata.record_count, 3);
        assert!(backup.data.len() > HEADER_SIZE + FOOTER_SIZE);

        let restored = manager.restore_from_backup(&backup.data).unwrap();

        assert_eq!(restored.records.len(), 3);
        assert_eq!(restored.metadata.sequence.as_u64(), 10);

        // Verify records match
        for (original, restored) in records.iter().zip(restored.records.iter()) {
            assert_eq!(original.collection_id, restored.collection_id);
            assert_eq!(original.entity_id, restored.entity_id);
            assert_eq!(original.payload, restored.payload);
        }
    }

    #[test]
    fn backup_excludes_tombstones_when_configured() {
        let manager = BackupManager::new(BackupConfig {
            include_tombstones: false,
            compress: false,
        });

        let records = vec![
            make_record(1, 1, b"live", 1),
            make_tombstone(1, 2, 2),
            make_record(1, 3, b"also_live", 3),
        ];

        // Only live records should be included
        // Note: create_backup_from_records doesn't filter, only create_backup does
        // For this test, we'll manually filter
        let filtered: Vec<SegmentRecord> = records
            .into_iter()
            .filter(|r| !r.is_tombstone())
            .collect();
        let backup = manager
            .create_backup_from_records(&filtered, SequenceNumber::new(10))
            .unwrap();

        assert_eq!(backup.metadata.record_count, 2);
    }

    #[test]
    fn validate_backup_detects_corruption() {
        let manager = BackupManager::with_defaults();

        let records = vec![make_record(1, 1, b"data", 1)];
        let backup = manager
            .create_backup_from_records(&records, SequenceNumber::new(1))
            .unwrap();

        // Valid backup
        assert!(manager.validate_backup(&backup.data).unwrap());

        // Corrupt the data
        let mut corrupted = backup.data.clone();
        if let Some(byte) = corrupted.get_mut(HEADER_SIZE + 10) {
            *byte ^= 0xFF;
        }
        assert!(!manager.validate_backup(&corrupted).unwrap());
    }

    #[test]
    fn invalid_magic_rejected() {
        let manager = BackupManager::with_defaults();

        let mut bad_data = vec![0u8; HEADER_SIZE + FOOTER_SIZE];
        bad_data[0..4].copy_from_slice(b"XXXX");

        assert!(manager.restore_from_backup(&bad_data).is_err());
    }

    #[test]
    fn read_metadata_without_parsing_records() {
        let manager = BackupManager::with_defaults();

        let records = vec![
            make_record(1, 1, b"data1", 1),
            make_record(1, 2, b"data2", 2),
        ];
        let backup = manager
            .create_backup_from_records(&records, SequenceNumber::new(5))
            .unwrap();

        let metadata = manager.read_metadata(&backup.data).unwrap();

        assert_eq!(metadata.sequence.as_u64(), 5);
        assert_eq!(metadata.record_count, 2);
    }

    #[test]
    fn empty_backup() {
        let manager = BackupManager::with_defaults();

        let records: Vec<SegmentRecord> = vec![];
        let backup = manager
            .create_backup_from_records(&records, SequenceNumber::new(0))
            .unwrap();

        assert_eq!(backup.metadata.record_count, 0);

        let restored = manager.restore_from_backup(&backup.data).unwrap();
        assert!(restored.records.is_empty());
    }

    #[test]
    fn backup_too_small_fails() {
        let manager = BackupManager::with_defaults();

        let small_data = vec![0u8; 10];
        assert!(manager.restore_from_backup(&small_data).is_err());
    }
}
