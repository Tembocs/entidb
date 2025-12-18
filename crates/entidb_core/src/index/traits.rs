//! Index traits and key types.

use crate::entity::EntityId;
use crate::error::CoreResult;
use crate::types::CollectionId;
use std::cmp::Ordering;
use std::hash::Hash;

/// A key that can be indexed.
///
/// Index keys must be:
/// - Hashable (for HashIndex)
/// - Orderable (for BTreeIndex)
/// - Serializable to bytes (for persistence)
pub trait IndexKey: Clone + Eq + Hash + Ord + Send + Sync + 'static {
    /// Serializes the key to bytes.
    fn to_bytes(&self) -> Vec<u8>;

    /// Deserializes the key from bytes.
    fn from_bytes(bytes: &[u8]) -> CoreResult<Self>;

    /// Compares two keys by their byte representation.
    fn compare_bytes(a: &[u8], b: &[u8]) -> Ordering {
        a.cmp(b)
    }
}

// Implement IndexKey for common types

impl IndexKey for i64 {
    fn to_bytes(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() != 8 {
            return Err(crate::error::CoreError::InvalidFormat {
                message: "expected 8 bytes for i64".into(),
            });
        }
        let arr: [u8; 8] = bytes.try_into().unwrap();
        Ok(i64::from_be_bytes(arr))
    }
}

impl IndexKey for String {
    fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        String::from_utf8(bytes.to_vec()).map_err(|_| crate::error::CoreError::InvalidFormat {
            message: "invalid UTF-8".into(),
        })
    }
}

impl IndexKey for Vec<u8> {
    fn to_bytes(&self) -> Vec<u8> {
        self.clone()
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        Ok(bytes.to_vec())
    }
}

/// Specification for an index on a collection.
#[derive(Debug, Clone)]
pub struct IndexSpec<K: IndexKey> {
    /// Collection this index belongs to.
    pub collection_id: CollectionId,
    /// Name of the index (internal, not user-facing).
    pub name: String,
    /// Whether the index enforces uniqueness.
    pub unique: bool,
    /// Key extractor marker.
    _marker: std::marker::PhantomData<K>,
}

impl<K: IndexKey> IndexSpec<K> {
    /// Creates a new index specification.
    pub fn new(collection_id: CollectionId, name: impl Into<String>) -> Self {
        Self {
            collection_id,
            name: name.into(),
            unique: false,
            _marker: std::marker::PhantomData,
        }
    }

    /// Makes this a unique index.
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
}

/// Core index trait.
///
/// All index implementations must provide these operations.
pub trait Index<K: IndexKey>: Send + Sync {
    /// Returns the index specification.
    fn spec(&self) -> &IndexSpec<K>;

    /// Inserts a key-entity mapping.
    fn insert(&mut self, key: K, entity_id: EntityId) -> CoreResult<()>;

    /// Removes a key-entity mapping.
    fn remove(&mut self, key: &K, entity_id: EntityId) -> CoreResult<bool>;

    /// Looks up entities by exact key.
    fn lookup(&self, key: &K) -> CoreResult<Vec<EntityId>>;

    /// Checks if the index contains a key.
    fn contains(&self, key: &K) -> bool;

    /// Returns the number of entries in the index.
    fn len(&self) -> usize;

    /// Returns true if the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears the index.
    fn clear(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionId;

    #[test]
    fn index_key_i64() {
        let key: i64 = 42;
        let bytes = key.to_bytes();
        let decoded = i64::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn index_key_string() {
        let key = "hello".to_string();
        let bytes = key.to_bytes();
        let decoded = String::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn index_key_bytes() {
        let key = vec![1u8, 2, 3, 4];
        let bytes = key.to_bytes();
        let decoded = Vec::<u8>::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn index_spec_builder() {
        let spec: IndexSpec<String> = IndexSpec::new(CollectionId::new(1), "name_idx").unique();

        assert_eq!(spec.collection_id, CollectionId::new(1));
        assert_eq!(spec.name, "name_idx");
        assert!(spec.unique);
    }

    #[test]
    fn i64_ordering() {
        let a: i64 = 10;
        let b: i64 = 20;
        assert!(i64::compare_bytes(&a.to_bytes(), &b.to_bytes()) == Ordering::Less);
    }
}
