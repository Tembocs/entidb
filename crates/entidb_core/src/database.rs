//! Database facade and recovery.

use crate::config::Config;
use crate::entity::{EntityId, EntityStore};
use crate::error::{CoreError, CoreResult};
use crate::manifest::Manifest;
use crate::segment::{SegmentManager, SegmentRecord};
use crate::transaction::{Transaction, TransactionManager};
use crate::types::{CollectionId, SequenceNumber, TransactionId};
use crate::wal::{WalManager, WalRecord};
use entidb_storage::StorageBackend;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The main database handle.
///
/// `Database` is the primary entry point for interacting with EntiDB.
/// It provides:
/// - Transaction management
/// - Entity CRUD operations
/// - Collection management
/// - Recovery from crashes
///
/// # Example
///
/// ```rust,ignore
/// use entidb_core::{Database, Config};
/// use entidb_storage::InMemoryBackend;
///
/// let db = Database::open_with_backends(
///     Config::default(),
///     Box::new(InMemoryBackend::new()), // WAL
///     Box::new(InMemoryBackend::new()), // Segments
/// )?;
///
/// db.transaction(|txn| {
///     let entity_id = EntityId::new();
///     txn.put(CollectionId::new(1), entity_id, vec![1, 2, 3])?;
///     Ok(())
/// })?;
/// ```
pub struct Database {
    /// Configuration.
    config: Config,
    /// Database manifest.
    manifest: RwLock<Manifest>,
    /// WAL manager.
    wal: Arc<WalManager>,
    /// Segment manager.
    segments: Arc<SegmentManager>,
    /// Transaction manager.
    txn_manager: Arc<TransactionManager>,
    /// Entity store.
    entity_store: EntityStore,
    /// Whether the database is open.
    is_open: RwLock<bool>,
}

impl Database {
    /// Opens a database with the given backends.
    ///
    /// This is the primary constructor when you have pre-configured backends.
    pub fn open_with_backends(
        config: Config,
        wal_backend: Box<dyn StorageBackend>,
        segment_backend: Box<dyn StorageBackend>,
    ) -> CoreResult<Self> {
        let wal = Arc::new(WalManager::new(wal_backend, config.sync_on_commit));
        let segments = Arc::new(SegmentManager::new(
            segment_backend,
            config.max_segment_size,
        ));

        // Recover from WAL
        let (manifest, next_txid, next_seq, committed_seq) = Self::recover(&wal, &segments)?;

        let txn_manager = Arc::new(TransactionManager::with_state(
            Arc::clone(&wal),
            Arc::clone(&segments),
            next_txid,
            next_seq,
            committed_seq,
        ));

        let entity_store = EntityStore::new(Arc::clone(&txn_manager), Arc::clone(&segments));

        Ok(Self {
            config,
            manifest: RwLock::new(manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
        })
    }

    /// Opens a fresh in-memory database for testing.
    pub fn open_in_memory() -> CoreResult<Self> {
        use entidb_storage::InMemoryBackend;
        Self::open_with_backends(
            Config::default(),
            Box::new(InMemoryBackend::new()),
            Box::new(InMemoryBackend::new()),
        )
    }

    /// Recovers database state from WAL.
    ///
    /// Returns (manifest, next_txid, next_seq, committed_seq).
    fn recover(
        wal: &WalManager,
        segments: &SegmentManager,
    ) -> CoreResult<(Manifest, u64, u64, u64)> {
        let records = wal.read_all()?;

        // Track transaction states
        let mut active_txns: HashMap<TransactionId, Vec<WalRecord>> = HashMap::new();
        let mut committed_txns: HashSet<TransactionId> = HashSet::new();
        let mut max_txid = 0u64;
        let mut max_seq = 0u64;
        let mut committed_seq = 0u64;

        // First pass: identify committed transactions
        for (_, record) in &records {
            if let Some(txid) = record.txid() {
                max_txid = max_txid.max(txid.as_u64());
            }

            match record {
                WalRecord::Begin { txid } => {
                    active_txns.insert(*txid, Vec::new());
                }
                WalRecord::Put { txid, .. } | WalRecord::Delete { txid, .. } => {
                    if let Some(ops) = active_txns.get_mut(txid) {
                        ops.push(record.clone());
                    }
                }
                WalRecord::Commit { txid, sequence } => {
                    committed_txns.insert(*txid);
                    max_seq = max_seq.max(sequence.as_u64());
                    committed_seq = committed_seq.max(sequence.as_u64());
                }
                WalRecord::Abort { txid } => {
                    active_txns.remove(txid);
                }
                WalRecord::Checkpoint { sequence } => {
                    max_seq = max_seq.max(sequence.as_u64());
                }
            }
        }

        // Second pass: replay committed transactions to segments
        for (txid, ops) in &active_txns {
            if !committed_txns.contains(txid) {
                continue; // Skip uncommitted transactions
            }

            // Find the commit record to get sequence number
            let commit_seq = records
                .iter()
                .find_map(|(_, r)| match r {
                    WalRecord::Commit { txid: t, sequence } if t == txid => Some(*sequence),
                    _ => None,
                })
                .unwrap_or(SequenceNumber::new(0));

            for op in ops {
                match op {
                    WalRecord::Put {
                        collection_id,
                        entity_id,
                        after_bytes,
                        ..
                    } => {
                        let record = SegmentRecord::put(
                            *collection_id,
                            *entity_id,
                            after_bytes.clone(),
                            commit_seq,
                        );
                        segments.append(&record)?;
                    }
                    WalRecord::Delete {
                        collection_id,
                        entity_id,
                        ..
                    } => {
                        let record =
                            SegmentRecord::tombstone(*collection_id, *entity_id, commit_seq);
                        segments.append(&record)?;
                    }
                    _ => {}
                }
            }
        }

        // Rebuild segment index
        segments.rebuild_index()?;

        let manifest = Manifest::new((1, 0));

        Ok((manifest, max_txid + 1, max_seq + 1, committed_seq))
    }

    /// Begins a new transaction.
    pub fn begin(&self) -> CoreResult<Transaction> {
        self.ensure_open()?;
        self.txn_manager.begin()
    }

    /// Commits a transaction.
    pub fn commit(&self, txn: &mut Transaction) -> CoreResult<SequenceNumber> {
        self.ensure_open()?;
        self.txn_manager.commit(txn)
    }

    /// Aborts a transaction.
    pub fn abort(&self, txn: &mut Transaction) -> CoreResult<()> {
        self.ensure_open()?;
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
        self.ensure_open()?;
        self.entity_store.transaction(f)
    }

    /// Gets an entity by collection and ID.
    pub fn get(
        &self,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.ensure_open()?;
        self.entity_store.get(collection_id, entity_id)
    }

    /// Gets an entity within a transaction.
    pub fn get_in_txn(
        &self,
        txn: &mut Transaction,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.ensure_open()?;
        self.entity_store.get_in_txn(txn, collection_id, entity_id)
    }

    /// Lists all entities in a collection.
    pub fn list(&self, collection_id: CollectionId) -> CoreResult<Vec<(EntityId, Vec<u8>)>> {
        self.ensure_open()?;
        self.entity_store.list(collection_id)
    }

    /// Gets or creates a collection ID for a name.
    pub fn collection(&self, name: &str) -> CollectionId {
        let mut manifest = self.manifest.write();
        CollectionId::new(manifest.get_or_create_collection(name))
    }

    /// Gets a collection ID by name, if it exists.
    pub fn get_collection(&self, name: &str) -> Option<CollectionId> {
        let manifest = self.manifest.read();
        manifest.get_collection(name).map(CollectionId::new)
    }

    /// Creates a checkpoint.
    pub fn checkpoint(&self) -> CoreResult<()> {
        self.ensure_open()?;
        self.txn_manager.checkpoint()
    }

    /// Returns the current committed sequence number.
    #[must_use]
    pub fn committed_seq(&self) -> SequenceNumber {
        self.txn_manager.committed_seq()
    }

    /// Returns the total entity count.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entity_store.total_count()
    }

    /// Closes the database.
    pub fn close(&self) -> CoreResult<()> {
        let mut is_open = self.is_open.write();
        if !*is_open {
            return Ok(());
        }

        // Flush everything
        self.wal.flush()?;
        self.segments.flush()?;

        *is_open = false;
        Ok(())
    }

    /// Checks if the database is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        *self.is_open.read()
    }

    /// Ensures the database is open.
    fn ensure_open(&self) -> CoreResult<()> {
        if *self.is_open.read() {
            Ok(())
        } else {
            Err(CoreError::DatabaseClosed)
        }
    }

    /// Returns database configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("is_open", &self.is_open())
            .field("entity_count", &self.entity_count())
            .field("committed_seq", &self.committed_seq())
            .finish_non_exhaustive()
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn open_in_memory() {
        let db = create_db();
        assert!(db.is_open());
    }

    #[test]
    fn simple_put_get() {
        let db = create_db();
        let collection = db.collection("users");
        let entity = EntityId::new();
        let payload = vec![1, 2, 3];

        db.transaction(|txn| {
            txn.put(collection, entity, payload.clone())?;
            Ok(())
        })
        .unwrap();

        let result = db.get(collection, entity).unwrap();
        assert_eq!(result, Some(payload));
    }

    #[test]
    fn delete_entity() {
        let db = create_db();
        let collection = db.collection("users");
        let entity = EntityId::new();

        // Create
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1, 2, 3])?;
            Ok(())
        })
        .unwrap();

        // Delete
        db.transaction(|txn| {
            txn.delete(collection, entity)?;
            Ok(())
        })
        .unwrap();

        let result = db.get(collection, entity).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn transaction_isolation() {
        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        let mut txn = db.begin().unwrap();
        txn.put(collection, entity, vec![42]).unwrap();

        // Uncommitted data not visible outside transaction
        let result = db.get(collection, entity).unwrap();
        assert!(result.is_none());

        // But visible inside
        let inner = db.get_in_txn(&mut txn, collection, entity).unwrap();
        assert_eq!(inner, Some(vec![42]));

        db.commit(&mut txn).unwrap();

        // Now visible
        let result = db.get(collection, entity).unwrap();
        assert_eq!(result, Some(vec![42]));
    }

    #[test]
    fn transaction_abort() {
        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        let mut txn = db.begin().unwrap();
        txn.put(collection, entity, vec![1, 2, 3]).unwrap();
        db.abort(&mut txn).unwrap();

        // Data not visible
        let result = db.get(collection, entity).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn list_collection() {
        let db = create_db();
        let collection = db.collection("items");

        db.transaction(|txn| {
            for i in 0..5u8 {
                txn.put(collection, EntityId::new(), vec![i])?;
            }
            Ok(())
        })
        .unwrap();

        let items = db.list(collection).unwrap();
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn collection_name_resolution() {
        let db = create_db();

        let c1 = db.collection("users");
        let c2 = db.collection("posts");
        let c1_again = db.collection("users");

        assert_eq!(c1, c1_again);
        assert_ne!(c1, c2);
    }

    #[test]
    fn get_collection_returns_none_if_missing() {
        let db = create_db();
        assert!(db.get_collection("nonexistent").is_none());
    }

    #[test]
    fn entity_count() {
        let db = create_db();
        assert_eq!(db.entity_count(), 0);

        let collection = db.collection("test");
        db.transaction(|txn| {
            txn.put(collection, EntityId::new(), vec![1])?;
            txn.put(collection, EntityId::new(), vec![2])?;
            Ok(())
        })
        .unwrap();

        assert_eq!(db.entity_count(), 2);
    }

    #[test]
    fn committed_seq_increases() {
        let db = create_db();
        let initial = db.committed_seq();

        let collection = db.collection("test");
        db.transaction(|txn| {
            txn.put(collection, EntityId::new(), vec![1])?;
            Ok(())
        })
        .unwrap();

        assert!(db.committed_seq() > initial);
    }

    #[test]
    fn close_database() {
        let db = create_db();
        assert!(db.is_open());

        db.close().unwrap();
        assert!(!db.is_open());

        // Operations should fail
        let result = db.get(CollectionId::new(1), EntityId::new());
        assert!(matches!(result, Err(CoreError::DatabaseClosed)));
    }

    #[test]
    fn checkpoint() {
        let db = create_db();
        let collection = db.collection("test");

        db.transaction(|txn| {
            txn.put(collection, EntityId::new(), vec![1])?;
            Ok(())
        })
        .unwrap();

        // Should not error
        db.checkpoint().unwrap();
    }

    #[test]
    fn multiple_collections() {
        let db = create_db();
        let users = db.collection("users");
        let posts = db.collection("posts");

        let user_id = EntityId::new();
        let post_id = EntityId::new();

        db.transaction(|txn| {
            txn.put(users, user_id, vec![1])?;
            txn.put(posts, post_id, vec![2])?;
            Ok(())
        })
        .unwrap();

        assert_eq!(db.get(users, user_id).unwrap(), Some(vec![1]));
        assert_eq!(db.get(posts, post_id).unwrap(), Some(vec![2]));

        // Cross-collection isolation
        assert!(db.get(users, post_id).unwrap().is_none());
        assert!(db.get(posts, user_id).unwrap().is_none());
    }
}
