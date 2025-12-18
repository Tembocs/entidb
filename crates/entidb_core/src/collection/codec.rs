//! Entity codec trait for typed collections.

use crate::entity::EntityId;
use crate::error::CoreResult;

/// Trait for types that can be stored as entities in EntiDB.
///
/// Implementors must provide:
/// - `entity_id()`: Returns the stable, immutable entity identifier
/// - `encode()`: Serializes to canonical CBOR bytes
/// - `decode()`: Deserializes from CBOR bytes
///
/// # Example
///
/// ```rust,ignore
/// use entidb_core::{EntityCodec, EntityId};
/// use entidb_codec::{to_canonical_cbor, from_cbor, Value};
///
/// struct User {
///     id: EntityId,
///     name: String,
///     age: u32,
/// }
///
/// impl EntityCodec for User {
///     fn entity_id(&self) -> EntityId {
///         self.id
///     }
///
///     fn encode(&self) -> CoreResult<Vec<u8>> {
///         let map = Value::map(vec![
///             (Value::Text("id".into()), Value::Bytes(self.id.as_bytes().to_vec())),
///             (Value::Text("name".into()), Value::Text(self.name.clone())),
///             (Value::Text("age".into()), Value::Integer(self.age as i64)),
///         ]);
///         Ok(to_canonical_cbor(&map)?)
///     }
///
///     fn decode(id: EntityId, bytes: &[u8]) -> CoreResult<Self> {
///         let value: Value = from_cbor(bytes)?;
///         // ... parse fields from value
///         Ok(User { id, name, age })
///     }
/// }
/// ```
pub trait EntityCodec: Sized {
    /// Returns the entity's stable, immutable identifier.
    ///
    /// This ID must not change over the entity's lifetime.
    fn entity_id(&self) -> EntityId;

    /// Encodes the entity to canonical CBOR bytes.
    ///
    /// The encoding must be deterministic - identical entities
    /// must produce identical bytes.
    fn encode(&self) -> CoreResult<Vec<u8>>;

    /// Decodes an entity from CBOR bytes.
    ///
    /// The `id` parameter provides the entity ID from storage,
    /// which should match the ID encoded in the bytes.
    fn decode(id: EntityId, bytes: &[u8]) -> CoreResult<Self>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityId;
    use entidb_codec::{from_cbor, to_canonical_cbor, Value};

    #[derive(Debug, Clone, PartialEq)]
    struct TestEntity {
        id: EntityId,
        name: String,
        value: i64,
    }

    impl EntityCodec for TestEntity {
        fn entity_id(&self) -> EntityId {
            self.id
        }

        fn encode(&self) -> CoreResult<Vec<u8>> {
            let map = Value::map(vec![
                (
                    Value::Text("name".into()),
                    Value::Text(self.name.clone()),
                ),
                (Value::Text("value".into()), Value::Integer(self.value)),
            ]);
            Ok(to_canonical_cbor(&map)?)
        }

        fn decode(id: EntityId, bytes: &[u8]) -> CoreResult<Self> {
            let value: Value = from_cbor(bytes)?;
            let map = value.as_map().ok_or_else(|| {
                crate::error::CoreError::InvalidFormat {
                    message: "expected map".into(),
                }
            })?;

            let name = map
                .iter()
                .find(|(k, _)| k.as_text() == Some("name"))
                .and_then(|(_, v)| v.as_text())
                .ok_or_else(|| crate::error::CoreError::InvalidFormat {
                    message: "missing name".into(),
                })?
                .to_string();

            let val = map
                .iter()
                .find(|(k, _)| k.as_text() == Some("value"))
                .and_then(|(_, v)| v.as_integer())
                .ok_or_else(|| crate::error::CoreError::InvalidFormat {
                    message: "missing value".into(),
                })?;

            Ok(TestEntity {
                id,
                name,
                value: val,
            })
        }
    }

    #[test]
    fn encode_decode_roundtrip() {
        let entity = TestEntity {
            id: EntityId::new(),
            name: "test".to_string(),
            value: 42,
        };

        let bytes = entity.encode().unwrap();
        let decoded = TestEntity::decode(entity.id, &bytes).unwrap();

        assert_eq!(entity, decoded);
    }

    #[test]
    fn entity_id_is_stable() {
        let id = EntityId::new();
        let entity = TestEntity {
            id,
            name: "test".to_string(),
            value: 100,
        };

        assert_eq!(entity.entity_id(), id);
    }

    #[test]
    fn deterministic_encoding() {
        let id = EntityId::new();
        let entity1 = TestEntity {
            id,
            name: "test".to_string(),
            value: 42,
        };
        let entity2 = entity1.clone();

        let bytes1 = entity1.encode().unwrap();
        let bytes2 = entity2.encode().unwrap();

        assert_eq!(bytes1, bytes2);
    }
}
