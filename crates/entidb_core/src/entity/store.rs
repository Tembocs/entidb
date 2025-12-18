//! Entity store for CRUD operations.

use crate::entity::EntityId;
use crate::error::CoreResult;
use crate::segment::SegmentManager;
use crate::transaction::{Transaction, TransactionManager};
use crate::types::CollectionId;
use std::sync::Arc;

/// Provides entity-level operations on the database.
///
/// The `EntityStore` is a convenience layer over `TransactionManager`
/// for working with entities. All operations go through transactions.
pub struct EntityStore {
    /// Transaction manager.
    txn_manager: Arc<TransactionManager>,
    /// Segment manager for direct reads.
    segments: Arc<SegmentManager>,
}

impl EntityStore {
    /// Creates a new entity store.
    pub fn new(txn_manager: Arc<TransactionManager>, segments: Arc<SegmentManager>) -> Self {
        Self {
            txn_manager,
            segments,
        }
    }

    /// Gets an entity by ID.
    ///
    /// This performs a snapshot read outside of a transaction.
    pub fn get(
        &self,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.segments.get(collection_id, entity_id.as_bytes())
    }

    /// Gets an entity within a transaction.
    pub fn get_in_txn(
        &self,
        txn: &mut Transaction,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.txn_manager.get(txn, collection_id, entity_id)
    }

    /// Checks if an entity exists.
    pub fn exists(&self, collection_id: CollectionId, entity_id: EntityId) -> CoreResult<bool> {
        Ok(self.get(collection_id, entity_id)?.is_some())
    }

    /// Returns all entities in a collection.
    pub fn list(&self, collection_id: CollectionId) -> CoreResult<Vec<(EntityId, Vec<u8>)>> {
        let raw = self.segments.iter_collection(collection_id)?;
        Ok(raw
            .into_iter()
            .map(|(id_bytes, payload)| (EntityId::from_bytes(id_bytes), payload))
            .collect())
    }

    /// Returns the count of entities in a collection.
    pub fn count(&self, collection_id: CollectionId) -> CoreResult<usize> {
        Ok(self.list(collection_id)?.len())
    }

    /// Returns the total number of entities across all collections.
    pub fn total_count(&self) -> usize {
        self.segments.entity_count()
    }

    /// Begins a new transaction.
    pub fn begin(&self) -> CoreResult<Transaction> {
        self.txn_manager.begin()
    }

    /// Commits a transaction.
    pub fn commit(&self, txn: &mut Transaction) -> CoreResult<()> {
        self.txn_manager.commit(txn)?;
        Ok(())
    }

    /// Aborts a transaction.
    pub fn abort(&self, txn: &mut Transaction) -> CoreResult<()> {
        self.txn_manager.abort(txn)
    }

    /// Executes a function within a transaction.
    ///
    /// If the function returns `Ok`, the transaction is committed.
    /// If it returns `Err`, the transaction is aborted.
    pub fn transaction<F, T>(&self, f: F) -> CoreResult<T>
    where
        F: FnOnce(&mut Transaction) -> CoreResult<T>,
    {
        let mut txn = self.begin()?;
        match f(&mut txn) {
            Ok(result) => {
                self.commit(&mut txn)?;
                Ok(result)
            }
            Err(e) => {
                // Try to abort, but don't mask the original error
                let _ = self.abort(&mut txn);
                Err(e)
            }
        }
    }
}

impl std::fmt::Debug for EntityStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntityStore")
            .field("total_count", &self.total_count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::WalManager;
    use entidb_storage::InMemoryBackend;

    fn create_store() -> EntityStore {
        let wal = Arc::new(WalManager::new(Box::new(InMemoryBackend::new()), false));
        let segments = Arc::new(SegmentManager::new(
            Box::new(InMemoryBackend::new()),
            1024 * 1024,
        ));
        let txn_manager = Arc::new(TransactionManager::new(wal, Arc::clone(&segments)));
        EntityStore::new(txn_manager, segments)
    }

    #[test]
    fn transaction_helper() {
        let store = create_store();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();
        let payload = vec![1, 2, 3];

        store
            .transaction(|txn| {
                txn.put(collection, entity, payload.clone())?;
                Ok(())
            })
            .unwrap();

        let result = store.get(collection, entity).unwrap();
        assert_eq!(result, Some(payload));
    }

    #[test]
    fn transaction_rollback_on_error() {
        let store = create_store();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        let result: CoreResult<()> = store.transaction(|txn| {
            txn.put(collection, entity, vec![1, 2, 3])?;
            Err(crate::error::CoreError::invalid_operation("test error"))
        });

        assert!(result.is_err());

        // Data should not be visible
        let data = store.get(collection, entity).unwrap();
        assert!(data.is_none());
    }

    #[test]
    fn list_collection() {
        let store = create_store();
        let collection = CollectionId::new(1);

        store
            .transaction(|txn| {
                for i in 0..3 {
                    txn.put(collection, EntityId::new(), vec![i])?;
                }
                Ok(())
            })
            .unwrap();

        let entities = store.list(collection).unwrap();
        assert_eq!(entities.len(), 3);
    }

    #[test]
    fn count_entities() {
        let store = create_store();
        let collection = CollectionId::new(1);

        assert_eq!(store.count(collection).unwrap(), 0);

        store
            .transaction(|txn| {
                txn.put(collection, EntityId::new(), vec![1])?;
                txn.put(collection, EntityId::new(), vec![2])?;
                Ok(())
            })
            .unwrap();

        assert_eq!(store.count(collection).unwrap(), 2);
    }

    #[test]
    fn exists_check() {
        let store = create_store();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        assert!(!store.exists(collection, entity).unwrap());

        store
            .transaction(|txn| {
                txn.put(collection, entity, vec![1])?;
                Ok(())
            })
            .unwrap();

        assert!(store.exists(collection, entity).unwrap());
    }

    #[test]
    fn get_in_transaction_sees_uncommitted() {
        let store = create_store();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();
        let payload = vec![42];

        let mut txn = store.begin().unwrap();
        txn.put(collection, entity, payload.clone()).unwrap();

        // Should see uncommitted write
        let result = store.get_in_txn(&mut txn, collection, entity).unwrap();
        assert_eq!(result, Some(payload));

        // But direct get should not
        let direct = store.get(collection, entity).unwrap();
        assert!(direct.is_none());

        store.abort(&mut txn).unwrap();
    }
}
