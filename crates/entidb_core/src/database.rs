//! Database facade and recovery.

use crate::config::Config;
use crate::dir::DatabaseDir;
use crate::entity::{EntityId, EntityStore};
use crate::error::{CoreError, CoreResult};
use crate::index::{BTreeIndex, HashIndex, Index, IndexSpec};
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
    /// Hash indexes keyed by (collection_id, index_name).
    hash_indexes: RwLock<HashMap<(u32, String), HashIndex<Vec<u8>>>>,
    /// BTree indexes keyed by (collection_id, index_name).
    btree_indexes: RwLock<HashMap<(u32, String), BTreeIndex<Vec<u8>>>>,
    /// Change feed for observing committed operations.
    change_feed: Arc<crate::change_feed::ChangeFeed>,
    /// Database statistics.
    stats: Arc<crate::stats::DatabaseStats>,
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
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            change_feed: Arc::new(crate::change_feed::ChangeFeed::new()),
            stats: Arc::new(crate::stats::DatabaseStats::new()),
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
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            change_feed: Arc::new(crate::change_feed::ChangeFeed::new()),
            stats: Arc::new(crate::stats::DatabaseStats::new()),
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
        self.stats.record_transaction_start();
        self.txn_manager.begin()
    }

    /// Commits a transaction.
    ///
    /// After successful commit, change events are emitted to all subscribers.
    pub fn commit(&self, txn: &mut Transaction) -> CoreResult<SequenceNumber> {
        self.ensure_open()?;
        
        // Collect pending writes before commit (they're consumed during commit)
        let pending_writes: Vec<_> = txn.pending_writes()
            .map(|((cid, eid), w)| (*cid, *eid, w.clone()))
            .collect();
        
        // Commit the transaction
        let sequence = self.txn_manager.commit(txn)?;
        
        // Record stats
        self.stats.record_transaction_commit();
        
        // Emit change events for each write
        for (collection_id, entity_id, write) in pending_writes {
            let event = match write {
                crate::transaction::PendingWrite::Put { payload } => {
                    // Track bytes written
                    self.stats.record_write(payload.len() as u64);
                    // Determine if this is insert or update (for now, always use insert semantics)
                    crate::change_feed::ChangeEvent::insert(
                        sequence.as_u64(),
                        collection_id.as_u32(),
                        *entity_id.as_bytes(),
                        payload,
                    )
                }
                crate::transaction::PendingWrite::Delete => {
                    self.stats.record_delete();
                    crate::change_feed::ChangeEvent::delete(
                        sequence.as_u64(),
                        collection_id.as_u32(),
                        *entity_id.as_bytes(),
                    )
                }
            };
            self.change_feed.emit(event);
        }
        
        Ok(sequence)
    }

    /// Aborts a transaction.
    pub fn abort(&self, txn: &mut Transaction) -> CoreResult<()> {
        self.ensure_open()?;
        self.stats.record_transaction_abort();
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
        let mut txn = self.begin()?;
        match f(&mut txn) {
            Ok(result) => {
                self.commit(&mut txn)?;
                Ok(result)
            }
            Err(e) => {
                // Abort will record the abort stat
                let _ = self.abort(&mut txn);
                Err(e)
            }
        }
    }

    /// Gets an entity by collection and ID.
    pub fn get(
        &self,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.ensure_open()?;
        let result = self.entity_store.get(collection_id, entity_id);
        if let Ok(Some(ref data)) = result {
            self.stats.record_read(data.len() as u64);
        }
        result
    }

    /// Gets an entity within a transaction.
    pub fn get_in_txn(
        &self,
        txn: &mut Transaction,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.ensure_open()?;
        let result = self.entity_store.get_in_txn(txn, collection_id, entity_id);
        if let Ok(Some(ref data)) = result {
            self.stats.record_read(data.len() as u64);
        }
        result
    }

    /// Lists all entities in a collection.
    pub fn list(&self, collection_id: CollectionId) -> CoreResult<Vec<(EntityId, Vec<u8>)>> {
        self.ensure_open()?;
        self.stats.record_scan(); // Full collection scan
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
        
        // Record checkpoint stat
        self.stats.record_checkpoint();
        
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
    // Observability
    // ========================================================================

    /// Subscribes to the change feed.
    ///
    /// Returns a receiver that will receive `ChangeEvent` notifications
    /// for all committed operations. The receiver is automatically cleaned up
    /// when dropped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::thread;
    ///
    /// let rx = db.subscribe();
    ///
    /// // In another thread, listen for changes
    /// thread::spawn(move || {
    ///     while let Ok(event) = rx.recv() {
    ///         println!("Change: {:?}", event);
    ///     }
    /// });
    /// ```
    #[must_use]
    pub fn subscribe(&self) -> std::sync::mpsc::Receiver<crate::change_feed::ChangeEvent> {
        self.change_feed.subscribe()
    }

    /// Returns a snapshot of database statistics.
    ///
    /// Statistics include counts of reads, writes, transactions, bytes,
    /// and other operations. This is useful for monitoring and diagnostics.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stats = db.stats();
    /// println!("Reads: {}, Writes: {}", stats.reads, stats.writes);
    /// ```
    #[must_use]
    pub fn stats(&self) -> crate::stats::StatsSnapshot {
        self.stats.snapshot()
    }

    /// Returns the change feed for direct access.
    ///
    /// This is useful for advanced use cases like polling with a cursor.
    #[must_use]
    pub fn change_feed(&self) -> &crate::change_feed::ChangeFeed {
        &self.change_feed
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

    // ========================================================================
    // Index Management
    // ========================================================================

    /// Creates a hash index for fast equality lookups.
    ///
    /// Hash indexes provide O(1) lookup by exact key match. They are ideal for:
    /// - Unique identifier lookups
    /// - Foreign key relationships
    /// - Equality filters
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection to index
    /// * `name` - A unique name for this index within the collection
    /// * `unique` - Whether the index should enforce uniqueness
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users = db.collection("users");
    /// db.create_hash_index(users, "email", true)?; // Unique email index
    /// ```
    pub fn create_hash_index(
        &self,
        collection_id: CollectionId,
        name: &str,
        unique: bool,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        let key = (collection_id.as_u32(), name.to_string());
        let mut indexes = self.hash_indexes.write();

        if indexes.contains_key(&key) {
            return Err(CoreError::invalid_format(format!(
                "hash index '{}' already exists on collection {}",
                name,
                collection_id.as_u32()
            )));
        }

        let spec = if unique {
            IndexSpec::new(collection_id, name).unique()
        } else {
            IndexSpec::new(collection_id, name)
        };

        indexes.insert(key, HashIndex::new(spec));
        Ok(())
    }

    /// Creates a BTree index for ordered traversal and range queries.
    ///
    /// BTree indexes support:
    /// - Equality lookups
    /// - Range queries (greater than, less than, between)
    /// - Ordered iteration
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection to index
    /// * `name` - A unique name for this index within the collection
    /// * `unique` - Whether the index should enforce uniqueness
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users = db.collection("users");
    /// db.create_btree_index(users, "age", false)?; // Non-unique age index
    /// ```
    pub fn create_btree_index(
        &self,
        collection_id: CollectionId,
        name: &str,
        unique: bool,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        let key = (collection_id.as_u32(), name.to_string());
        let mut indexes = self.btree_indexes.write();

        if indexes.contains_key(&key) {
            return Err(CoreError::invalid_format(format!(
                "btree index '{}' already exists on collection {}",
                name,
                collection_id.as_u32()
            )));
        }

        let spec = if unique {
            IndexSpec::new(collection_id, name).unique()
        } else {
            IndexSpec::new(collection_id, name)
        };

        indexes.insert(key, BTreeIndex::new(spec));
        Ok(())
    }

    /// Inserts an entry into a hash index.
    ///
    /// This should be called when inserting/updating an entity to maintain the index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the hash index
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to associate with this key
    pub fn hash_index_insert(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        key: Vec<u8>,
        entity_id: EntityId,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let mut indexes = self.hash_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "hash index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        index.insert(key, entity_id)
    }

    /// Removes an entry from a hash index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the hash index
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to remove
    pub fn hash_index_remove(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        key: &[u8],
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let mut indexes = self.hash_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "hash index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        index.remove(&key.to_vec(), entity_id)
    }

    /// Looks up entities by exact key match in a hash index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the hash index
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// A vector of entity IDs that have the given key value.
    pub fn hash_index_lookup(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;
        self.stats.record_index_lookup();

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let indexes = self.hash_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "hash index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        index.lookup(&key.to_vec())
    }

    /// Inserts an entry into a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the BTree index
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to associate with this key
    pub fn btree_index_insert(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        key: Vec<u8>,
        entity_id: EntityId,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let mut indexes = self.btree_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "btree index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        index.insert(key, entity_id)
    }

    /// Removes an entry from a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the BTree index
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to remove
    pub fn btree_index_remove(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        key: &[u8],
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let mut indexes = self.btree_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "btree index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        index.remove(&key.to_vec(), entity_id)
    }

    /// Looks up entities by exact key match in a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the BTree index
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// A vector of entity IDs that have the given key value.
    pub fn btree_index_lookup(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;
        self.stats.record_index_lookup();

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let indexes = self.btree_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "btree index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        index.lookup(&key.to_vec())
    }

    /// Performs a range query on a BTree index.
    ///
    /// Returns all entities whose key is >= min_key and <= max_key.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `index_name` - The name of the BTree index
    /// * `min_key` - The minimum key (inclusive), or None for unbounded
    /// * `max_key` - The maximum key (inclusive), or None for unbounded
    ///
    /// # Returns
    ///
    /// A vector of entity IDs whose keys fall within the range.
    pub fn btree_index_range(
        &self,
        collection_id: CollectionId,
        index_name: &str,
        min_key: Option<&[u8]>,
        max_key: Option<&[u8]>,
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;
        self.stats.record_index_lookup(); // Range query is still an index operation

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let indexes = self.btree_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "btree index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        use std::ops::Bound;

        let start = match min_key {
            Some(k) => Bound::Included(k.to_vec()),
            None => Bound::Unbounded,
        };
        let end = match max_key {
            Some(k) => Bound::Included(k.to_vec()),
            None => Bound::Unbounded,
        };

        index.range((start, end))
    }

    /// Returns the number of entries in a hash index.
    pub fn hash_index_len(
        &self,
        collection_id: CollectionId,
        index_name: &str,
    ) -> CoreResult<usize> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let indexes = self.hash_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "hash index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        Ok(index.len())
    }

    /// Returns the number of entries in a BTree index.
    pub fn btree_index_len(
        &self,
        collection_id: CollectionId,
        index_name: &str,
    ) -> CoreResult<usize> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let indexes = self.btree_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_format(format!(
                "btree index '{}' not found on collection {}",
                index_name,
                collection_id.as_u32()
            ))
        })?;

        Ok(index.len())
    }

    /// Drops a hash index.
    pub fn drop_hash_index(
        &self,
        collection_id: CollectionId,
        index_name: &str,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let mut indexes = self.hash_indexes.write();

        Ok(indexes.remove(&idx_key).is_some())
    }

    /// Drops a BTree index.
    pub fn drop_btree_index(
        &self,
        collection_id: CollectionId,
        index_name: &str,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), index_name.to_string());
        let mut indexes = self.btree_indexes.write();

        Ok(indexes.remove(&idx_key).is_some())
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

/// Index tests.
#[cfg(test)]
mod index_tests {
    use super::*;

    fn create_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn create_hash_index() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "email", true).unwrap();

        // Should be able to insert
        let entity = EntityId::new();
        db.hash_index_insert(collection, "email", b"alice@example.com".to_vec(), entity)
            .unwrap();

        // Should be able to lookup
        let found = db.hash_index_lookup(collection, "email", b"alice@example.com").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], entity);
    }

    #[test]
    fn hash_index_multiple_entries() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "status", false).unwrap();

        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();

        db.hash_index_insert(collection, "status", b"active".to_vec(), e1).unwrap();
        db.hash_index_insert(collection, "status", b"active".to_vec(), e2).unwrap();
        db.hash_index_insert(collection, "status", b"inactive".to_vec(), e3).unwrap();

        let active = db.hash_index_lookup(collection, "status", b"active").unwrap();
        assert_eq!(active.len(), 2);

        let inactive = db.hash_index_lookup(collection, "status", b"inactive").unwrap();
        assert_eq!(inactive.len(), 1);
    }

    #[test]
    fn hash_index_unique_constraint() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "email", true).unwrap();

        let e1 = EntityId::new();
        let e2 = EntityId::new();

        db.hash_index_insert(collection, "email", b"alice@example.com".to_vec(), e1)
            .unwrap();

        // Duplicate should fail
        let result = db.hash_index_insert(collection, "email", b"alice@example.com".to_vec(), e2);
        assert!(result.is_err());
    }

    #[test]
    fn hash_index_remove() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "email", false).unwrap();

        let entity = EntityId::new();
        db.hash_index_insert(collection, "email", b"alice@example.com".to_vec(), entity)
            .unwrap();

        assert_eq!(db.hash_index_len(collection, "email").unwrap(), 1);

        db.hash_index_remove(collection, "email", b"alice@example.com", entity)
            .unwrap();

        assert_eq!(db.hash_index_len(collection, "email").unwrap(), 0);
    }

    #[test]
    fn create_btree_index() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_btree_index(collection, "age", false).unwrap();

        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();

        // Use big-endian bytes for proper ordering
        db.btree_index_insert(collection, "age", 25i64.to_be_bytes().to_vec(), e1)
            .unwrap();
        db.btree_index_insert(collection, "age", 30i64.to_be_bytes().to_vec(), e2)
            .unwrap();
        db.btree_index_insert(collection, "age", 35i64.to_be_bytes().to_vec(), e3)
            .unwrap();

        // Lookup exact
        let found = db.btree_index_lookup(collection, "age", &30i64.to_be_bytes()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], e2);
    }

    #[test]
    fn btree_index_range_query() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_btree_index(collection, "age", false).unwrap();

        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();
        let e4 = EntityId::new();

        db.btree_index_insert(collection, "age", 20i64.to_be_bytes().to_vec(), e1).unwrap();
        db.btree_index_insert(collection, "age", 25i64.to_be_bytes().to_vec(), e2).unwrap();
        db.btree_index_insert(collection, "age", 30i64.to_be_bytes().to_vec(), e3).unwrap();
        db.btree_index_insert(collection, "age", 35i64.to_be_bytes().to_vec(), e4).unwrap();

        // Range: 25 <= age <= 30
        let min = 25i64.to_be_bytes();
        let max = 30i64.to_be_bytes();
        let found = db.btree_index_range(collection, "age", Some(&min), Some(&max)).unwrap();
        assert_eq!(found.len(), 2);

        // Range: age >= 30
        let found = db.btree_index_range(collection, "age", Some(&max), None).unwrap();
        assert_eq!(found.len(), 2);

        // Range: age <= 25
        let found = db.btree_index_range(collection, "age", None, Some(&min)).unwrap();
        assert_eq!(found.len(), 2);

        // All
        let found = db.btree_index_range(collection, "age", None, None).unwrap();
        assert_eq!(found.len(), 4);
    }

    #[test]
    fn drop_index() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "email", true).unwrap();
        db.create_btree_index(collection, "age", false).unwrap();

        assert!(db.drop_hash_index(collection, "email").unwrap());
        assert!(db.drop_btree_index(collection, "age").unwrap());

        // Lookup on dropped index should fail
        assert!(db.hash_index_lookup(collection, "email", b"test").is_err());
        assert!(db.btree_index_lookup(collection, "age", b"test").is_err());
    }

    #[test]
    fn duplicate_index_creation_fails() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "email", true).unwrap();

        // Creating same index again should fail
        let result = db.create_hash_index(collection, "email", true);
        assert!(result.is_err());
    }

    #[test]
    fn index_not_found_error() {
        let db = create_db();
        let collection = db.collection("users");

        // Lookup on non-existent index should fail
        let result = db.hash_index_lookup(collection, "nonexistent", b"test");
        assert!(result.is_err());
    }
}

/// Observability tests for change feed and stats.
#[cfg(test)]
mod observability_tests {
    use super::*;
    use std::time::Duration;

    fn create_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn stats_track_operations() {
        let db = create_db();
        let collection = db.collection("items");
        let entity = EntityId::new();
        let payload = vec![1, 2, 3, 4, 5];

        // Initial stats should be zero
        let stats = db.stats();
        assert_eq!(stats.transactions_started, 0);
        assert_eq!(stats.transactions_committed, 0);

        // Perform a write transaction
        db.transaction(|txn| {
            txn.put(collection, entity, payload.clone())?;
            Ok(())
        })
        .unwrap();

        // Stats should reflect the transaction
        let stats = db.stats();
        assert_eq!(stats.transactions_started, 1);
        assert_eq!(stats.transactions_committed, 1);
        assert_eq!(stats.writes, 1);
        assert_eq!(stats.bytes_written, payload.len() as u64);

        // Read should be tracked
        let _data = db.get(collection, entity).unwrap();
        let stats = db.stats();
        assert_eq!(stats.reads, 1);
        assert_eq!(stats.bytes_read, payload.len() as u64);
    }

    #[test]
    fn stats_track_aborted_transactions() {
        let db = create_db();
        let collection = db.collection("items");

        // Start a transaction and abort it
        let mut txn = db.begin().unwrap();
        txn.put(collection, EntityId::new(), vec![1, 2, 3]).unwrap();
        db.abort(&mut txn).unwrap();

        // Stats should reflect the abort
        let stats = db.stats();
        assert_eq!(stats.transactions_started, 1);
        assert_eq!(stats.transactions_aborted, 1);
        assert_eq!(stats.transactions_committed, 0);
    }

    #[test]
    fn stats_track_scans() {
        let db = create_db();
        let collection = db.collection("items");

        // Put some data
        for i in 0..3 {
            let entity = EntityId::new();
            db.transaction(|txn| {
                txn.put(collection, entity, vec![i])?;
                Ok(())
            })
            .unwrap();
        }

        // List triggers a scan
        let _items = db.list(collection).unwrap();
        let stats = db.stats();
        assert_eq!(stats.scans, 1);
    }

    #[test]
    fn stats_track_checkpoints() {
        let db = create_db();
        let collection = db.collection("items");

        // Put some data
        db.transaction(|txn| {
            txn.put(collection, EntityId::new(), vec![1, 2, 3])?;
            Ok(())
        })
        .unwrap();

        // Checkpoint
        db.checkpoint().unwrap();

        let stats = db.stats();
        assert_eq!(stats.checkpoints, 1);
    }

    #[test]
    fn subscribe_receives_change_events() {
        let db = create_db();
        let collection = db.collection("items");
        let entity = EntityId::new();
        let payload = vec![10, 20, 30];

        // Subscribe before making changes
        let rx = db.subscribe();

        // Perform a write
        db.transaction(|txn| {
            txn.put(collection, entity, payload.clone())?;
            Ok(())
        })
        .unwrap();

        // Should receive the change event
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.collection_id, collection.as_u32());
        assert_eq!(event.entity_id, *entity.as_bytes());
        assert_eq!(
            event.change_type,
            crate::change_feed::ChangeType::Insert
        );
        assert_eq!(event.payload, Some(payload));
    }

    #[test]
    fn subscribe_receives_delete_events() {
        let db = create_db();
        let collection = db.collection("items");
        let entity = EntityId::new();
        let payload = vec![1, 2, 3];

        // Insert first
        db.transaction(|txn| {
            txn.put(collection, entity, payload.clone())?;
            Ok(())
        })
        .unwrap();

        // Subscribe
        let rx = db.subscribe();

        // Delete
        db.transaction(|txn| {
            txn.delete(collection, entity)?;
            Ok(())
        })
        .unwrap();

        // Should receive delete event
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, crate::change_feed::ChangeType::Delete);
        assert!(event.payload.is_none());
    }

    #[test]
    fn change_feed_poll_allows_catchup() {
        let db = create_db();
        let collection = db.collection("items");

        // Perform several writes
        for i in 0..5u8 {
            db.transaction(|txn| {
                txn.put(collection, EntityId::new(), vec![i])?;
                Ok(())
            })
            .unwrap();
        }

        // Poll from beginning
        let events = db.change_feed().poll(0, 10);
        assert_eq!(events.len(), 5);

        // Poll from middle
        let events = db.change_feed().poll(3, 10);
        assert_eq!(events.len(), 2); // sequences 4 and 5
    }

    #[test]
    fn stats_track_index_lookups() {
        let db = create_db();
        let collection = db.collection("users");

        // Create an index
        db.create_hash_index(collection, "email", false).unwrap();

        // Insert some data
        let entity = EntityId::new();
        db.hash_index_insert(collection, "email", b"test@example.com".to_vec(), entity)
            .unwrap();

        // Perform lookup
        let _results = db.hash_index_lookup(collection, "email", b"test@example.com").unwrap();

        let stats = db.stats();
        assert_eq!(stats.index_lookups, 1);
    }
}
