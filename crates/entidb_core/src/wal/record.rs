//! WAL record types and serialization.

use crate::error::{CoreError, CoreResult};
use crate::types::{CollectionId, SequenceNumber, TransactionId};

/// Magic bytes identifying a WAL record.
pub const WAL_MAGIC: [u8; 4] = *b"EWAL";

/// Current WAL format version.
pub const WAL_VERSION: u16 = 1;

/// Type of WAL record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordType {
    /// Begin a new transaction.
    Begin = 1,
    /// Put (insert or update) an entity.
    Put = 2,
    /// Delete an entity.
    Delete = 3,
    /// Commit a transaction.
    Commit = 4,
    /// Abort a transaction.
    Abort = 5,
    /// Checkpoint marker.
    Checkpoint = 6,
}

impl WalRecordType {
    /// Converts a byte to a record type.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Begin),
            2 => Some(Self::Put),
            3 => Some(Self::Delete),
            4 => Some(Self::Commit),
            5 => Some(Self::Abort),
            6 => Some(Self::Checkpoint),
            _ => None,
        }
    }

    /// Converts the record type to a byte.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// A WAL record representing a database operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalRecord {
    /// Begin a new transaction.
    Begin {
        /// Transaction ID.
        txid: TransactionId,
    },

    /// Put (insert or update) an entity.
    Put {
        /// Transaction ID.
        txid: TransactionId,
        /// Collection containing the entity.
        collection_id: CollectionId,
        /// Entity identifier (128-bit UUID).
        entity_id: [u8; 16],
        /// Optional hash of previous value (for conflict detection).
        before_hash: Option<[u8; 32]>,
        /// New entity payload (canonical CBOR bytes).
        after_bytes: Vec<u8>,
    },

    /// Delete an entity.
    Delete {
        /// Transaction ID.
        txid: TransactionId,
        /// Collection containing the entity.
        collection_id: CollectionId,
        /// Entity identifier.
        entity_id: [u8; 16],
        /// Optional hash of previous value (for conflict detection).
        before_hash: Option<[u8; 32]>,
    },

    /// Commit a transaction.
    Commit {
        /// Transaction ID.
        txid: TransactionId,
        /// Sequence number assigned to this commit.
        sequence: SequenceNumber,
    },

    /// Abort a transaction.
    Abort {
        /// Transaction ID.
        txid: TransactionId,
    },

    /// Checkpoint marker for WAL truncation.
    Checkpoint {
        /// Sequence number at checkpoint.
        sequence: SequenceNumber,
    },
}

impl WalRecord {
    /// Returns the record type.
    #[must_use]
    pub fn record_type(&self) -> WalRecordType {
        match self {
            Self::Begin { .. } => WalRecordType::Begin,
            Self::Put { .. } => WalRecordType::Put,
            Self::Delete { .. } => WalRecordType::Delete,
            Self::Commit { .. } => WalRecordType::Commit,
            Self::Abort { .. } => WalRecordType::Abort,
            Self::Checkpoint { .. } => WalRecordType::Checkpoint,
        }
    }

    /// Returns the transaction ID if this record is associated with one.
    #[must_use]
    pub fn txid(&self) -> Option<TransactionId> {
        match self {
            Self::Begin { txid }
            | Self::Put { txid, .. }
            | Self::Delete { txid, .. }
            | Self::Commit { txid, .. }
            | Self::Abort { txid } => Some(*txid),
            Self::Checkpoint { .. } => None,
        }
    }

    /// Maximum size for entity payload in a WAL record.
    ///
    /// Payloads larger than this will be rejected with an error.
    /// This limit exists because the WAL format uses a 4-byte length field.
    pub const MAX_PAYLOAD_SIZE: usize = u32::MAX as usize;

    /// Serializes the record payload (without envelope).
    ///
    /// # Errors
    ///
    /// Returns an error if `after_bytes` in a `Put` record exceeds [`Self::MAX_PAYLOAD_SIZE`].
    /// This prevents creating malformed WAL records that cannot be correctly decoded.
    pub fn encode_payload(&self) -> CoreResult<Vec<u8>> {
        let mut buf = Vec::new();

        match self {
            Self::Begin { txid } | Self::Abort { txid } => {
                buf.extend_from_slice(&txid.as_u64().to_le_bytes());
            }

            Self::Put {
                txid,
                collection_id,
                entity_id,
                before_hash,
                after_bytes,
            } => {
                // Validate payload size before encoding to prevent corruption
                if after_bytes.len() > Self::MAX_PAYLOAD_SIZE {
                    return Err(CoreError::invalid_argument(format!(
                        "entity payload too large: {} bytes exceeds maximum of {} bytes",
                        after_bytes.len(),
                        Self::MAX_PAYLOAD_SIZE
                    )));
                }

                buf.extend_from_slice(&txid.as_u64().to_le_bytes());
                buf.extend_from_slice(&collection_id.as_u32().to_le_bytes());
                buf.extend_from_slice(entity_id);
                // before_hash: 1 byte flag + optional 32 bytes
                if let Some(hash) = before_hash {
                    buf.push(1);
                    buf.extend_from_slice(hash);
                } else {
                    buf.push(0);
                }
                // after_bytes: 4 byte length + data
                // Safe: we validated len <= u32::MAX above
                let len = after_bytes.len() as u32;
                buf.extend_from_slice(&len.to_le_bytes());
                buf.extend_from_slice(after_bytes);
            }

            Self::Delete {
                txid,
                collection_id,
                entity_id,
                before_hash,
            } => {
                buf.extend_from_slice(&txid.as_u64().to_le_bytes());
                buf.extend_from_slice(&collection_id.as_u32().to_le_bytes());
                buf.extend_from_slice(entity_id);
                if let Some(hash) = before_hash {
                    buf.push(1);
                    buf.extend_from_slice(hash);
                } else {
                    buf.push(0);
                }
            }

            Self::Commit { txid, sequence } => {
                buf.extend_from_slice(&txid.as_u64().to_le_bytes());
                buf.extend_from_slice(&sequence.as_u64().to_le_bytes());
            }

            Self::Checkpoint { sequence } => {
                buf.extend_from_slice(&sequence.as_u64().to_le_bytes());
            }
        }

        Ok(buf)
    }

    /// Deserializes a record from its type and payload.
    pub fn decode_payload(record_type: WalRecordType, payload: &[u8]) -> CoreResult<Self> {
        let mut cursor = 0;

        let read_u64 = |cursor: &mut usize| -> CoreResult<u64> {
            if *cursor + 8 > payload.len() {
                return Err(CoreError::wal_corruption("unexpected end of payload"));
            }
            let bytes: [u8; 8] = payload[*cursor..*cursor + 8]
                .try_into()
                .map_err(|_| CoreError::wal_corruption("invalid u64"))?;
            *cursor += 8;
            Ok(u64::from_le_bytes(bytes))
        };

        let read_u32 = |cursor: &mut usize| -> CoreResult<u32> {
            if *cursor + 4 > payload.len() {
                return Err(CoreError::wal_corruption("unexpected end of payload"));
            }
            let bytes: [u8; 4] = payload[*cursor..*cursor + 4]
                .try_into()
                .map_err(|_| CoreError::wal_corruption("invalid u32"))?;
            *cursor += 4;
            Ok(u32::from_le_bytes(bytes))
        };

        let read_entity_id = |cursor: &mut usize| -> CoreResult<[u8; 16]> {
            if *cursor + 16 > payload.len() {
                return Err(CoreError::wal_corruption("unexpected end of payload"));
            }
            let bytes: [u8; 16] = payload[*cursor..*cursor + 16]
                .try_into()
                .map_err(|_| CoreError::wal_corruption("invalid entity_id"))?;
            *cursor += 16;
            Ok(bytes)
        };

        let read_optional_hash = |cursor: &mut usize| -> CoreResult<Option<[u8; 32]>> {
            if *cursor >= payload.len() {
                return Err(CoreError::wal_corruption("unexpected end of payload"));
            }
            let has_hash = payload[*cursor] != 0;
            *cursor += 1;
            if has_hash {
                if *cursor + 32 > payload.len() {
                    return Err(CoreError::wal_corruption("unexpected end of hash"));
                }
                let bytes: [u8; 32] = payload[*cursor..*cursor + 32]
                    .try_into()
                    .map_err(|_| CoreError::wal_corruption("invalid hash"))?;
                *cursor += 32;
                Ok(Some(bytes))
            } else {
                Ok(None)
            }
        };

        match record_type {
            WalRecordType::Begin => {
                let txid = TransactionId::new(read_u64(&mut cursor)?);
                // Validate no trailing bytes for fixed-size record
                if cursor != payload.len() {
                    return Err(CoreError::wal_corruption(format!(
                        "trailing bytes in Begin record: expected {} bytes, got {}",
                        cursor,
                        payload.len()
                    )));
                }
                Ok(Self::Begin { txid })
            }

            WalRecordType::Put => {
                let txid = TransactionId::new(read_u64(&mut cursor)?);
                let collection_id = CollectionId::new(read_u32(&mut cursor)?);
                let entity_id = read_entity_id(&mut cursor)?;
                let before_hash = read_optional_hash(&mut cursor)?;
                let len = read_u32(&mut cursor)? as usize;
                if cursor + len > payload.len() {
                    return Err(CoreError::wal_corruption("unexpected end of after_bytes"));
                }
                let after_bytes = payload[cursor..cursor + len].to_vec();
                // Advance cursor past after_bytes for validation
                cursor += len;
                // Validate no trailing bytes
                if cursor != payload.len() {
                    return Err(CoreError::wal_corruption(format!(
                        "trailing bytes in Put record: expected {} bytes, got {}",
                        cursor,
                        payload.len()
                    )));
                }
                Ok(Self::Put {
                    txid,
                    collection_id,
                    entity_id,
                    before_hash,
                    after_bytes,
                })
            }

            WalRecordType::Delete => {
                let txid = TransactionId::new(read_u64(&mut cursor)?);
                let collection_id = CollectionId::new(read_u32(&mut cursor)?);
                let entity_id = read_entity_id(&mut cursor)?;
                let before_hash = read_optional_hash(&mut cursor)?;
                // Validate no trailing bytes
                if cursor != payload.len() {
                    return Err(CoreError::wal_corruption(format!(
                        "trailing bytes in Delete record: expected {} bytes, got {}",
                        cursor,
                        payload.len()
                    )));
                }
                Ok(Self::Delete {
                    txid,
                    collection_id,
                    entity_id,
                    before_hash,
                })
            }

            WalRecordType::Commit => {
                let txid = TransactionId::new(read_u64(&mut cursor)?);
                let sequence = SequenceNumber::new(read_u64(&mut cursor)?);
                // Validate no trailing bytes for fixed-size record
                if cursor != payload.len() {
                    return Err(CoreError::wal_corruption(format!(
                        "trailing bytes in Commit record: expected {} bytes, got {}",
                        cursor,
                        payload.len()
                    )));
                }
                Ok(Self::Commit { txid, sequence })
            }

            WalRecordType::Abort => {
                let txid = TransactionId::new(read_u64(&mut cursor)?);
                // Validate no trailing bytes for fixed-size record
                if cursor != payload.len() {
                    return Err(CoreError::wal_corruption(format!(
                        "trailing bytes in Abort record: expected {} bytes, got {}",
                        cursor,
                        payload.len()
                    )));
                }
                Ok(Self::Abort { txid })
            }

            WalRecordType::Checkpoint => {
                let sequence = SequenceNumber::new(read_u64(&mut cursor)?);
                // Validate no trailing bytes for fixed-size record
                if cursor != payload.len() {
                    return Err(CoreError::wal_corruption(format!(
                        "trailing bytes in Checkpoint record: expected {} bytes, got {}",
                        cursor,
                        payload.len()
                    )));
                }
                Ok(Self::Checkpoint { sequence })
            }
        }
    }
}

/// Computes CRC32 checksum for data.
pub fn compute_crc32(data: &[u8]) -> u32 {
    // Simple CRC32 implementation (IEEE polynomial)
    const CRC32_TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut j = 0;
            while j < 8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
                j += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };

    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        let index = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_type_roundtrip() {
        for t in [
            WalRecordType::Begin,
            WalRecordType::Put,
            WalRecordType::Delete,
            WalRecordType::Commit,
            WalRecordType::Abort,
            WalRecordType::Checkpoint,
        ] {
            assert_eq!(WalRecordType::from_byte(t.as_byte()), Some(t));
        }
    }

    #[test]
    fn begin_record_roundtrip() {
        let record = WalRecord::Begin {
            txid: TransactionId::new(42),
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Begin, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn put_record_roundtrip() {
        let record = WalRecord::Put {
            txid: TransactionId::new(1),
            collection_id: CollectionId::new(5),
            entity_id: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            before_hash: Some([0xAB; 32]),
            after_bytes: vec![0xCA, 0xFE, 0xBA, 0xBE],
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Put, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn put_record_no_hash() {
        let record = WalRecord::Put {
            txid: TransactionId::new(1),
            collection_id: CollectionId::new(5),
            entity_id: [0; 16],
            before_hash: None,
            after_bytes: vec![1, 2, 3],
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Put, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn delete_record_roundtrip() {
        let record = WalRecord::Delete {
            txid: TransactionId::new(99),
            collection_id: CollectionId::new(10),
            entity_id: [0xFF; 16],
            before_hash: None,
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Delete, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn commit_record_roundtrip() {
        let record = WalRecord::Commit {
            txid: TransactionId::new(7),
            sequence: SequenceNumber::new(100),
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Commit, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn abort_record_roundtrip() {
        let record = WalRecord::Abort {
            txid: TransactionId::new(8),
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Abort, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn checkpoint_record_roundtrip() {
        let record = WalRecord::Checkpoint {
            sequence: SequenceNumber::new(500),
        };
        let payload = record.encode_payload().unwrap();
        let decoded = WalRecord::decode_payload(WalRecordType::Checkpoint, &payload).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn crc32_known_value() {
        // Known test vector: "123456789" should give 0xCBF43926
        let crc = compute_crc32(b"123456789");
        assert_eq!(crc, 0xCBF4_3926);
    }

    #[test]
    fn crc32_empty() {
        let crc = compute_crc32(b"");
        assert_eq!(crc, 0x0000_0000);
    }
}
