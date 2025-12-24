//! BTree index implementation.

use crate::entity::EntityId;
use crate::error::CoreResult;
use crate::index::traits::{Index, IndexKey, IndexSpec};
use std::collections::{BTreeMap, HashSet};
use std::ops::Bound;

/// BTree-based index for ordered traversal and range queries.
///
/// `BTreeIndex` supports:
/// - Equality lookups
/// - Range queries (greater than, less than, between)
/// - Ordered iteration
/// - Prefix scans (for string keys)
///
/// # Use Cases
///
/// - Range queries (e.g., age > 18)
/// - Ordered pagination
/// - Prefix matching
///
/// # Example
///
/// ```rust,ignore
/// let mut index: BTreeIndex<i64> = BTreeIndex::new(spec);
///
/// // Insert
/// index.insert(25, entity_id)?;
///
/// // Range query
/// let adults = index.range(18..)?;
/// ```
pub struct BTreeIndex<K: IndexKey> {
    /// Index specification.
    spec: IndexSpec<K>,
    /// Ordered key to entity IDs mapping.
    entries: BTreeMap<K, HashSet<EntityId>>,
    /// Total entry count.
    count: usize,
}

impl<K: IndexKey> BTreeIndex<K> {
    /// Creates a new BTree index.
    pub fn new(spec: IndexSpec<K>) -> Self {
        Self {
            spec,
            entries: BTreeMap::new(),
            count: 0,
        }
    }

    /// Returns entities with keys in the given range.
    ///
    /// This is the primary range query method.
    pub fn range<R>(&self, range: R) -> CoreResult<Vec<EntityId>>
    where
        R: std::ops::RangeBounds<K>,
    {
        let mut result = Vec::new();
        for (_, entities) in self.entries.range(range) {
            result.extend(entities.iter().copied());
        }
        Ok(result)
    }

    /// Returns entities with keys greater than the given key.
    pub fn greater_than(&self, key: &K) -> CoreResult<Vec<EntityId>> {
        self.range((Bound::Excluded(key.clone()), Bound::Unbounded))
    }

    /// Returns entities with keys greater than or equal to the given key.
    pub fn greater_than_or_equal(&self, key: &K) -> CoreResult<Vec<EntityId>> {
        self.range((Bound::Included(key.clone()), Bound::Unbounded))
    }

    /// Returns entities with keys less than the given key.
    pub fn less_than(&self, key: &K) -> CoreResult<Vec<EntityId>> {
        self.range((Bound::Unbounded, Bound::Excluded(key.clone())))
    }

    /// Returns entities with keys less than or equal to the given key.
    pub fn less_than_or_equal(&self, key: &K) -> CoreResult<Vec<EntityId>> {
        self.range((Bound::Unbounded, Bound::Included(key.clone())))
    }

    /// Returns entities with keys between min and max (inclusive).
    pub fn between(&self, min: &K, max: &K) -> CoreResult<Vec<EntityId>> {
        self.range(min.clone()..=max.clone())
    }

    /// Returns all entries in order.
    pub fn scan_ordered(&self) -> Vec<(K, EntityId)> {
        let mut result = Vec::new();
        for (key, entities) in &self.entries {
            for entity_id in entities {
                result.push((key.clone(), *entity_id));
            }
        }
        result
    }

    /// Returns the first N entries in order.
    pub fn take(&self, n: usize) -> Vec<(K, EntityId)> {
        let mut result = Vec::new();
        for (key, entities) in &self.entries {
            for entity_id in entities {
                result.push((key.clone(), *entity_id));
                if result.len() >= n {
                    return result;
                }
            }
        }
        result
    }

    /// Returns the last N entries in order.
    pub fn take_last(&self, n: usize) -> Vec<(K, EntityId)> {
        let mut result = Vec::new();
        for (key, entities) in self.entries.iter().rev() {
            for entity_id in entities {
                result.push((key.clone(), *entity_id));
                if result.len() >= n {
                    result.reverse();
                    return result;
                }
            }
        }
        result.reverse();
        result
    }

    /// Returns the minimum key.
    pub fn min_key(&self) -> Option<&K> {
        self.entries.keys().next()
    }

    /// Returns the maximum key.
    pub fn max_key(&self) -> Option<&K> {
        self.entries.keys().next_back()
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

    /// Returns a reference to all entries for iteration.
    ///
    /// Used by persistence layer to serialize index state.
    pub fn entries(&self) -> &BTreeMap<K, HashSet<EntityId>> {
        &self.entries
    }

    /// Serializes this index to bytes for persistence.
    pub fn to_bytes(&self) -> Vec<u8> {
        crate::index::persistence::persist_btree_index(self)
    }

    /// Deserializes a BTree index from bytes.
    pub fn from_bytes(data: &[u8]) -> crate::error::CoreResult<Self> {
        crate::index::persistence::load_btree_index(data)
    }
}

impl<K: IndexKey> Index<K> for BTreeIndex<K> {
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

    fn test_spec() -> IndexSpec<i64> {
        IndexSpec::new(CollectionId::new(1), "age_idx")
    }

    #[test]
    fn insert_and_lookup() {
        let mut index = BTreeIndex::new(test_spec());
        let entity_id = EntityId::new();

        index.insert(25, entity_id).unwrap();

        let found = index.lookup(&25).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], entity_id);
    }

    #[test]
    fn range_query() {
        let mut index = BTreeIndex::new(test_spec());

        // Insert ages 10, 20, 30, 40, 50
        let mut ids = Vec::new();
        for age in [10, 20, 30, 40, 50] {
            let id = EntityId::new();
            ids.push((age, id));
            index.insert(age, id).unwrap();
        }

        // Range 20..40 (inclusive start, exclusive end)
        let found = index.range(20..40).unwrap();
        assert_eq!(found.len(), 2); // 20, 30

        // Range 20..=40 (inclusive both)
        let found = index.range(20..=40).unwrap();
        assert_eq!(found.len(), 3); // 20, 30, 40
    }

    #[test]
    fn greater_than() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [10, 20, 30, 40, 50] {
            index.insert(age, EntityId::new()).unwrap();
        }

        let found = index.greater_than(&30).unwrap();
        assert_eq!(found.len(), 2); // 40, 50

        let found = index.greater_than_or_equal(&30).unwrap();
        assert_eq!(found.len(), 3); // 30, 40, 50
    }

    #[test]
    fn less_than() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [10, 20, 30, 40, 50] {
            index.insert(age, EntityId::new()).unwrap();
        }

        let found = index.less_than(&30).unwrap();
        assert_eq!(found.len(), 2); // 10, 20

        let found = index.less_than_or_equal(&30).unwrap();
        assert_eq!(found.len(), 3); // 10, 20, 30
    }

    #[test]
    fn between() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [10, 20, 30, 40, 50] {
            index.insert(age, EntityId::new()).unwrap();
        }

        let found = index.between(&20, &40).unwrap();
        assert_eq!(found.len(), 3); // 20, 30, 40
    }

    #[test]
    fn min_max_key() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [30, 10, 50, 20, 40] {
            index.insert(age, EntityId::new()).unwrap();
        }

        assert_eq!(index.min_key(), Some(&10));
        assert_eq!(index.max_key(), Some(&50));
    }

    #[test]
    fn take_first_n() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [50, 40, 30, 20, 10] {
            index.insert(age, EntityId::new()).unwrap();
        }

        let first_3 = index.take(3);
        assert_eq!(first_3.len(), 3);
        assert_eq!(first_3[0].0, 10);
        assert_eq!(first_3[1].0, 20);
        assert_eq!(first_3[2].0, 30);
    }

    #[test]
    fn take_last_n() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [10, 20, 30, 40, 50] {
            index.insert(age, EntityId::new()).unwrap();
        }

        let last_3 = index.take_last(3);
        assert_eq!(last_3.len(), 3);
        assert_eq!(last_3[0].0, 30);
        assert_eq!(last_3[1].0, 40);
        assert_eq!(last_3[2].0, 50);
    }

    #[test]
    fn scan_ordered() {
        let mut index = BTreeIndex::new(test_spec());

        for age in [50, 10, 30] {
            index.insert(age, EntityId::new()).unwrap();
        }

        let ordered = index.scan_ordered();
        assert_eq!(ordered[0].0, 10);
        assert_eq!(ordered[1].0, 30);
        assert_eq!(ordered[2].0, 50);
    }

    #[test]
    fn remove_entry() {
        let mut index = BTreeIndex::new(test_spec());
        let entity_id = EntityId::new();

        index.insert(25, entity_id).unwrap();
        assert!(index.contains(&25));

        let removed = index.remove(&25, entity_id).unwrap();
        assert!(removed);
        assert!(!index.contains(&25));
    }

    #[test]
    fn string_key() {
        let spec: IndexSpec<String> = IndexSpec::new(CollectionId::new(1), "name_idx");
        let mut index = BTreeIndex::new(spec);

        index.insert("alice".to_string(), EntityId::new()).unwrap();
        index.insert("bob".to_string(), EntityId::new()).unwrap();
        index.insert("charlie".to_string(), EntityId::new()).unwrap();

        // Range query on strings
        let found = index
            .range("alice".to_string()..="bob".to_string())
            .unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn unique_constraint() {
        let spec = IndexSpec::new(CollectionId::new(1), "unique_age").unique();
        let mut index = BTreeIndex::new(spec);

        let id1 = EntityId::new();
        let id2 = EntityId::new();

        index.insert(25, id1).unwrap();

        // Same key with different entity should fail
        let result = index.insert(25, id2);
        assert!(result.is_err());
    }
}
