//! Typed collection implementation.

use crate::collection::codec::EntityCodec;
use crate::entity::EntityId;
use crate::error::CoreResult;
use crate::segment::SegmentManager;
use crate::transaction::{PendingWrite, Transaction, TransactionManager};
use crate::types::CollectionId;
use std::marker::PhantomData;
use std::sync::Arc;

/// A typed collection of entities.
///
/// `Collection<T>` provides type-safe access to entities of type `T`,
/// where `T` implements `EntityCodec`. It handles encoding/decoding
/// automatically using canonical CBOR.
///
/// # Language-Native Querying
///
/// EntiDB does not use SQL or DSLs. Filtering is done using
/// host-language constructs:
///
/// ```rust,ignore
/// // Rust: use iterator adapters
/// let adults: Vec<User> = collection.iter()?
///     .filter(|u| u.age >= 18)
///     .collect();
///
/// // With explicit scan warning
/// let users = collection.scan_all()?; // Makes full scan explicit
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use entidb_core::{Collection, EntityCodec, EntityId};
///
/// let users: Collection<User> = db.typed_collection("users")?;
///
/// // Insert
/// let user = User { id: EntityId::new(), name: "Alice".into(), age: 30 };
/// users.put(&user)?;
///
/// // Get by ID
/// let found = users.get(user.id)?;
///
/// // Iterate with filter (host-language)
/// for user in users.iter()?.filter(|u| u.age > 25) {
///     println!("{}", user.name);
/// }
/// ```
pub struct Collection<T: EntityCodec> {
    /// The collection identifier.
    collection_id: CollectionId,
    /// Collection name for display.
    name: String,
    /// Transaction manager.
    txn_manager: Arc<TransactionManager>,
    /// Segment manager for direct reads.
    segments: Arc<SegmentManager>,
    /// Type marker.
    _marker: PhantomData<T>,
}

impl<T: EntityCodec> Collection<T> {
    /// Creates a new typed collection.
    pub fn new(
        collection_id: CollectionId,
        name: String,
        txn_manager: Arc<TransactionManager>,
        segments: Arc<SegmentManager>,
    ) -> Self {
        Self {
            collection_id,
            name,
            txn_manager,
            segments,
            _marker: PhantomData,
        }
    }

    /// Returns the collection ID.
    pub fn id(&self) -> CollectionId {
        self.collection_id
    }

    /// Returns the collection name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Puts an entity into the collection.
    ///
    /// This executes in its own transaction.
    pub fn put(&self, entity: &T) -> CoreResult<()> {
        let bytes = entity.encode()?;
        let entity_id = entity.entity_id();

        let mut txn = self.txn_manager.begin()?;
        txn.put(self.collection_id, entity_id, bytes)?;
        self.txn_manager.commit(&mut txn)?;

        Ok(())
    }

    /// Puts an entity within an existing transaction.
    pub fn put_in_txn(&self, txn: &mut Transaction, entity: &T) -> CoreResult<()> {
        let bytes = entity.encode()?;
        let entity_id = entity.entity_id();
        txn.put(self.collection_id, entity_id, bytes)?;
        Ok(())
    }

    /// Gets an entity by ID.
    ///
    /// Returns `None` if the entity doesn't exist.
    pub fn get(&self, id: EntityId) -> CoreResult<Option<T>> {
        match self.segments.get(self.collection_id, id.as_bytes())? {
            Some(bytes) => Ok(Some(T::decode(id, &bytes)?)),
            None => Ok(None),
        }
    }

    /// Gets an entity within a transaction.
    ///
    /// This sees uncommitted writes from the transaction.
    pub fn get_in_txn(&self, txn: &Transaction, id: EntityId) -> CoreResult<Option<T>> {
        // First check pending writes in transaction
        if let Some(pending) = txn.get_pending_write(self.collection_id, id) {
            match pending {
                PendingWrite::Put { payload, .. } => {
                    return Ok(Some(T::decode(id, payload)?));
                }
                PendingWrite::Delete { .. } => {
                    return Ok(None);
                }
            }
        }

        // Fall back to committed data
        self.get(id)
    }

    /// Deletes an entity by ID.
    ///
    /// This executes in its own transaction.
    pub fn delete(&self, id: EntityId) -> CoreResult<()> {
        let mut txn = self.txn_manager.begin()?;
        txn.delete(self.collection_id, id)?;
        self.txn_manager.commit(&mut txn)?;
        Ok(())
    }

    /// Deletes an entity within an existing transaction.
    pub fn delete_in_txn(&self, txn: &mut Transaction, id: EntityId) -> CoreResult<()> {
        txn.delete(self.collection_id, id)?;
        Ok(())
    }

    /// Checks if an entity exists.
    pub fn exists(&self, id: EntityId) -> CoreResult<bool> {
        Ok(self
            .segments
            .get(self.collection_id, id.as_bytes())?
            .is_some())
    }

    /// Returns the count of entities in this collection.
    ///
    /// **Warning**: This performs a full scan.
    pub fn count(&self) -> CoreResult<usize> {
        // Use iter_collection to count only this collection's entities
        Ok(self.segments.iter_collection(self.collection_id)?.len())
    }

    /// Scans all entities in the collection.
    ///
    /// **Warning**: This is a full table scan. For filtered access,
    /// consider using indexes.
    ///
    /// This method is intentionally named `scan_all` to make the
    /// performance implications explicit.
    pub fn scan_all(&self) -> CoreResult<Vec<T>> {
        let raw_entities = self.segments.iter_collection(self.collection_id)?;
        let mut result = Vec::with_capacity(raw_entities.len());

        for (entity_bytes, payload) in raw_entities {
            let id = EntityId::from_bytes(entity_bytes);
            result.push(T::decode(id, &payload)?);
        }

        Ok(result)
    }

    /// Returns an iterator over all entities.
    ///
    /// **Warning**: This is a full table scan. Use `iter_with_index`
    /// for indexed access.
    pub fn iter(&self) -> CoreResult<impl Iterator<Item = T>> {
        let entities = self.scan_all()?;
        Ok(entities.into_iter())
    }

    /// Executes a function within a transaction on this collection.
    pub fn transaction<F, R>(&self, f: F) -> CoreResult<R>
    where
        F: FnOnce(&mut Transaction) -> CoreResult<R>,
    {
        let mut txn = self.txn_manager.begin()?;

        match f(&mut txn) {
            Ok(result) => {
                self.txn_manager.commit(&mut txn)?;
                Ok(result)
            }
            Err(e) => {
                let _ = self.txn_manager.abort(&mut txn);
                Err(e)
            }
        }
    }
}

/// A reference to a collection within a transaction.
///
/// This allows multiple operations on the same collection within
/// a single transaction while maintaining type safety.
#[allow(dead_code)]
pub struct CollectionRef<'a, T: EntityCodec> {
    collection: &'a Collection<T>,
    #[allow(dead_code)]
    txn: &'a mut Transaction,
}

#[allow(dead_code)]
impl<'a, T: EntityCodec> CollectionRef<'a, T> {
    /// Gets an entity by ID.
    pub fn get(&self, id: EntityId) -> CoreResult<Option<T>> {
        self.collection.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityId;
    use crate::error::CoreError;
    use entidb_codec::{from_cbor, to_canonical_cbor, Value};
    use entidb_storage::InMemoryBackend;

    #[derive(Debug, Clone, PartialEq)]
    struct TestUser {
        id: EntityId,
        name: String,
        age: i64,
    }

    impl EntityCodec for TestUser {
        fn entity_id(&self) -> EntityId {
            self.id
        }

        fn encode(&self) -> CoreResult<Vec<u8>> {
            let map = Value::map(vec![
                (Value::Text("name".into()), Value::Text(self.name.clone())),
                (Value::Text("age".into()), Value::Integer(self.age)),
            ]);
            Ok(to_canonical_cbor(&map)?)
        }

        fn decode(id: EntityId, bytes: &[u8]) -> CoreResult<Self> {
            let value: Value = from_cbor(bytes)?;
            let map = value.as_map().ok_or_else(|| CoreError::InvalidFormat {
                message: "expected map".into(),
            })?;

            let name = map
                .iter()
                .find(|(k, _)| k.as_text() == Some("name"))
                .and_then(|(_, v)| v.as_text())
                .unwrap_or("unknown")
                .to_string();

            let age = map
                .iter()
                .find(|(k, _)| k.as_text() == Some("age"))
                .and_then(|(_, v)| v.as_integer())
                .unwrap_or(0);

            Ok(TestUser { id, name, age })
        }
    }

    fn create_test_collection() -> (Collection<TestUser>, Arc<TransactionManager>) {
        let wal = Arc::new(crate::wal::WalManager::new(
            Box::new(InMemoryBackend::new()),
            false,
        ));
        let segments = Arc::new(SegmentManager::new(
            Box::new(InMemoryBackend::new()),
            1024 * 1024,
        ));
        let txn_manager = Arc::new(TransactionManager::new(
            Arc::clone(&wal),
            Arc::clone(&segments),
        ));

        let collection = Collection::new(
            CollectionId::new(1),
            "users".to_string(),
            Arc::clone(&txn_manager),
            segments,
        );

        (collection, txn_manager)
    }

    #[test]
    fn put_and_get() {
        let (collection, _) = create_test_collection();

        let user = TestUser {
            id: EntityId::new(),
            name: "Alice".to_string(),
            age: 30,
        };

        collection.put(&user).unwrap();

        let found = collection.get(user.id).unwrap();
        assert_eq!(found, Some(user));
    }

    #[test]
    fn get_nonexistent() {
        let (collection, _) = create_test_collection();

        let found = collection.get(EntityId::new()).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn delete_entity() {
        let (collection, _) = create_test_collection();

        let user = TestUser {
            id: EntityId::new(),
            name: "Bob".to_string(),
            age: 25,
        };

        collection.put(&user).unwrap();
        assert!(collection.exists(user.id).unwrap());

        collection.delete(user.id).unwrap();
        assert!(!collection.exists(user.id).unwrap());
    }

    #[test]
    fn scan_all() {
        let (collection, _) = create_test_collection();

        let users = vec![
            TestUser {
                id: EntityId::new(),
                name: "Alice".to_string(),
                age: 30,
            },
            TestUser {
                id: EntityId::new(),
                name: "Bob".to_string(),
                age: 25,
            },
            TestUser {
                id: EntityId::new(),
                name: "Charlie".to_string(),
                age: 35,
            },
        ];

        for user in &users {
            collection.put(user).unwrap();
        }

        let scanned = collection.scan_all().unwrap();
        assert_eq!(scanned.len(), 3);

        // All users should be present
        for user in &users {
            assert!(scanned.contains(user));
        }
    }

    #[test]
    fn count() {
        let (collection, _) = create_test_collection();

        assert_eq!(collection.count().unwrap(), 0);

        for i in 0..5 {
            let user = TestUser {
                id: EntityId::new(),
                name: format!("User{}", i),
                age: i * 10,
            };
            collection.put(&user).unwrap();
        }

        assert_eq!(collection.count().unwrap(), 5);
    }

    #[test]
    fn iter_with_filter() {
        let (collection, _) = create_test_collection();

        let users = vec![
            TestUser {
                id: EntityId::new(),
                name: "Young".to_string(),
                age: 20,
            },
            TestUser {
                id: EntityId::new(),
                name: "Adult".to_string(),
                age: 30,
            },
            TestUser {
                id: EntityId::new(),
                name: "Senior".to_string(),
                age: 50,
            },
        ];

        for user in &users {
            collection.put(user).unwrap();
        }

        // Language-native filtering (the EntiDB way - no SQL!)
        let adults: Vec<TestUser> = collection.iter().unwrap().filter(|u| u.age >= 25).collect();

        assert_eq!(adults.len(), 2);
    }

    #[test]
    fn collection_metadata() {
        let (collection, _) = create_test_collection();

        assert_eq!(collection.name(), "users");
        assert_eq!(collection.id(), CollectionId::new(1));
    }
}
