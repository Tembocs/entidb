//! Segment compaction.
//!
//! Compaction merges multiple records to remove obsolete versions and
//! tombstones, reclaiming storage space. This module provides the
//! [`Compactor`] that performs this operation.
//!
//! ## Invariants
//!
//! - Compaction **MUST NOT** change logical state
//! - Latest committed version per (collection_id, entity_id) wins
//! - Tombstones older than retention window MAY be dropped
//! - Compaction produces a single output containing only live entities

use crate::error::{CoreError, CoreResult};
use crate::segment::record::SegmentRecord;
use crate::types::SequenceNumber;
use std::collections::HashMap;

/// Configuration for compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Minimum age (in sequence numbers) before tombstones can be removed.
    /// Set to 0 to remove all tombstones immediately.
    pub tombstone_retention: u64,
    /// Whether to preserve tombstones that are newer than retention period.
    pub preserve_recent_tombstones: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            tombstone_retention: 0,
            preserve_recent_tombstones: false,
        }
    }
}

impl CompactionConfig {
    /// Creates a config that removes all tombstones.
    #[must_use]
    pub fn remove_all_tombstones() -> Self {
        Self::default()
    }

    /// Creates a config that preserves tombstones for a certain number of sequences.
    #[must_use]
    pub fn with_tombstone_retention(sequences: u64) -> Self {
        Self {
            tombstone_retention: sequences,
            preserve_recent_tombstones: true,
        }
    }
}

/// Result of a compaction operation.
#[derive(Debug)]
pub struct CompactionResult {
    /// Number of records in the input.
    pub input_records: usize,
    /// Number of records in the output.
    pub output_records: usize,
    /// Number of tombstones removed.
    pub tombstones_removed: usize,
    /// Number of obsolete versions removed.
    pub obsolete_versions_removed: usize,
    /// Bytes saved (input size - output size).
    pub bytes_saved: usize,
}

/// Compactor for merging segment records.
///
/// The compactor takes a set of records and produces a deduplicated set
/// containing only the latest version of each entity.
///
/// ## Example
///
/// ```ignore
/// use entidb_core::segment::compaction::{Compactor, CompactionConfig};
///
/// let compactor = Compactor::new(CompactionConfig::default());
/// let (output, stats) = compactor.compact(records, current_sequence)?;
/// ```
pub struct Compactor {
    config: CompactionConfig,
}

impl Compactor {
    /// Creates a new compactor with the given configuration.
    #[must_use]
    pub fn new(config: CompactionConfig) -> Self {
        Self { config }
    }

    /// Creates a compactor with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(CompactionConfig::default())
    }

    /// Compacts a set of records, producing deduplicated output.
    ///
    /// # Arguments
    ///
    /// * `records` - Input records to compact
    /// * `current_sequence` - Current sequence number (for tombstone retention)
    ///
    /// # Returns
    ///
    /// A tuple of (compacted records, compaction statistics).
    pub fn compact(
        &self,
        records: Vec<SegmentRecord>,
        current_sequence: SequenceNumber,
    ) -> CoreResult<(Vec<SegmentRecord>, CompactionResult)> {
        let input_records = records.len();
        let input_size: usize = records.iter().map(|r| r.encoded_size()).sum();

        // Build a map of (collection_id, entity_id) -> latest record
        let mut latest: HashMap<(u32, [u8; 16]), SegmentRecord> = HashMap::new();
        let mut obsolete_count = 0usize;

        for record in records {
            let key = (record.collection_id.as_u32(), record.entity_id);

            let should_replace = match latest.get(&key) {
                Some(existing) => record.sequence > existing.sequence,
                None => true,
            };

            if should_replace {
                if latest.insert(key, record).is_some() {
                    obsolete_count += 1;
                }
            } else {
                obsolete_count += 1;
            }
        }

        // Filter out tombstones based on retention policy
        let current_seq = current_sequence.as_u64();
        let mut tombstones_removed = 0usize;
        let mut output: Vec<SegmentRecord> = Vec::with_capacity(latest.len());

        for (_, record) in latest {
            if record.is_tombstone() {
                let record_age = current_seq.saturating_sub(record.sequence.as_u64());

                // Keep tombstone if it's within retention period
                if self.config.preserve_recent_tombstones
                    && record_age < self.config.tombstone_retention
                {
                    output.push(record);
                } else {
                    tombstones_removed += 1;
                }
            } else {
                output.push(record);
            }
        }

        // Sort by (collection_id, entity_id) for deterministic output
        output.sort_by(|a, b| {
            let key_a = (a.collection_id.as_u32(), a.entity_id);
            let key_b = (b.collection_id.as_u32(), b.entity_id);
            key_a.cmp(&key_b)
        });

        let output_size: usize = output.iter().map(|r| r.encoded_size()).sum();

        let result = CompactionResult {
            input_records,
            output_records: output.len(),
            tombstones_removed,
            obsolete_versions_removed: obsolete_count,
            bytes_saved: input_size.saturating_sub(output_size),
        };

        Ok((output, result))
    }

    /// Compacts records from raw bytes.
    ///
    /// This is useful when reading from a segment file and writing to a new one.
    pub fn compact_bytes(
        &self,
        input: &[u8],
        current_sequence: SequenceNumber,
    ) -> CoreResult<(Vec<u8>, CompactionResult)> {
        // Parse all records
        let records = Self::parse_records(input)?;

        // Compact
        let (compacted, stats) = self.compact(records, current_sequence)?;

        // Serialize output
        let mut output = Vec::new();
        for record in &compacted {
            output.extend(record.encode()?);
        }

        Ok((output, stats))
    }

    /// Parses records from raw bytes.
    fn parse_records(data: &[u8]) -> CoreResult<Vec<SegmentRecord>> {
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
                return Err(CoreError::segment_corruption("record extends beyond data"));
            }

            let record_data = &data[offset..offset + record_len];
            let record = SegmentRecord::decode(record_data)?;
            records.push(record);

            offset += record_len;
        }

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CollectionId;

    fn make_put(collection: u32, entity: u8, payload: &[u8], seq: u64) -> SegmentRecord {
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
    fn compact_removes_older_versions() {
        let compactor = Compactor::with_defaults();

        let records = vec![
            make_put(1, 1, b"v1", 1),
            make_put(1, 1, b"v2", 2),
            make_put(1, 1, b"v3", 3),
        ];

        let (output, stats) = compactor.compact(records, SequenceNumber::new(10)).unwrap();

        assert_eq!(output.len(), 1);
        assert_eq!(output[0].payload, b"v3");
        assert_eq!(stats.obsolete_versions_removed, 2);
    }

    #[test]
    fn compact_removes_tombstones() {
        let compactor = Compactor::new(CompactionConfig::remove_all_tombstones());

        let records = vec![
            make_put(1, 1, b"data", 1),
            make_tombstone(1, 1, 2),
            make_put(1, 2, b"live", 3),
        ];

        let (output, stats) = compactor.compact(records, SequenceNumber::new(10)).unwrap();

        assert_eq!(output.len(), 1);
        assert_eq!(output[0].entity_id, [2; 16]);
        assert_eq!(stats.tombstones_removed, 1);
    }

    #[test]
    fn compact_preserves_recent_tombstones() {
        let compactor = Compactor::new(CompactionConfig::with_tombstone_retention(10));

        let records = vec![
            make_tombstone(1, 1, 5), // Old tombstone, should be removed
            make_tombstone(1, 2, 8), // Recent tombstone, should be kept
        ];

        // Current sequence is 10, retention is 10
        // Tombstone at seq 5 is 5 sequences old (< 10), kept
        // Tombstone at seq 8 is 2 sequences old (< 10), kept
        let (output, stats) = compactor.compact(records, SequenceNumber::new(10)).unwrap();

        assert_eq!(output.len(), 2);
        assert_eq!(stats.tombstones_removed, 0);

        // Now with current sequence 20
        // Tombstone at seq 5 is 15 sequences old (>= 10), removed
        // Tombstone at seq 8 is 12 sequences old (>= 10), removed
        let records2 = vec![make_tombstone(1, 1, 5), make_tombstone(1, 2, 8)];
        let (output2, stats2) = compactor
            .compact(records2, SequenceNumber::new(20))
            .unwrap();

        assert_eq!(output2.len(), 0);
        assert_eq!(stats2.tombstones_removed, 2);
    }

    #[test]
    fn compact_multiple_collections() {
        let compactor = Compactor::with_defaults();

        let records = vec![
            make_put(1, 1, b"c1e1", 1),
            make_put(2, 1, b"c2e1", 2),
            make_put(1, 2, b"c1e2", 3),
        ];

        let (output, _) = compactor.compact(records, SequenceNumber::new(10)).unwrap();

        assert_eq!(output.len(), 3);
    }

    #[test]
    fn compact_empty_input() {
        let compactor = Compactor::with_defaults();
        let (output, stats) = compactor.compact(vec![], SequenceNumber::new(1)).unwrap();

        assert!(output.is_empty());
        assert_eq!(stats.input_records, 0);
        assert_eq!(stats.output_records, 0);
    }

    #[test]
    fn compact_deterministic_order() {
        let compactor = Compactor::with_defaults();

        // Insert in random order
        let records = vec![
            make_put(2, 2, b"c2e2", 1),
            make_put(1, 1, b"c1e1", 2),
            make_put(1, 2, b"c1e2", 3),
            make_put(2, 1, b"c2e1", 4),
        ];

        let (output, _) = compactor.compact(records, SequenceNumber::new(10)).unwrap();

        // Should be sorted by (collection_id, entity_id)
        assert_eq!(output[0].collection_id.as_u32(), 1);
        assert_eq!(output[0].entity_id, [1; 16]);
        assert_eq!(output[1].collection_id.as_u32(), 1);
        assert_eq!(output[1].entity_id, [2; 16]);
        assert_eq!(output[2].collection_id.as_u32(), 2);
        assert_eq!(output[2].entity_id, [1; 16]);
        assert_eq!(output[3].collection_id.as_u32(), 2);
        assert_eq!(output[3].entity_id, [2; 16]);
    }
}
