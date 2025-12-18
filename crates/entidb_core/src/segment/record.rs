//! Segment record types.

use crate::error::{CoreError, CoreResult};
use crate::types::{CollectionId, SequenceNumber};
use crate::wal::compute_crc32;

/// Flags for segment records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SegmentRecordFlags(u8);

impl SegmentRecordFlags {
    /// No flags set.
    pub const NONE: Self = Self(0);
    /// Record is a tombstone (entity deleted).
    pub const TOMBSTONE: Self = Self(0x01);
    /// Record payload is encrypted.
    pub const ENCRYPTED: Self = Self(0x02);

    /// Creates new flags from raw byte.
    #[must_use]
    pub const fn from_byte(b: u8) -> Self {
        Self(b)
    }

    /// Returns the raw byte value.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        self.0
    }

    /// Checks if tombstone flag is set.
    #[must_use]
    pub const fn is_tombstone(self) -> bool {
        self.0 & 0x01 != 0
    }

    /// Checks if encrypted flag is set.
    #[must_use]
    pub const fn is_encrypted(self) -> bool {
        self.0 & 0x02 != 0
    }

    /// Sets the tombstone flag.
    #[must_use]
    pub const fn with_tombstone(self) -> Self {
        Self(self.0 | 0x01)
    }
}

/// A record stored in a segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRecord {
    /// Collection this entity belongs to.
    pub collection_id: CollectionId,
    /// Entity identifier (128-bit UUID).
    pub entity_id: [u8; 16],
    /// Record flags.
    pub flags: SegmentRecordFlags,
    /// Entity payload (canonical CBOR bytes, or empty for tombstone).
    pub payload: Vec<u8>,
    /// Sequence number when this record was committed.
    pub sequence: SequenceNumber,
}

impl SegmentRecord {
    /// Header size: record_len (4) + collection_id (4) + entity_id (16) + flags (1) + sequence (8) = 33
    const HEADER_SIZE: usize = 33;
    /// CRC size.
    const CRC_SIZE: usize = 4;

    /// Creates a new put record.
    #[must_use]
    pub fn put(
        collection_id: CollectionId,
        entity_id: [u8; 16],
        payload: Vec<u8>,
        sequence: SequenceNumber,
    ) -> Self {
        Self {
            collection_id,
            entity_id,
            flags: SegmentRecordFlags::NONE,
            payload,
            sequence,
        }
    }

    /// Creates a tombstone record.
    #[must_use]
    pub fn tombstone(
        collection_id: CollectionId,
        entity_id: [u8; 16],
        sequence: SequenceNumber,
    ) -> Self {
        Self {
            collection_id,
            entity_id,
            flags: SegmentRecordFlags::TOMBSTONE,
            payload: Vec::new(),
            sequence,
        }
    }

    /// Returns whether this is a tombstone.
    #[must_use]
    pub fn is_tombstone(&self) -> bool {
        self.flags.is_tombstone()
    }

    /// Encodes the record to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let record_len = Self::HEADER_SIZE + self.payload.len() + Self::CRC_SIZE;
        let mut buf = Vec::with_capacity(record_len);

        // Record length (total including this field)
        buf.extend_from_slice(&(record_len as u32).to_le_bytes());

        // Collection ID
        buf.extend_from_slice(&self.collection_id.as_u32().to_le_bytes());

        // Entity ID
        buf.extend_from_slice(&self.entity_id);

        // Flags
        buf.push(self.flags.as_byte());

        // Sequence number
        buf.extend_from_slice(&self.sequence.as_u64().to_le_bytes());

        // Payload
        buf.extend_from_slice(&self.payload);

        // CRC32 (over everything before it)
        let crc = compute_crc32(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        buf
    }

    /// Decodes a record from bytes.
    pub fn decode(data: &[u8]) -> CoreResult<Self> {
        if data.len() < Self::HEADER_SIZE + Self::CRC_SIZE {
            return Err(CoreError::segment_corruption("record too short"));
        }

        // Parse header
        let record_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

        if data.len() < record_len {
            return Err(CoreError::segment_corruption("incomplete record"));
        }

        // Verify CRC
        let stored_crc = u32::from_le_bytes([
            data[record_len - 4],
            data[record_len - 3],
            data[record_len - 2],
            data[record_len - 1],
        ]);
        let computed_crc = compute_crc32(&data[..record_len - 4]);
        if stored_crc != computed_crc {
            return Err(CoreError::ChecksumMismatch {
                expected: stored_crc,
                actual: computed_crc,
            });
        }

        let collection_id =
            CollectionId::new(u32::from_le_bytes([data[4], data[5], data[6], data[7]]));

        let entity_id: [u8; 16] = data[8..24]
            .try_into()
            .map_err(|_| CoreError::segment_corruption("invalid entity_id"))?;

        let flags = SegmentRecordFlags::from_byte(data[24]);

        let sequence = SequenceNumber::new(u64::from_le_bytes([
            data[25], data[26], data[27], data[28], data[29], data[30], data[31], data[32],
        ]));

        let payload_len = record_len - Self::HEADER_SIZE - Self::CRC_SIZE;
        let payload = data[Self::HEADER_SIZE..Self::HEADER_SIZE + payload_len].to_vec();

        Ok(Self {
            collection_id,
            entity_id,
            flags,
            payload,
            sequence,
        })
    }

    /// Returns the encoded size of this record.
    #[must_use]
    pub fn encoded_size(&self) -> usize {
        Self::HEADER_SIZE + self.payload.len() + Self::CRC_SIZE
    }
}

/// A segment file containing entity records.
#[derive(Debug)]
pub struct Segment {
    /// Segment ID (monotonically increasing).
    pub id: u64,
    /// Whether this segment is sealed (immutable).
    pub sealed: bool,
    /// Current size in bytes.
    pub size: u64,
}

impl Segment {
    /// Creates a new segment.
    #[must_use]
    pub fn new(id: u64) -> Self {
        Self {
            id,
            sealed: false,
            size: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_record_flags() {
        let flags = SegmentRecordFlags::NONE;
        assert!(!flags.is_tombstone());
        assert!(!flags.is_encrypted());

        let tombstone = flags.with_tombstone();
        assert!(tombstone.is_tombstone());
        assert!(!tombstone.is_encrypted());
    }

    #[test]
    fn put_record_roundtrip() {
        let record = SegmentRecord::put(
            CollectionId::new(5),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            vec![0xCA, 0xFE, 0xBA, 0xBE],
            SequenceNumber::new(42),
        );

        let encoded = record.encode();
        let decoded = SegmentRecord::decode(&encoded).unwrap();

        assert_eq!(record, decoded);
    }

    #[test]
    fn tombstone_record_roundtrip() {
        let record =
            SegmentRecord::tombstone(CollectionId::new(10), [0xFF; 16], SequenceNumber::new(100));

        assert!(record.is_tombstone());

        let encoded = record.encode();
        let decoded = SegmentRecord::decode(&encoded).unwrap();

        assert_eq!(record, decoded);
        assert!(decoded.is_tombstone());
    }

    #[test]
    fn detect_corruption() {
        let record = SegmentRecord::put(
            CollectionId::new(1),
            [0; 16],
            vec![1, 2, 3],
            SequenceNumber::new(1),
        );

        let mut encoded = record.encode();
        // Corrupt a byte
        encoded[10] ^= 0xFF;

        let result = SegmentRecord::decode(&encoded);
        assert!(matches!(result, Err(CoreError::ChecksumMismatch { .. })));
    }

    #[test]
    fn encoded_size() {
        let record = SegmentRecord::put(
            CollectionId::new(1),
            [0; 16],
            vec![1, 2, 3, 4, 5],
            SequenceNumber::new(1),
        );

        assert_eq!(record.encoded_size(), record.encode().len());
    }
}
