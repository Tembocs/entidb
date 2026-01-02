//! Fuzz testing harnesses for EntiDB.
//!
//! This module provides fuzz targets that can be used with cargo-fuzz
//! or other fuzzing frameworks.

use entidb_codec::{from_cbor, to_canonical_cbor};
use entidb_core::{CollectionId, Database, EntityId};

/// Fuzz target for CBOR decoding.
///
/// Tests that arbitrary byte sequences either:
/// - Decode successfully to a valid Value, or
/// - Return a proper error (no panics)
pub fn fuzz_cbor_decode(data: &[u8]) {
    // Try to decode - should never panic
    let _ = from_cbor(data);
}

/// Fuzz target for CBOR roundtrip.
///
/// Tests that encoding and decoding preserves values.
pub fn fuzz_cbor_roundtrip(data: &[u8]) {
    // Try to decode
    if let Ok(value) = from_cbor(data) {
        // If it decodes, try to re-encode
        if let Ok(encoded) = to_canonical_cbor(&value) {
            // Re-decode and compare
            if let Ok(decoded) = from_cbor(&encoded) {
                // Values should be equal (canonical form)
                assert_eq!(
                    format!("{:?}", value),
                    format!("{:?}", decoded),
                    "Roundtrip mismatch"
                );
            }
        }
    }
}

/// Fuzz target for database operations.
///
/// Tests that arbitrary operation sequences don't cause panics.
pub fn fuzz_database_operations(data: &[u8]) {
    if data.len() < 4 {
        return;
    }

    let db = match Database::open_in_memory() {
        Ok(db) => db,
        Err(_) => return,
    };

    let collection = CollectionId::new(1);
    let mut offset = 0;

    while offset + 17 <= data.len() {
        let op = data[offset];
        let id_bytes: [u8; 16] = data[offset + 1..offset + 17]
            .try_into()
            .unwrap_or([0u8; 16]);
        let id = EntityId::from_bytes(id_bytes);

        offset += 17;

        match op % 4 {
            0 => {
                // Put
                let payload_len = (data.get(offset).copied().unwrap_or(0) as usize) % 256;
                offset += 1;

                let payload: Vec<u8> = if offset + payload_len <= data.len() {
                    data[offset..offset + payload_len].to_vec()
                } else {
                    vec![0u8; payload_len]
                };
                offset += payload_len;

                let _ = db.transaction(|tx| {
                    tx.put(collection, id, payload)?;
                    Ok(())
                });
            }
            1 => {
                // Get
                let _ = db.get(collection, id);
            }
            2 => {
                // Delete
                let _ = db.transaction(|tx| {
                    tx.delete(collection, id)?;
                    Ok(())
                });
            }
            3 => {
                // List
                let _ = db.list(collection);
            }
            _ => {}
        }
    }
}

/// Fuzz target for entity ID handling.
///
/// Tests that entity IDs handle arbitrary input safely.
pub fn fuzz_entity_id(data: &[u8]) {
    if data.len() >= 16 {
        let bytes: [u8; 16] = data[..16].try_into().unwrap();
        let id = EntityId::from_bytes(bytes);

        // Round-trip
        let bytes2 = id.as_bytes();
        assert_eq!(&bytes, bytes2);

        // Display shouldn't panic
        let _ = format!("{}", id);
        let _ = format!("{:?}", id);
    }
}

/// Fuzz target for WAL record parsing.
///
/// Tests that WAL record parsing handles arbitrary input safely.
pub fn fuzz_wal_record(data: &[u8]) {
    use entidb_core::{WalRecord, WalRecordType};

    // Try to decode with various record types - should never panic
    for record_type in [
        WalRecordType::Begin,
        WalRecordType::Put,
        WalRecordType::Delete,
        WalRecordType::Commit,
        WalRecordType::Abort,
        WalRecordType::Checkpoint,
    ] {
        let _ = WalRecord::decode_payload(record_type, data);
    }
}

/// Fuzz target for segment record parsing.
pub fn fuzz_segment_record(data: &[u8]) {
    use entidb_core::SegmentRecord;

    // Try to decode - should never panic
    let _ = SegmentRecord::decode(data);
}

/// Structured fuzzing input for database operations.
#[derive(Debug, Clone)]
pub enum FuzzOp {
    /// Put an entity.
    Put {
        /// Collection identifier.
        collection: u8,
        /// Entity identifier (16 bytes).
        entity: [u8; 16],
        /// Entity data payload.
        data: Vec<u8>,
    },
    /// Get an entity.
    Get {
        /// Collection identifier.
        collection: u8,
        /// Entity identifier (16 bytes).
        entity: [u8; 16],
    },
    /// Delete an entity.
    Delete {
        /// Collection identifier.
        collection: u8,
        /// Entity identifier (16 bytes).
        entity: [u8; 16],
    },
    /// List entities in a collection.
    List {
        /// Collection identifier.
        collection: u8,
    },
    /// Checkpoint the database.
    Checkpoint,
}

impl FuzzOp {
    /// Parse operations from fuzzer input.
    pub fn parse_sequence(data: &[u8]) -> Vec<FuzzOp> {
        let mut ops = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let op_type = data[offset];
            offset += 1;

            let op = match op_type % 5 {
                0 => {
                    // Put
                    if offset + 17 > data.len() {
                        break;
                    }
                    let collection = data[offset];
                    let entity: [u8; 16] = data[offset + 1..offset + 17]
                        .try_into()
                        .unwrap_or([0u8; 16]);
                    offset += 17;

                    let data_len = data.get(offset).copied().unwrap_or(0) as usize;
                    offset += 1;

                    let payload = if offset + data_len <= data.len() {
                        data[offset..offset + data_len].to_vec()
                    } else {
                        break;
                    };
                    offset += data_len;

                    FuzzOp::Put {
                        collection,
                        entity,
                        data: payload,
                    }
                }
                1 => {
                    // Get
                    if offset + 17 > data.len() {
                        break;
                    }
                    let collection = data[offset];
                    let entity: [u8; 16] = data[offset + 1..offset + 17]
                        .try_into()
                        .unwrap_or([0u8; 16]);
                    offset += 17;

                    FuzzOp::Get { collection, entity }
                }
                2 => {
                    // Delete
                    if offset + 17 > data.len() {
                        break;
                    }
                    let collection = data[offset];
                    let entity: [u8; 16] = data[offset + 1..offset + 17]
                        .try_into()
                        .unwrap_or([0u8; 16]);
                    offset += 17;

                    FuzzOp::Delete { collection, entity }
                }
                3 => {
                    // List
                    if offset >= data.len() {
                        break;
                    }
                    let collection = data[offset];
                    offset += 1;

                    FuzzOp::List { collection }
                }
                4 => FuzzOp::Checkpoint,
                _ => break,
            };

            ops.push(op);
        }

        ops
    }

    /// Execute operations on a database.
    pub fn execute_sequence(ops: &[FuzzOp], db: &Database) {
        for op in ops {
            match op {
                FuzzOp::Put {
                    collection,
                    entity,
                    data,
                } => {
                    let coll = CollectionId::new(*collection as u32);
                    let id = EntityId::from_bytes(*entity);
                    let _ = db.transaction(|tx| {
                        tx.put(coll, id, data.clone())?;
                        Ok(())
                    });
                }
                FuzzOp::Get { collection, entity } => {
                    let coll = CollectionId::new(*collection as u32);
                    let id = EntityId::from_bytes(*entity);
                    let _ = db.get(coll, id);
                }
                FuzzOp::Delete { collection, entity } => {
                    let coll = CollectionId::new(*collection as u32);
                    let id = EntityId::from_bytes(*entity);
                    let _ = db.transaction(|tx| {
                        tx.delete(coll, id)?;
                        Ok(())
                    });
                }
                FuzzOp::List { collection } => {
                    let coll = CollectionId::new(*collection as u32);
                    let _ = db.list(coll);
                }
                FuzzOp::Checkpoint => {
                    let _ = db.checkpoint();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{DefaultHasher, Hash, Hasher};

    /// Generate pseudo-random data for fuzzing based on a seed.
    fn generate_random_data(seed: u64, len: usize) -> Vec<u8> {
        let mut hasher = DefaultHasher::new();
        let mut result = Vec::with_capacity(len);
        let mut state = seed;

        for _ in 0..len {
            state.hash(&mut hasher);
            state = hasher.finish();
            hasher = DefaultHasher::new();
            result.push((state & 0xFF) as u8);
        }

        result
    }

    #[test]
    fn test_fuzz_cbor_decode_empty() {
        fuzz_cbor_decode(&[]);
    }

    #[test]
    fn test_fuzz_cbor_decode_garbage() {
        fuzz_cbor_decode(&[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_fuzz_cbor_roundtrip_valid() {
        // Valid CBOR: positive integer 42
        fuzz_cbor_roundtrip(&[0x18, 0x2a]);
    }

    #[test]
    fn test_fuzz_database_operations_empty() {
        fuzz_database_operations(&[]);
    }

    #[test]
    fn test_fuzz_database_operations_random() {
        fuzz_database_operations(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17]);
    }

    #[test]
    fn test_fuzz_entity_id() {
        fuzz_entity_id(&[0u8; 16]);
        fuzz_entity_id(&[0xFF; 16]);
    }

    #[test]
    fn test_parse_fuzz_ops() {
        let data = vec![
            0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 1, 2, 3, 4, 5,
        ];
        let ops = FuzzOp::parse_sequence(&data);
        assert!(!ops.is_empty());
    }

    // Extended randomized fuzz tests for CI

    #[test]
    fn fuzz_cbor_decode_random_iterations() {
        // Run 1000 iterations with random data
        for seed in 0..1000u64 {
            let len = ((seed % 256) + 1) as usize;
            let data = generate_random_data(seed, len);
            fuzz_cbor_decode(&data);
        }
    }

    #[test]
    fn fuzz_cbor_roundtrip_random_iterations() {
        for seed in 0..500u64 {
            let len = ((seed % 64) + 1) as usize;
            let data = generate_random_data(seed, len);
            fuzz_cbor_roundtrip(&data);
        }
    }

    #[test]
    fn fuzz_entity_id_random_iterations() {
        for seed in 0..1000u64 {
            let data = generate_random_data(seed, 32);
            fuzz_entity_id(&data);
        }
    }

    #[test]
    fn fuzz_wal_record_random_iterations() {
        for seed in 0..500u64 {
            let len = ((seed % 128) + 1) as usize;
            let data = generate_random_data(seed, len);
            fuzz_wal_record(&data);
        }
    }

    #[test]
    fn fuzz_segment_record_random_iterations() {
        for seed in 0..500u64 {
            let len = ((seed % 256) + 1) as usize;
            let data = generate_random_data(seed, len);
            fuzz_segment_record(&data);
        }
    }

    #[test]
    fn fuzz_database_operations_random_iterations() {
        // Fewer iterations since database operations are more expensive
        for seed in 0..50u64 {
            let len = ((seed % 512) + 32) as usize;
            let data = generate_random_data(seed, len);
            fuzz_database_operations(&data);
        }
    }

    #[test]
    fn fuzz_structured_ops_random_iterations() {
        // Test structured operation sequences
        for seed in 0..30u64 {
            let len = ((seed % 256) + 32) as usize;
            let data = generate_random_data(seed, len);
            let ops = FuzzOp::parse_sequence(&data);

            if !ops.is_empty() {
                let db = Database::open_in_memory().expect("Failed to open in-memory database");
                FuzzOp::execute_sequence(&ops, &db);
            }
        }    }
}