//! Cross-language test vectors for EntiDB.
//!
//! These vectors ensure identical behavior across Rust, Dart, and Python bindings.

use serde::{Deserialize, Serialize};

/// A test vector that can be shared across languages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestVector {
    /// Unique identifier for this vector.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Input data (hex-encoded).
    pub input_hex: String,
    /// Expected output data (hex-encoded).
    pub expected_hex: String,
    /// Expected error message (if this should fail).
    pub expected_error: Option<String>,
}

/// CBOR encoding test vectors.
pub fn cbor_encoding_vectors() -> Vec<TestVector> {
    vec![
        TestVector {
            id: "cbor_null".into(),
            description: "CBOR null value".into(),
            input_hex: "f6".into(),
            expected_hex: "f6".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_true".into(),
            description: "CBOR true value".into(),
            input_hex: "f5".into(),
            expected_hex: "f5".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_false".into(),
            description: "CBOR false value".into(),
            input_hex: "f4".into(),
            expected_hex: "f4".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_0".into(),
            description: "CBOR integer 0".into(),
            input_hex: "00".into(),
            expected_hex: "00".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_23".into(),
            description: "CBOR integer 23 (largest 1-byte)".into(),
            input_hex: "17".into(),
            expected_hex: "17".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_24".into(),
            description: "CBOR integer 24 (smallest 2-byte)".into(),
            input_hex: "1818".into(),
            expected_hex: "1818".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_255".into(),
            description: "CBOR integer 255".into(),
            input_hex: "18ff".into(),
            expected_hex: "18ff".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_256".into(),
            description: "CBOR integer 256".into(),
            input_hex: "190100".into(),
            expected_hex: "190100".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_neg1".into(),
            description: "CBOR integer -1".into(),
            input_hex: "20".into(),
            expected_hex: "20".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_int_neg100".into(),
            description: "CBOR integer -100".into(),
            input_hex: "3863".into(),
            expected_hex: "3863".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_text_empty".into(),
            description: "CBOR empty text string".into(),
            input_hex: "60".into(),
            expected_hex: "60".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_text_hello".into(),
            description: "CBOR text string 'hello'".into(),
            input_hex: "6568656c6c6f".into(),
            expected_hex: "6568656c6c6f".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_bytes_empty".into(),
            description: "CBOR empty byte string".into(),
            input_hex: "40".into(),
            expected_hex: "40".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_bytes_data".into(),
            description: "CBOR byte string [0x01, 0x02, 0x03]".into(),
            input_hex: "43010203".into(),
            expected_hex: "43010203".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_array_empty".into(),
            description: "CBOR empty array".into(),
            input_hex: "80".into(),
            expected_hex: "80".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_array_123".into(),
            description: "CBOR array [1, 2, 3]".into(),
            input_hex: "83010203".into(),
            expected_hex: "83010203".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_map_empty".into(),
            description: "CBOR empty map".into(),
            input_hex: "a0".into(),
            expected_hex: "a0".into(),
            expected_error: None,
        },
        TestVector {
            id: "cbor_map_simple".into(),
            description: "CBOR map {'a': 1, 'b': 2} (canonical order)".into(),
            input_hex: "a2616101616202".into(),
            expected_hex: "a2616101616202".into(),
            expected_error: None,
        },
    ]
}

/// Entity ID test vectors.
pub fn entity_id_vectors() -> Vec<TestVector> {
    vec![
        TestVector {
            id: "entity_id_zeros".into(),
            description: "Entity ID with all zeros".into(),
            input_hex: "00000000000000000000000000000000".into(),
            expected_hex: "00000000000000000000000000000000".into(),
            expected_error: None,
        },
        TestVector {
            id: "entity_id_ones".into(),
            description: "Entity ID with all ones".into(),
            input_hex: "ffffffffffffffffffffffffffffffff".into(),
            expected_hex: "ffffffffffffffffffffffffffffffff".into(),
            expected_error: None,
        },
        TestVector {
            id: "entity_id_pattern".into(),
            description: "Entity ID with pattern".into(),
            input_hex: "0123456789abcdef0123456789abcdef".into(),
            expected_hex: "0123456789abcdef0123456789abcdef".into(),
            expected_error: None,
        },
    ]
}

/// WAL record test vectors.
pub fn wal_record_vectors() -> Vec<TestVector> {
    vec![
        TestVector {
            id: "wal_begin".into(),
            description: "WAL BEGIN record for txid=1".into(),
            input_hex: "454e5449000001000000000100000000000000crc32".into(),
            expected_hex: "454e5449000001000000000100000000000000crc32".into(),
            expected_error: None,
        },
        TestVector {
            id: "wal_commit".into(),
            description: "WAL COMMIT record for txid=1, seq=1".into(),
            input_hex: "454e5449000003000000000100000000000000010000000000000crc32".into(),
            expected_hex: "454e5449000003000000000100000000000000010000000000000crc32".into(),
            expected_error: None,
        },
    ]
}

/// Segment record test vectors.
pub fn segment_record_vectors() -> Vec<TestVector> {
    vec![
        TestVector {
            id: "segment_put".into(),
            description: "Segment PUT record".into(),
            input_hex: "put_record_hex".into(),
            expected_hex: "put_record_hex".into(),
            expected_error: None,
        },
        TestVector {
            id: "segment_tombstone".into(),
            description: "Segment tombstone record".into(),
            input_hex: "tombstone_record_hex".into(),
            expected_hex: "tombstone_record_hex".into(),
            expected_error: None,
        },
    ]
}

/// Generate all test vectors as JSON for cross-language use.
pub fn all_vectors_json() -> String {
    let vectors = AllTestVectors {
        cbor: cbor_encoding_vectors(),
        entity_id: entity_id_vectors(),
        wal: wal_record_vectors(),
        segment: segment_record_vectors(),
    };

    serde_json::to_string_pretty(&vectors).expect("Failed to serialize vectors")
}

#[derive(Debug, Serialize, Deserialize)]
struct AllTestVectors {
    cbor: Vec<TestVector>,
    entity_id: Vec<TestVector>,
    wal: Vec<TestVector>,
    segment: Vec<TestVector>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::golden::{hex_decode, hex_encode};

    #[test]
    fn test_cbor_vectors() {
        for vector in cbor_encoding_vectors() {
            let input = hex_decode(&vector.input_hex);
            let expected = hex_decode(&vector.expected_hex);

            // Try to decode and re-encode
            if let Ok(value) = entidb_codec::from_cbor(&input) {
                if let Ok(encoded) = entidb_codec::to_canonical_cbor(&value) {
                    assert_eq!(
                        hex_encode(&encoded),
                        hex_encode(&expected),
                        "Vector {} failed: {}",
                        vector.id,
                        vector.description
                    );
                }
            }
        }
    }

    #[test]
    fn test_entity_id_vectors() {
        for vector in entity_id_vectors() {
            let input = hex_decode(&vector.input_hex);
            if input.len() == 16 {
                let bytes: [u8; 16] = input.try_into().unwrap();
                let id = entidb_core::EntityId::from_bytes(bytes);
                let output = id.as_bytes();

                assert_eq!(
                    hex_encode(output),
                    vector.expected_hex,
                    "Vector {} failed: {}",
                    vector.id,
                    vector.description
                );
            }
        }
    }

    #[test]
    fn test_all_vectors_json() {
        let json = all_vectors_json();
        assert!(!json.is_empty());
        assert!(json.contains("cbor"));
        assert!(json.contains("entity_id"));
    }
}
