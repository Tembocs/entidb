//! Property-based test generators using proptest.
//!
//! Provides strategies for generating random test data
//! that maintains required invariants.

use entidb_core::EntityId;
use proptest::prelude::*;

/// Strategy for generating valid entity IDs.
pub fn entity_id_strategy() -> impl Strategy<Value = EntityId> {
    prop::array::uniform16(any::<u8>()).prop_map(|bytes| EntityId::from_bytes(bytes))
}

/// Strategy for generating valid collection names.
pub fn collection_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z][a-zA-Z0-9_]{0,31}")
        .expect("Invalid regex")
        .prop_filter("Collection name must not be empty", |s| !s.is_empty())
}

/// Strategy for generating valid entity data (arbitrary bytes).
pub fn entity_data_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..1024)
}

/// Strategy for generating valid CBOR-like data.
/// This generates simple CBOR maps with string keys.
pub fn cbor_data_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        (
            prop::string::string_regex("[a-z]{1,10}").expect("Invalid regex"),
            any::<i32>(),
        ),
        1..5,
    )
    .prop_map(|pairs| {
        // Build a simple CBOR map manually
        let mut data = Vec::new();
        let len = pairs.len();

        // Map header (major type 5)
        if len < 24 {
            data.push(0xa0 | (len as u8));
        } else {
            data.push(0xb8);
            data.push(len as u8);
        }

        // Sort pairs by key for canonical CBOR
        let mut sorted_pairs = pairs;
        sorted_pairs.sort_by(|a, b| a.0.cmp(&b.0));

        for (key, value) in sorted_pairs {
            // Text string key
            let key_bytes = key.as_bytes();
            if key_bytes.len() < 24 {
                data.push(0x60 | (key_bytes.len() as u8));
            } else {
                data.push(0x78);
                data.push(key_bytes.len() as u8);
            }
            data.extend_from_slice(key_bytes);

            // Integer value
            if value >= 0 {
                let v = value as u32;
                if v < 24 {
                    data.push(v as u8);
                } else if v < 256 {
                    data.push(0x18);
                    data.push(v as u8);
                } else if v < 65536 {
                    data.push(0x19);
                    data.push((v >> 8) as u8);
                    data.push(v as u8);
                } else {
                    data.push(0x1a);
                    data.extend_from_slice(&v.to_be_bytes());
                }
            } else {
                let v = (-1 - value) as u32;
                if v < 24 {
                    data.push(0x20 | (v as u8));
                } else if v < 256 {
                    data.push(0x38);
                    data.push(v as u8);
                } else {
                    data.push(0x39);
                    data.push((v >> 8) as u8);
                    data.push(v as u8);
                }
            }
        }

        data
    })
}

/// Strategy for generating a batch of entity operations.
#[derive(Debug, Clone)]
pub enum EntityOperation {
    /// Put an entity
    Put {
        /// Entity ID
        id: EntityId,
        /// Entity data
        data: Vec<u8>,
    },
    /// Delete an entity
    Delete {
        /// Entity ID
        id: EntityId,
    },
    /// Get an entity
    Get {
        /// Entity ID
        id: EntityId,
    },
}

/// Strategy for generating entity operations.
pub fn entity_operation_strategy() -> impl Strategy<Value = EntityOperation> {
    prop_oneof![
        3 => (entity_id_strategy(), entity_data_strategy())
            .prop_map(|(id, data)| EntityOperation::Put { id, data }),
        1 => entity_id_strategy().prop_map(|id| EntityOperation::Delete { id }),
        2 => entity_id_strategy().prop_map(|id| EntityOperation::Get { id }),
    ]
}

/// Strategy for generating a sequence of operations.
pub fn operation_sequence_strategy(
    min_ops: usize,
    max_ops: usize,
) -> impl Strategy<Value = Vec<EntityOperation>> {
    prop::collection::vec(entity_operation_strategy(), min_ops..max_ops)
}

/// Configuration for property tests.
#[derive(Debug, Clone)]
pub struct PropTestConfig {
    /// Number of test cases to run.
    pub cases: u32,
    /// Maximum shrink iterations.
    pub max_shrink_iters: u32,
}

impl Default for PropTestConfig {
    fn default() -> Self {
        Self {
            cases: 256,
            max_shrink_iters: 1000,
        }
    }
}

impl PropTestConfig {
    /// Creates a configuration for quick tests.
    #[must_use]
    pub fn quick() -> Self {
        Self {
            cases: 32,
            max_shrink_iters: 100,
        }
    }

    /// Creates a configuration for thorough tests.
    #[must_use]
    pub fn thorough() -> Self {
        Self {
            cases: 1024,
            max_shrink_iters: 10000,
        }
    }

    /// Converts to proptest config.
    #[must_use]
    pub fn to_proptest_config(&self) -> ProptestConfig {
        ProptestConfig {
            cases: self.cases,
            max_shrink_iters: self.max_shrink_iters,
            ..ProptestConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #![proptest_config(PropTestConfig::quick().to_proptest_config())]

        #[test]
        fn entity_id_is_valid(id in entity_id_strategy()) {
            // Entity ID should be 16 bytes
            let bytes = id.as_bytes();
            prop_assert_eq!(bytes.len(), 16);
        }

        #[test]
        fn collection_name_is_valid(name in collection_name_strategy()) {
            // Collection name should start with a letter
            let first = name.chars().next();
            prop_assert!(first.map_or(false, |c| c.is_ascii_alphabetic()));
        }

        #[test]
        fn cbor_data_has_valid_header(data in cbor_data_strategy()) {
            // CBOR map should start with map header
            prop_assert!(!data.is_empty());
            let header = data[0];
            let major_type = header >> 5;
            prop_assert_eq!(major_type, 5, "Should be a map (major type 5)");
        }
    }
}
