//! Hash index implementation.

use crate::entity::EntityId;
use crate::error::CoreResult;
use crate::index::traits::{Index, IndexKey, IndexSpec};
use std::collections::{HashMap, HashSet};

/// Hash-based index for O(1) equality lookups.
///
/// `HashIndex` is optimized for exact-match queries. It stores
/// a mapping from key to a set of entity IDs (non-unique index).
///
/// # Use Cases
///
/// - Lookup by unique identifier
/// - Foreign key lookups
/// - Equality filters
///
/// # Example
///
/// ```rust,ignore
/// let mut index: HashIndex<String> = HashIndex::new(spec);
///
/// // Insert
/// index.insert("alice".to_string(), entity_id)?;
///
/// // Lookup
/// let entities = index.lookup(&"alice".to_string())?;
/// ```
pub struct HashIndex<K: IndexKey> {
    /// Index specification.
    spec: IndexSpec<K>,
    /// Key to entity IDs mapping.
    entries: HashMap<K, HashSet<EntityId>>,
    /// Total entry count.
    count: usize,
}

impl<K: IndexKey> HashIndex<K> {
    /// Creates a new hash index.
    pub fn new(spec: IndexSpec<K>) -> Self {
        Self {
            spec,
            entries: HashMap::new(),
            count: 0,
        }
    }

    /// Rebuilds the index from a set of key-entity pairs.
    pub fn rebuild<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (K, EntityId)>,
    {
        self.clear();
        for (key, entity_id) in entries {
            let _ = self.insert(key, entity_id);
        }
    }
}

impl<K: IndexKey> Index<K> for HashIndex<K> {
    fn spec(&self) -> &IndexSpec<K> {
        &self.spec
    }

    fn insert(&mut self, key: K, entity_id: EntityId) -> CoreResult<()> {
        // For unique indexes, check if key already exists with different entity
        if self.spec.unique {
            if let Some(existing) = self.entries.get(&key) {
                if !existing.contains(&entity_id) && !existing.is_empty() {
                    return Err(crate::error::CoreError::TransactionConflict {
                        collection_id: self.spec.collection_id.as_u32(),
                        entity_id: *entity_id.as_bytes(),
                    });
                }
            }
        }

        let set = self.entries.entry(key).or_default();
        if set.insert(entity_id) {
            self.count += 1;
        }
        Ok(())
    }

    fn remove(&mut self, key: &K, entity_id: EntityId) -> CoreResult<bool> {
        if let Some(set) = self.entries.get_mut(key) {
            if set.remove(&entity_id) {
                self.count -= 1;
                if set.is_empty() {
                    self.entries.remove(key);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn lookup(&self, key: &K) -> CoreResult<Vec<EntityId>> {
        match self.entries.get(key) {
            Some(set) => Ok(set.iter().copied().collect()),
            None => Ok(Vec::new()),
        }
    }

    fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    fn len(&self) -> usize {
        self.count
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionId;

    fn test_spec() -> IndexSpec<String> {
        IndexSpec::new(CollectionId::new(1), "test_idx")
    }

    fn unique_spec() -> IndexSpec<String> {
        IndexSpec::new(CollectionId::new(1), "unique_idx").unique()
    }

    #[test]
    fn insert_and_lookup() {
        let mut index = HashIndex::new(test_spec());
        let entity_id = EntityId::new();

        index.insert("key1".to_string(), entity_id).unwrap();

        let found = index.lookup(&"key1".to_string()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], entity_id);
    }

    #[test]
    fn lookup_missing() {
        let index: HashIndex<String> = HashIndex::new(test_spec());

        let found = index.lookup(&"missing".to_string()).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn multiple_entities_same_key() {
        let mut index = HashIndex::new(test_spec());
        let id1 = EntityId::new();
        let id2 = EntityId::new();

        index.insert("key".to_string(), id1).unwrap();
        index.insert("key".to_string(), id2).unwrap();

        let found = index.lookup(&"key".to_string()).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.contains(&id1));
        assert!(found.contains(&id2));
    }

    #[test]
    fn remove_entry() {
        let mut index = HashIndex::new(test_spec());
        let entity_id = EntityId::new();

        index.insert("key".to_string(), entity_id).unwrap();
        assert!(index.contains(&"key".to_string()));

        let removed = index.remove(&"key".to_string(), entity_id).unwrap();
        assert!(removed);
        assert!(!index.contains(&"key".to_string()));
    }

    #[test]
    fn remove_one_of_many() {
        let mut index = HashIndex::new(test_spec());
        let id1 = EntityId::new();
        let id2 = EntityId::new();

        index.insert("key".to_string(), id1).unwrap();
        index.insert("key".to_string(), id2).unwrap();

        index.remove(&"key".to_string(), id1).unwrap();

        let found = index.lookup(&"key".to_string()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], id2);
    }

    #[test]
    fn unique_index_prevents_duplicates() {
        let mut index = HashIndex::new(unique_spec());
        let id1 = EntityId::new();
        let id2 = EntityId::new();

        index.insert("key".to_string(), id1).unwrap();

        // Same key with different entity should fail
        let result = index.insert("key".to_string(), id2);
        assert!(result.is_err());
    }

    #[test]
    fn unique_index_allows_update() {
        let mut index = HashIndex::new(unique_spec());
        let entity_id = EntityId::new();

        index.insert("key".to_string(), entity_id).unwrap();

        // Same key with same entity is allowed (update)
        index.insert("key".to_string(), entity_id).unwrap();

        assert_eq!(index.len(), 1);
    }

    #[test]
    fn len_and_clear() {
        let mut index = HashIndex::new(test_spec());

        for i in 0..5 {
            let id = EntityId::new();
            index.insert(format!("key{}", i), id).unwrap();
        }

        assert_eq!(index.len(), 5);
        assert!(!index.is_empty());

        index.clear();

        assert_eq!(index.len(), 0);
        assert!(index.is_empty());
    }

    #[test]
    fn rebuild_index() {
        let mut index = HashIndex::new(test_spec());

        // Initial data
        index.insert("old".to_string(), EntityId::new()).unwrap();
        assert_eq!(index.len(), 1);

        // Rebuild with new data
        let new_entries = vec![
            ("a".to_string(), EntityId::new()),
            ("b".to_string(), EntityId::new()),
            ("c".to_string(), EntityId::new()),
        ];

        index.rebuild(new_entries);

        assert_eq!(index.len(), 3);
        assert!(!index.contains(&"old".to_string()));
        assert!(index.contains(&"a".to_string()));
    }

    #[test]
    fn i64_key() {
        let spec: IndexSpec<i64> = IndexSpec::new(CollectionId::new(1), "age_idx");
        let mut index = HashIndex::new(spec);
        let entity_id = EntityId::new();

        index.insert(42, entity_id).unwrap();

        let found = index.lookup(&42).unwrap();
        assert_eq!(found.len(), 1);
    }
}
