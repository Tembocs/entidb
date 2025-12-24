//! Database facade and recovery.

use crate::config::Config;
use crate::dir::DatabaseDir;
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
use std::path::Path;
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
/// # Opening a Database
///
/// Use `Database::open()` to open a database from a directory path:
///
/// ```rust,ignore
/// use entidb_core::Database;
/// use std::path::Path;
///
/// // Open or create a database
/// let db = Database::open(Path::new("my_database"))?;
///
/// // Use the database
/// db.transaction(|txn| {
///     let id = entidb_core::EntityId::new();
///     txn.put(db.collection("users"), id, vec![1, 2, 3])?;
///     Ok(())
/// })?;
///
/// // Close gracefully
/// db.close()?;
/// ```
///
/// # In-Memory Databases
///
/// For testing, use `Database::open_in_memory()`:
///
/// ```rust,ignore
/// let db = Database::open_in_memory()?;
/// ```
pub struct Database {
    /// Configuration.
    config: Config,
    /// Database directory (holds the lock). None for in-memory databases.
    dir: Option<DatabaseDir>,
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
    /// Opens a database from a directory path.
    ///
    /// This is the recommended way to open a persistent database. The method:
    /// - Creates the directory if it doesn't exist (unless `create_if_missing` is false)
    /// - Acquires an exclusive lock to prevent concurrent access
    /// - Loads or creates the manifest
    /// - Recovers from the WAL if present
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the database directory
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Another process has the database locked (`DatabaseLocked`)
    /// - The database format is incompatible (`InvalidFormat`)
    /// - I/O errors occur
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use entidb_core::Database;
    /// use std::path::Path;
    ///
    /// let db = Database::open(Path::new("my_database"))?;
    /// ```
    pub fn open(path: &Path) -> CoreResult<Self> {
        Self::open_with_config(path, Config::default())
    }

    /// Opens a database from a directory path with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the database directory
    /// * `config` - Database configuration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use entidb_core::{Database, Config};
    /// use std::path::Path;
    ///
    /// let config = Config::default()
    ///     .create_if_missing(true)
    ///     .sync_on_commit(true);
    ///
    /// let db = Database::open_with_config(Path::new("my_database"), config)?;
    /// ```
    pub fn open_with_config(path: &Path, config: Config) -> CoreResult<Self> {
        use entidb_storage::FileBackend;

        // Open directory with lock
        let dir = DatabaseDir::open(path, config.create_if_missing)?;

        // Check if this is an existing database
        if !config.create_if_missing && dir.is_new_database() {
            return Err(CoreError::invalid_format(
                "database does not exist and create_if_missing is false",
            ));
        }

        if config.error_if_exists && !dir.is_new_database() {
            return Err(CoreError::invalid_format(
                "database already exists and error_if_exists is true",
            ));
        }

        // Load or create manifest
        let manifest = match dir.load_manifest()? {
            Some(m) => {
                // Validate format version
                if m.format_version.0 != config.format_version.0 {
                    return Err(CoreError::invalid_format(format!(
                        "incompatible format version: database is v{}.{}, expected v{}.{}",
                        m.format_version.0,
                        m.format_version.1,
                        config.format_version.0,
                        config.format_version.1
                    )));
                }
                m
            }
            None => Manifest::new(config.format_version),
        };

        // Open storage backends
        let wal_backend = FileBackend::open_with_create_dirs(&dir.wal_path())?;
        let segment_backend = FileBackend::open_with_create_dirs(&dir.segment_path())?;

        // Create managers
        let wal = Arc::new(WalManager::new(
            Box::new(wal_backend),
            config.sync_on_commit,
        ));
        let segments = Arc::new(SegmentManager::new(
            Box::new(segment_backend),
            config.max_segment_size,
        ));

        // Recover from WAL (use existing manifest as base)
        let (recovered_manifest, next_txid, next_seq, committed_seq) =
            Self::recover_with_manifest(&wal, &segments, manifest)?;

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
            dir: Some(dir),
            manifest: RwLock::new(recovered_manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
        })
    }

    /// Opens a database with the given backends.
    ///
    /// This is a lower-level constructor for when you have pre-configured backends.
    /// For most use cases, prefer `Database::open()` instead.
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
            dir: None,
            manifest: RwLock::new(manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
        })
    }

    /// Opens a fresh in-memory database for testing.
    ///
    /// This creates a non-persistent database that exists only in memory.
    /// Data is lost when the database is closed.
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
        Self::recover_with_manifest(wal, segments, Manifest::new((1, 0)))
    }

    /// Recovers database state from WAL with an existing manifest.
    ///
    /// Returns (manifest, next_txid, next_seq, committed_seq).
    fn recover_with_manifest(
        wal: &WalManager,
        segments: &SegmentManager,
        manifest: Manifest,
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
    ///
    /// If this is a persistent database (opened via `open()` or `open_with_config()`),
    /// the manifest is automatically saved to disk when a new collection is created.
    pub fn collection(&self, name: &str) -> CollectionId {
        let mut manifest = self.manifest.write();
        let existing = manifest.get_collection(name);
        let id = manifest.get_or_create_collection(name);
        
        // Save manifest if this was a new collection
        if existing.is_none() {
            if let Some(ref dir) = self.dir {
                // Best-effort save - log but don't fail
                if let Err(e) = dir.save_manifest(&manifest) {
                    // In production, this would be logged
                    // For now, we just ignore the error since collection() returns CollectionId, not Result
                    let _ = e;
                }
            }
        }
        
        CollectionId::new(id)
    }

    /// Gets a collection ID by name, if it exists.
    pub fn get_collection(&self, name: &str) -> Option<CollectionId> {
        let manifest = self.manifest.read();
        manifest.get_collection(name).map(CollectionId::new)
    }

    /// Creates a checkpoint.
    ///
    /// A checkpoint persists all committed data and truncates the WAL
    /// to reclaim space. After a checkpoint:
    /// - All committed transactions are durable in segments
    /// - The WAL is cleared
    /// - The manifest is updated with the checkpoint sequence
    pub fn checkpoint(&self) -> CoreResult<()> {
        self.ensure_open()?;
        
        // Perform the checkpoint (flushes segments, truncates WAL)
        self.txn_manager.checkpoint()?;
        
        // Update manifest with checkpoint sequence and save
        if let Some(ref dir) = self.dir {
            let mut manifest = self.manifest.write();
            manifest.last_checkpoint = Some(self.committed_seq());
            dir.save_manifest(&manifest)?;
        }
        
        Ok(())
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

        // Save manifest if we have a directory
        if let Some(ref dir) = self.dir {
            let manifest = self.manifest.read();
            dir.save_manifest(&manifest)?;
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

    // ========================================================================
    // Backup and Restore
    // ========================================================================

    /// Creates a backup of the database.
    ///
    /// Returns the backup data as bytes that can be saved to a file.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let backup_data = db.backup()?;
    /// std::fs::write("backup.endb", &backup_data)?;
    /// ```
    pub fn backup(&self) -> CoreResult<Vec<u8>> {
        self.ensure_open()?;

        use crate::backup::{BackupConfig, BackupManager};

        let backup_mgr = BackupManager::new(BackupConfig::default());
        let current_seq = self.committed_seq();

        let result = backup_mgr.create_backup(&self.segments, current_seq)?;
        Ok(result.data)
    }

    /// Creates a backup with custom options.
    ///
    /// # Arguments
    ///
    /// * `include_tombstones` - Whether to include deleted entities in the backup.
    pub fn backup_with_options(&self, include_tombstones: bool) -> CoreResult<Vec<u8>> {
        self.ensure_open()?;

        use crate::backup::{BackupConfig, BackupManager};

        let config = BackupConfig {
            include_tombstones,
            compress: false,
        };
        let backup_mgr = BackupManager::new(config);
        let current_seq = self.committed_seq();

        let result = backup_mgr.create_backup(&self.segments, current_seq)?;
        Ok(result.data)
    }

    /// Restores entities from a backup into this database.
    ///
    /// This merges the backup data into the current database.
    /// Existing entities with the same ID will be overwritten.
    ///
    /// # Arguments
    ///
    /// * `backup_data` - The backup data bytes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let backup_data = std::fs::read("backup.endb")?;
    /// db.restore(&backup_data)?;
    /// ```
    pub fn restore(&self, backup_data: &[u8]) -> CoreResult<RestoreStats> {
        self.ensure_open()?;

        use crate::backup::{BackupConfig, BackupManager};

        let backup_mgr = BackupManager::new(BackupConfig::default());
        let result = backup_mgr.restore_from_backup(backup_data)?;

        let mut restored = 0u64;
        let mut tombstones = 0u64;

        // Import all records in a transaction
        self.transaction(|txn| {
            for record in &result.records {
                let entity_id = EntityId::from_bytes(record.entity_id);
                if record.is_tombstone() {
                    txn.delete(record.collection_id, entity_id)?;
                    tombstones += 1;
                } else {
                    txn.put(
                        record.collection_id,
                        entity_id,
                        record.payload.clone(),
                    )?;
                    restored += 1;
                }
            }
            Ok(())
        })?;

        Ok(RestoreStats {
            entities_restored: restored,
            tombstones_applied: tombstones,
            backup_timestamp: result.metadata.timestamp,
            backup_sequence: result.metadata.sequence.as_u64(),
        })
    }

    /// Validates a backup without restoring it.
    ///
    /// Returns the backup metadata if valid.
    pub fn validate_backup(&self, backup_data: &[u8]) -> CoreResult<BackupInfo> {
        use crate::backup::{BackupConfig, BackupManager};

        let backup_mgr = BackupManager::new(BackupConfig::default());
        let metadata = backup_mgr.read_metadata(backup_data)?;
        let valid = backup_mgr.validate_backup(backup_data)?;

        Ok(BackupInfo {
            valid,
            timestamp: metadata.timestamp,
            sequence: metadata.sequence.as_u64(),
            record_count: metadata.record_count,
            size: metadata.size,
        })
    }
}

/// Statistics from a restore operation.
#[derive(Debug, Clone)]
pub struct RestoreStats {
    /// Number of entities restored.
    pub entities_restored: u64,
    /// Number of tombstones (deletions) applied.
    pub tombstones_applied: u64,
    /// Timestamp when the backup was created (Unix millis).
    pub backup_timestamp: u64,
    /// Sequence number at the time of backup.
    pub backup_sequence: u64,
}

/// Information about a backup.
#[derive(Debug, Clone)]
pub struct BackupInfo {
    /// Whether the backup checksum is valid.
    pub valid: bool,
    /// Timestamp when the backup was created (Unix millis).
    pub timestamp: u64,
    /// Sequence number at the time of backup.
    pub sequence: u64,
    /// Number of records in the backup.
    pub record_count: u32,
    /// Size of the backup in bytes.
    pub size: usize,
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

    #[test]
    fn checkpoint_clears_wal() {
        let db = create_db();
        let collection = db.collection("test");

        // Create some data
        db.transaction(|txn| {
            txn.put(collection, EntityId::new(), vec![1, 2, 3])?;
            Ok(())
        })
        .unwrap();

        // WAL should have data
        assert!(db.wal.size().unwrap() > 0);

        // Checkpoint should clear the WAL
        db.checkpoint().unwrap();

        // WAL should be empty after checkpoint
        assert_eq!(db.wal.size().unwrap(), 0);

        // But data should still be accessible from segments
        let items = db.list(collection).unwrap();
        assert_eq!(items.len(), 1);
    }
}

/// Persistence tests that require a real file system.
#[cfg(test)]
mod persistence_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn collections_persist_across_restarts() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("persist_test");

        let entity_id = EntityId::new();

        // First session: create collection and data
        {
            let db = Database::open(&db_path).unwrap();
            let users = db.collection("users");
            let posts = db.collection("posts");

            db.transaction(|txn| {
                txn.put(users, entity_id, vec![1, 2, 3])?;
                Ok(())
            })
            .unwrap();

            // Close to ensure everything is saved
            db.close().unwrap();
        }

        // Second session: verify collections and data persist
        {
            let db = Database::open(&db_path).unwrap();

            // Collections should be found
            let users = db.get_collection("users");
            let posts = db.get_collection("posts");
            assert!(users.is_some(), "users collection should persist");
            assert!(posts.is_some(), "posts collection should persist");

            // Data should be available
            let data = db.get(users.unwrap(), entity_id).unwrap();
            assert_eq!(data, Some(vec![1, 2, 3]));

            db.close().unwrap();
        }
    }

    #[test]
    fn wal_recovery_after_crash() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("crash_test");

        let entity_id = EntityId::new();

        // First session: create data but don't call close()
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("test");

            db.transaction(|txn| {
                txn.put(collection, entity_id, vec![42, 43, 44])?;
                Ok(())
            })
            .unwrap();

            // Simulate crash - don't call close(), just drop
            // This means WAL is flushed but manifest might not be saved
        }

        // Second session: should recover from WAL
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("test");

            let data = db.get(collection, entity_id).unwrap();
            assert_eq!(data, Some(vec![42, 43, 44]));

            db.close().unwrap();
        }
    }

    #[test]
    fn checkpoint_enables_wal_free_restart() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("checkpoint_test");

        let entity_id = EntityId::new();

        // First session: create data and checkpoint
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            db.transaction(|txn| {
                txn.put(collection, entity_id, vec![1, 2, 3, 4, 5])?;
                Ok(())
            })
            .unwrap();

            // Checkpoint flushes everything to segments and clears WAL
            db.checkpoint().unwrap();

            // WAL should be empty
            assert_eq!(db.wal.size().unwrap(), 0);

            db.close().unwrap();
        }

        // Second session: data should be recovered from segments only
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            let data = db.get(collection, entity_id).unwrap();
            assert_eq!(data, Some(vec![1, 2, 3, 4, 5]));

            db.close().unwrap();
        }
    }
}
