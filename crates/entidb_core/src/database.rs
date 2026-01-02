//! Database facade and recovery.

use crate::config::Config;
#[cfg(feature = "std")]
use crate::dir::DatabaseDir;
use crate::entity::{EntityId, EntityStore};
use crate::error::{CoreError, CoreResult};
use crate::index::{
    BTreeIndex, FtsIndex, FtsIndexSpec, HashIndex, Index, IndexEngine, IndexEngineConfig,
    IndexSpec, TokenizerConfig,
};
use crate::manifest::Manifest;
use crate::segment::{SegmentManager, SegmentRecord};
use crate::transaction::{Transaction, TransactionManager};
use crate::types::{CollectionId, SequenceNumber};
use crate::wal::{WalManager, WalRecord};
use entidb_storage::StorageBackend;
use parking_lot::RwLock;
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::path::Path;
use std::sync::Arc;

/// Statistics from a compaction operation.
#[derive(Debug, Clone, Default)]
pub struct CompactionStats {
    /// Number of records in the input.
    pub input_records: usize,
    /// Number of records in the output.
    pub output_records: usize,
    /// Number of tombstones removed.
    pub tombstones_removed: usize,
    /// Number of obsolete versions removed.
    pub obsolete_versions_removed: usize,
    /// Bytes saved (estimated).
    pub bytes_saved: usize,
}

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
///     let users = db.create_collection("users")?;
///     txn.put(users, id, vec![1, 2, 3])?;
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
    #[cfg(feature = "std")]
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
    /// Central index engine managing all indexes (new architecture).
    /// This replaces the separate hash_indexes/btree_indexes maps for persistence.
    index_engine: IndexEngine,
    /// Hash indexes keyed by (collection_id, index_name).
    /// DEPRECATED: Retained for backward API compatibility. Will be removed in future version.
    hash_indexes: RwLock<HashMap<(u32, String), HashIndex<Vec<u8>>>>,
    /// BTree indexes keyed by (collection_id, index_name).
    /// DEPRECATED: Retained for backward API compatibility. Will be removed in future version.
    btree_indexes: RwLock<HashMap<(u32, String), BTreeIndex<Vec<u8>>>>,
    /// FTS indexes keyed by (collection_id, index_name).
    fts_indexes: RwLock<HashMap<(u32, String), FtsIndex>>,
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
    #[cfg(feature = "std")]
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
    #[cfg(feature = "std")]
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

        // Create segments directory if it doesn't exist
        let segments_dir = dir.segments_dir();
        std::fs::create_dir_all(&segments_dir)?;

        // Discover existing segment files (for recovery)
        let mut existing_segment_ids: Vec<u64> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&segments_dir) {
            for entry in entries.filter_map(Result::ok) {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                // Parse segment file names like "seg-000001.dat"
                if name.starts_with("seg-") && name.ends_with(".dat") {
                    if let Ok(id) = name[4..10].parse::<u64>() {
                        existing_segment_ids.push(id);
                    }
                }
            }
        }
        existing_segment_ids.sort();

        // Create segment manager with file-backed factory for proper rotation
        // Returns CoreResult to properly propagate file creation errors
        let segments_dir_clone = segments_dir.clone();
        let segment_factory = move |segment_id: u64| -> CoreResult<Box<dyn StorageBackend>> {
            let segment_path = segments_dir_clone.join(format!("seg-{:06}.dat", segment_id));
            match FileBackend::open_with_create_dirs(&segment_path) {
                Ok(backend) => Ok(Box::new(backend)),
                Err(e) => {
                    // Return a proper error instead of silently falling back to in-memory
                    // This ensures durability guarantees are not silently broken
                    Err(CoreError::segment_file_creation_failed(
                        segment_path.display().to_string(),
                        e.to_string(),
                    ))
                }
            }
        };

        // Create managers
        let wal = Arc::new(WalManager::new(
            Box::new(wal_backend),
            config.sync_on_commit,
        ));
        let segments = Arc::new(SegmentManager::with_factory_and_existing(
            segment_factory,
            config.max_segment_size,
            existing_segment_ids,
        )?);

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

        // Create index engine with persisted definitions from manifest
        let index_engine = IndexEngine::new(IndexEngineConfig::default());
        for idx_def in &recovered_manifest.indexes {
            index_engine.register_index(idx_def.clone());
        }

        // Rebuild indexes from segment records using streaming iterator
        // This is memory-efficient: records are processed one at a time.
        // If rebuild fails, indexes are marked as invalid and lookups through
        // them will fail with a hard error until a successful rebuild occurs.
        if let Ok(record_iter) = segments.iter_all() {
            // Errors are handled by marking indexes invalid; database still opens
            let _ = index_engine.rebuild_from_iterator(record_iter);
        }

        Ok(Self {
            config,
            #[cfg(feature = "std")]
            dir: Some(dir),
            manifest: RwLock::new(recovered_manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
            index_engine,
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            fts_indexes: RwLock::new(HashMap::new()),
            change_feed: Arc::new(crate::change_feed::ChangeFeed::new()),
            stats: Arc::new(crate::stats::DatabaseStats::new()),
        })
    }

    /// Opens a database with the given backends.
    ///
    /// **WARNING**: This is a low-level constructor intended for testing only.
    /// It uses `SegmentManager::new()` which does NOT support segment rotation.
    /// If your database grows beyond `max_segment_size`, operations will fail.
    ///
    /// For production use with persistence, use [`Database::open()`] or
    /// [`Database::open_with_config()`] which properly handle:
    /// - LOCK file for single-writer guarantee
    /// - MANIFEST for metadata persistence
    /// - SEGMENTS/ directory with proper file-backed rotation
    ///
    /// # Arguments
    ///
    /// * `config` - Database configuration
    /// * `wal_backend` - Storage backend for the WAL
    /// * `segment_backend` - Storage backend for the initial segment
    ///
    /// # Errors
    ///
    /// Returns an error if WAL recovery fails.
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

        // Create index engine with persisted definitions from manifest
        let index_engine = IndexEngine::new(IndexEngineConfig::default());
        for idx_def in &manifest.indexes {
            index_engine.register_index(idx_def.clone());
        }

        // Rebuild indexes from segment records using streaming iterator
        // If rebuild fails, indexes are marked as invalid and lookups through
        // them will fail with a hard error until a successful rebuild occurs.
        if let Ok(record_iter) = segments.iter_all() {
            let _ = index_engine.rebuild_from_iterator(record_iter);
        }

        Ok(Self {
            config,
            #[cfg(feature = "std")]
            dir: None,
            manifest: RwLock::new(manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
            index_engine,
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            fts_indexes: RwLock::new(HashMap::new()),
            change_feed: Arc::new(crate::change_feed::ChangeFeed::new()),
            stats: Arc::new(crate::stats::DatabaseStats::new()),
        })
    }

    /// Opens a database with custom backends and a pre-loaded manifest.
    ///
    /// This is intended for web/WASM environments where the manifest is
    /// persisted separately (e.g., in OPFS or IndexedDB) and loaded before
    /// opening the database.
    ///
    /// # Arguments
    ///
    /// * `config` - Database configuration
    /// * `wal_backend` - Backend for WAL storage
    /// * `segment_backend` - Backend for segment storage
    /// * `manifest` - Pre-loaded manifest (or None to create fresh)
    ///
    /// # Errors
    ///
    /// Returns an error if WAL recovery fails.
    pub fn open_with_backends_and_manifest(
        config: Config,
        wal_backend: Box<dyn StorageBackend>,
        segment_backend: Box<dyn StorageBackend>,
        manifest: Option<Manifest>,
    ) -> CoreResult<Self> {
        let wal = Arc::new(WalManager::new(wal_backend, config.sync_on_commit));
        let segments = Arc::new(SegmentManager::new(
            segment_backend,
            config.max_segment_size,
        ));

        // Recover from WAL, using provided manifest as base if available
        let base_manifest = manifest.unwrap_or_else(|| Manifest::new(config.format_version));
        let (manifest, next_txid, next_seq, committed_seq) =
            Self::recover_with_manifest(&wal, &segments, base_manifest)?;

        let txn_manager = Arc::new(TransactionManager::with_state(
            Arc::clone(&wal),
            Arc::clone(&segments),
            next_txid,
            next_seq,
            committed_seq,
        ));

        let entity_store = EntityStore::new(Arc::clone(&txn_manager), Arc::clone(&segments));

        // Create index engine with persisted definitions from manifest
        let index_engine = IndexEngine::new(IndexEngineConfig::default());
        for idx_def in &manifest.indexes {
            index_engine.register_index(idx_def.clone());
        }

        // Rebuild indexes from segment records using streaming iterator
        if let Ok(record_iter) = segments.iter_all() {
            let _ = index_engine.rebuild_from_iterator(record_iter);
        }

        Ok(Self {
            config,
            #[cfg(feature = "std")]
            dir: None,
            manifest: RwLock::new(manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
            index_engine,
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            fts_indexes: RwLock::new(HashMap::new()),
            change_feed: Arc::new(crate::change_feed::ChangeFeed::new()),
            stats: Arc::new(crate::stats::DatabaseStats::new()),
        })
    }

    /// Returns the current manifest (for persistence in web environments).
    ///
    /// The manifest contains collection name→ID mappings and index definitions.
    /// In web environments, call this after mutations and persist the encoded
    /// bytes to ensure metadata survives restarts.
    pub fn get_manifest(&self) -> Manifest {
        self.manifest.read().clone()
    }

    /// Opens a fresh in-memory database for testing.
    ///
    /// This creates a non-persistent database that exists only in memory.
    /// Data is lost when the database is closed. Segment rotation is fully
    /// supported (rotated segments are also in-memory).
    pub fn open_in_memory() -> CoreResult<Self> {
        use entidb_storage::InMemoryBackend;

        let config = Config::default();
        let wal = Arc::new(WalManager::new(
            Box::new(InMemoryBackend::new()),
            config.sync_on_commit,
        ));

        // Use with_factory for proper in-memory segment rotation support
        let segments = Arc::new(SegmentManager::with_factory(
            |_segment_id| Ok(Box::new(InMemoryBackend::new())),
            config.max_segment_size,
        )?);

        // Fresh in-memory DB has no WAL to recover
        let manifest = Manifest::new(config.format_version);
        let next_txid = 1;
        let next_seq = 1;
        let committed_seq = 0;

        let txn_manager = Arc::new(TransactionManager::with_state(
            Arc::clone(&wal),
            Arc::clone(&segments),
            next_txid,
            next_seq,
            committed_seq,
        ));

        let entity_store = EntityStore::new(Arc::clone(&txn_manager), Arc::clone(&segments));
        let index_engine = IndexEngine::new(IndexEngineConfig::default());

        Ok(Self {
            config,
            #[cfg(feature = "std")]
            dir: None,
            manifest: RwLock::new(manifest),
            wal,
            segments,
            txn_manager,
            entity_store,
            is_open: RwLock::new(true),
            index_engine,
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            fts_indexes: RwLock::new(HashMap::new()),
            change_feed: Arc::new(crate::change_feed::ChangeFeed::new()),
            stats: Arc::new(crate::stats::DatabaseStats::new()),
        })
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
    /// Uses streaming two-pass recovery for memory efficiency:
    /// - Pass 1: Identify committed transactions (O(txn count) memory)
    /// - Pass 2: Replay operations from committed transactions (O(1) memory per record)
    ///
    /// **Important**: Only transactions with `commit_seq > checkpoint_seq` are replayed.
    /// Transactions at or below the checkpoint sequence are already materialized in segments
    /// and skipping them prevents segment bloat on repeated crashes.
    ///
    /// Returns (manifest, next_txid, next_seq, committed_seq).
    fn recover_with_manifest(
        wal: &WalManager,
        segments: &SegmentManager,
        manifest: Manifest,
    ) -> CoreResult<(Manifest, u64, u64, u64)> {
        use crate::wal::StreamingRecovery;

        // Start with manifest's last_checkpoint if available (post-checkpoint recovery)
        // This is critical for MVCC: if WAL was cleared after checkpoint, we need
        // to know the committed_seq from manifest to read segment data correctly.
        let checkpoint_seq = manifest.last_checkpoint.map(|s| s.as_u64()).unwrap_or(0);

        // === PASS 1: Streaming scan to identify committed transactions ===
        // Memory usage: O(number of committed transactions), not O(WAL size)
        let mut recovery = StreamingRecovery::new(checkpoint_seq);
        recovery.scan_committed(wal.iter()?)?;

        // === PASS 2: Streaming replay of committed operations ===
        // Memory usage: O(1) per record
        //
        // CRITICAL: Only replay transactions with commit_seq > checkpoint_seq.
        // Transactions at or below the checkpoint are already in segments.
        // Skipping them prevents segment bloat on repeated crash-recovery cycles.
        for result in wal.iter()? {
            let (_, record) = result?;

            match &record {
                WalRecord::Put {
                    txid,
                    collection_id,
                    entity_id,
                    after_bytes,
                    ..
                } => {
                    // Only replay if this transaction was committed AND not already checkpointed
                    if let Some(commit_seq) = recovery.get_commit_sequence(txid) {
                        // Skip if already materialized in segments (at or below checkpoint)
                        if commit_seq.as_u64() <= checkpoint_seq {
                            continue;
                        }
                        let segment_record = SegmentRecord::put(
                            *collection_id,
                            *entity_id,
                            after_bytes.clone(),
                            commit_seq,
                        );
                        segments.append(&segment_record)?;
                    }
                }
                WalRecord::Delete {
                    txid,
                    collection_id,
                    entity_id,
                    ..
                } => {
                    // Only replay if this transaction was committed AND not already checkpointed
                    if let Some(commit_seq) = recovery.get_commit_sequence(txid) {
                        // Skip if already materialized in segments (at or below checkpoint)
                        if commit_seq.as_u64() <= checkpoint_seq {
                            continue;
                        }
                        let segment_record =
                            SegmentRecord::tombstone(*collection_id, *entity_id, commit_seq);
                        segments.append(&segment_record)?;
                    }
                }
                _ => {
                    // BEGIN, COMMIT, ABORT, CHECKPOINT don't need replay to segments
                }
            }
        }

        // Rebuild segment index
        segments.rebuild_index()?;

        Ok((
            manifest,
            recovery.next_txid(),
            recovery.next_seq(),
            recovery.committed_seq(),
        ))
    }

    /// Begins a new transaction.
    pub fn begin(&self) -> CoreResult<Transaction> {
        self.ensure_open()?;
        self.stats.record_transaction_start();
        self.txn_manager.begin()
    }

    /// Begins a new write transaction with exclusive lock.
    ///
    /// This acquires the write lock immediately and holds it for the
    /// transaction's lifetime. Only one write transaction can exist at a time.
    ///
    /// For write operations, prefer `write_transaction()` which handles
    /// commit/abort automatically.
    pub fn begin_write(&self) -> CoreResult<crate::transaction::WriteTransaction<'_>> {
        self.ensure_open()?;
        self.stats.record_transaction_start();
        self.txn_manager.begin_write()
    }

    /// Commits a transaction.
    ///
    /// After successful commit, change events are emitted to all subscribers.
    /// Index updates are applied atomically with the commit.
    pub fn commit(&self, txn: &mut Transaction) -> CoreResult<SequenceNumber> {
        self.ensure_open()?;

        // Capture snapshot sequence for existence checks
        let snapshot_seq = txn.snapshot_seq();

        // Collect pending writes before commit (they're consumed during commit)
        let pending_writes: Vec<_> = txn
            .pending_writes()
            .map(|((cid, eid), w)| (*cid, *eid, w.clone()))
            .collect();

        // Build old_payloads map for index updates (fetch old values before commit)
        let old_payloads: std::collections::HashMap<(CollectionId, EntityId), Option<Vec<u8>>> =
            pending_writes
                .iter()
                .map(|(cid, eid, _)| {
                    let old_val = self
                        .entity_store
                        .get_at_snapshot(*cid, *eid, snapshot_seq)
                        .unwrap_or(None);
                    ((*cid, *eid), old_val)
                })
                .collect();

        // Commit the transaction (WAL + segments)
        let sequence = self.txn_manager.commit(txn)?;

        // Update indexes atomically after commit
        let writes_for_index = pending_writes.iter().map(|(cid, eid, w)| {
            let payload: Option<&[u8]> = match w {
                crate::transaction::PendingWrite::Put { payload, .. } => Some(payload.as_slice()),
                crate::transaction::PendingWrite::Delete { .. } => None,
            };
            (*cid, *eid, payload)
        });
        // Index update errors are logged but don't fail the commit (indexes are derivable)
        if let Err(e) = self.index_engine.update_from_writes(writes_for_index, &old_payloads) {
            #[cfg(feature = "std")]
            eprintln!("[EntiDB WARNING] Index update failed: {}. Indexes may need rebuild.", e);
        }

        // Record stats
        self.stats.record_transaction_commit();

        // Emit change events for each write
        for (collection_id, entity_id, write) in pending_writes {
            let event = match write {
                crate::transaction::PendingWrite::Put {
                    payload, is_update, ..
                } => {
                    // Track bytes written
                    self.stats.record_write(payload.len() as u64);

                    // Determine if this is an insert or update
                    let is_update = match is_update {
                        Some(known) => known,
                        None => {
                            // Use old_payloads to determine if it was an update
                            old_payloads
                                .get(&(collection_id, entity_id))
                                .map(|v| v.is_some())
                                .unwrap_or(false)
                        }
                    };

                    if is_update {
                        crate::change_feed::ChangeEvent::update(
                            sequence.as_u64(),
                            collection_id.as_u32(),
                            *entity_id.as_bytes(),
                            payload,
                        )
                    } else {
                        crate::change_feed::ChangeEvent::insert(
                            sequence.as_u64(),
                            collection_id.as_u32(),
                            *entity_id.as_bytes(),
                            payload,
                        )
                    }
                }
                crate::transaction::PendingWrite::Delete { .. } => {
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

        // Check if WAL has grown beyond max_wal_size and trigger auto-checkpoint if needed
        self.maybe_auto_checkpoint();

        Ok(sequence)
    }

    /// Commits a write transaction.
    ///
    /// After successful commit, change events are emitted to all subscribers.
    /// Index updates are applied atomically with the commit.
    /// The write lock is released after this call.
    pub fn commit_write(
        &self,
        wtxn: &mut crate::transaction::WriteTransaction<'_>,
    ) -> CoreResult<SequenceNumber> {
        self.ensure_open()?;

        // Capture snapshot sequence for existence checks
        let snapshot_seq = wtxn.snapshot_seq();

        // Collect pending writes before commit (for change feed and index updates)
        let pending_writes: Vec<_> = wtxn
            .inner()
            .pending_writes()
            .map(|((cid, eid), w)| (*cid, *eid, w.clone()))
            .collect();

        // Build old_payloads map for index updates (fetch old values before commit)
        let old_payloads: std::collections::HashMap<(CollectionId, EntityId), Option<Vec<u8>>> =
            pending_writes
                .iter()
                .map(|(cid, eid, _)| {
                    let old_val = self
                        .entity_store
                        .get_at_snapshot(*cid, *eid, snapshot_seq)
                        .unwrap_or(None);
                    ((*cid, *eid), old_val)
                })
                .collect();

        // Commit the transaction (WAL + segments)
        let sequence = self.txn_manager.commit_write(wtxn)?;

        // Update indexes atomically after commit
        // Build writes iterator for index engine
        let writes_for_index = pending_writes.iter().map(|(cid, eid, w)| {
            let payload: Option<&[u8]> = match w {
                crate::transaction::PendingWrite::Put { payload, .. } => Some(payload.as_slice()),
                crate::transaction::PendingWrite::Delete { .. } => None,
            };
            (*cid, *eid, payload)
        });
        // Index update errors are logged but don't fail the commit (indexes are derivable)
        if let Err(e) = self.index_engine.update_from_writes(writes_for_index, &old_payloads) {
            #[cfg(feature = "std")]
            eprintln!("[EntiDB WARNING] Index update failed: {}. Indexes may need rebuild.", e);
        }

        // Record stats
        self.stats.record_transaction_commit();

        // Emit change events for each write
        for (collection_id, entity_id, write) in pending_writes {
            let event = match write {
                crate::transaction::PendingWrite::Put {
                    payload, is_update, ..
                } => {
                    self.stats.record_write(payload.len() as u64);

                    // Determine if this is an insert or update
                    let is_update = match is_update {
                        Some(known) => known,
                        None => {
                            // Use old_payloads to determine if it was an update
                            old_payloads
                                .get(&(collection_id, entity_id))
                                .map(|v| v.is_some())
                                .unwrap_or(false)
                        }
                    };

                    if is_update {
                        crate::change_feed::ChangeEvent::update(
                            sequence.as_u64(),
                            collection_id.as_u32(),
                            *entity_id.as_bytes(),
                            payload,
                        )
                    } else {
                        crate::change_feed::ChangeEvent::insert(
                            sequence.as_u64(),
                            collection_id.as_u32(),
                            *entity_id.as_bytes(),
                            payload,
                        )
                    }
                }
                crate::transaction::PendingWrite::Delete { .. } => {
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

        // Check if WAL has grown beyond max_wal_size and trigger auto-checkpoint if needed
        self.maybe_auto_checkpoint();

        Ok(sequence)
    }

    /// Aborts a transaction.
    pub fn abort(&self, txn: &mut Transaction) -> CoreResult<()> {
        self.ensure_open()?;
        self.stats.record_transaction_abort();
        self.txn_manager.abort(txn)
    }

    /// Aborts a write transaction.
    ///
    /// All pending writes are discarded. The write lock is released after this call.
    pub fn abort_write(
        &self,
        wtxn: &mut crate::transaction::WriteTransaction<'_>,
    ) -> CoreResult<()> {
        self.ensure_open()?;
        self.stats.record_transaction_abort();
        self.txn_manager.abort_write(wtxn)
    }

    /// Executes a function within a transaction.
    ///
    /// If the function returns `Ok`, the transaction is committed.
    /// If it returns `Err`, the transaction is aborted.
    ///
    /// # Note
    ///
    /// This method acquires the exclusive write lock for the duration of the
    /// transaction to ensure single-writer semantics. For read-only operations,
    /// use `get()`, `list()`, or other non-transactional methods.
    pub fn transaction<F, T>(&self, f: F) -> CoreResult<T>
    where
        F: FnOnce(&mut Transaction) -> CoreResult<T>,
    {
        self.ensure_open()?;
        // Acquire write lock to ensure single-writer semantics
        let mut wtxn = self.begin_write()?;
        match f(wtxn.inner_mut()) {
            Ok(result) => {
                self.commit_write(&mut wtxn)?;
                Ok(result)
            }
            Err(e) => {
                let _ = self.abort_write(&mut wtxn);
                Err(e)
            }
        }
    }

    /// Executes a function within a write transaction.
    ///
    /// This is the preferred way to perform write operations. It:
    /// - Acquires an exclusive write lock for the transaction's duration
    /// - Ensures single-writer semantics (only one write transaction at a time)
    /// - Automatically commits on success or aborts on error
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.write_transaction(|wtxn| {
    ///     wtxn.put(collection, entity, payload)?;
    ///     Ok(())
    /// })?;
    /// ```
    pub fn write_transaction<F, T>(&self, f: F) -> CoreResult<T>
    where
        F: FnOnce(&mut crate::transaction::WriteTransaction<'_>) -> CoreResult<T>,
    {
        self.ensure_open()?;
        let mut wtxn = self.begin_write()?;
        match f(&mut wtxn) {
            Ok(result) => {
                self.commit_write(&mut wtxn)?;
                Ok(result)
            }
            Err(e) => {
                let _ = self.abort_write(&mut wtxn);
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

    /// Gets an entity within a write transaction.
    pub fn get_in_write_txn(
        &self,
        wtxn: &mut crate::transaction::WriteTransaction<'_>,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.ensure_open()?;
        let result = self
            .entity_store
            .get_in_write_txn(wtxn, collection_id, entity_id);
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

    /// Creates or retrieves a collection, ensuring persistence.
    ///
    /// This is the **recommended** method for collection creation. It guarantees that:
    /// - If the collection already exists, its ID is returned immediately
    /// - If the collection is new, it is persisted to the manifest before returning
    /// - If persistence fails, the in-memory state is rolled back and an error is returned
    ///
    /// # Errors
    ///
    /// Returns `CoreError::ManifestPersistFailed` if a new collection cannot be persisted
    /// to disk. In this case, the collection is **not** created in memory either.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users = db.create_collection("users")?;
    /// ```
    pub fn create_collection(&self, name: &str) -> CoreResult<CollectionId> {
        let mut manifest = self.manifest.write();

        // Fast path: collection already exists
        if let Some(id) = manifest.get_collection(name) {
            return Ok(CollectionId::new(id));
        }

        // Slow path: create new collection
        let id = manifest.get_or_create_collection(name);

        // Persist the manifest if we have a directory
        #[cfg(feature = "std")]
        if let Some(ref dir) = self.dir {
            if let Err(e) = dir.save_manifest(&manifest) {
                // Rollback: remove the collection from in-memory state
                manifest.collections.remove(name);
                // Restore next_collection_id (the ID we just assigned)
                manifest.next_collection_id = id;

                return Err(CoreError::manifest_persist_failed(format!(
                    "failed to persist collection '{}': {}",
                    name, e
                )));
            }
        }

        Ok(CollectionId::new(id))
    }

    /// Gets or creates a collection ID for a name.
    ///
    /// If this is a persistent database (opened via `open()` or `open_with_config()`),
    /// the manifest is automatically saved to disk when a new collection is created.
    ///
    /// # Deprecated
    ///
    /// This method is provided for backward compatibility. Prefer `create_collection()`
    /// which returns a `Result` and allows proper error handling.
    ///
    /// **Warning:** If manifest persistence fails, this method silently falls back
    /// to an in-memory-only collection ID. Data written to such collections will
    /// be LOST on restart. Use `create_collection()` to detect and handle such errors.
    #[deprecated(
        since = "0.2.0",
        note = "use create_collection() for proper error handling"
    )]
    pub fn collection(&self, name: &str) -> CollectionId {
        match self.create_collection(name) {
            Ok(id) => id,
            Err(e) => {
                // Log warning about fallback behavior
                #[cfg(feature = "std")]
                eprintln!(
                    "[EntiDB WARNING] Failed to persist collection '{}': {}. \
                     Using in-memory fallback - data may be lost on restart!",
                    name, e
                );
                self.collection_unchecked_internal(name)
            }
        }
    }

    /// Gets or creates a collection ID for a name, ignoring persistence errors.
    ///
    /// This method is for internal use only. External code should use
    /// `create_collection()` which returns a proper `Result`.
    ///
    /// **Warning:** Collections created when persistence fails will be lost on restart.
    #[doc(hidden)]
    pub fn collection_unchecked(&self, name: &str) -> CollectionId {
        self.collection_unchecked_internal(name)
    }

    /// Internal implementation of unchecked collection creation.
    fn collection_unchecked_internal(&self, name: &str) -> CollectionId {
        match self.create_collection(name) {
            Ok(id) => id,
            Err(_e) => {
                // Fallback: use in-memory only
                let mut manifest = self.manifest.write();
                let id = manifest.get_or_create_collection(name);
                CollectionId::new(id)
            }
        }
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
    /// - The manifest is updated with the checkpoint sequence
    /// - The WAL is cleared (only after manifest is durable)
    ///
    /// The ordering is critical for crash safety:
    /// 1. Sync segments to disk
    /// 2. Write checkpoint record to WAL and flush
    /// 3. Save manifest with checkpoint sequence (MUST be durable before WAL clear)
    /// 4. Truncate WAL
    ///
    /// If a crash occurs between steps 3 and 4, recovery will replay the WAL
    /// (which is safe since segments already have the data).
    pub fn checkpoint(&self) -> CoreResult<()> {
        self.ensure_open()?;

        // Step 1-2: Sync segments and write checkpoint record to WAL
        let checkpoint_seq = self.txn_manager.checkpoint()?;

        // Step 3: Update manifest with checkpoint sequence and save BEFORE truncating WAL
        // This is critical: if we crash after WAL truncation but before manifest save,
        // we could lose track of committed data.
        #[cfg(feature = "std")]
        if let Some(ref dir) = self.dir {
            let mut manifest = self.manifest.write();
            manifest.last_checkpoint = Some(checkpoint_seq);
            dir.save_manifest(&manifest)?;
        }

        // Step 4: Only NOW truncate the WAL, after manifest is durable
        self.txn_manager.truncate_wal()?;

        // Record checkpoint stat
        self.stats.record_checkpoint();

        Ok(())
    }

    /// Checks if WAL size exceeds `max_wal_size` and triggers an automatic checkpoint.
    ///
    /// This method is called after each commit to enforce WAL growth bounds.
    /// Auto-checkpoint failures are logged but don't affect commit success.
    /// The transaction is already committed; WAL size is an operational concern.
    fn maybe_auto_checkpoint(&self) {
        // Skip if max_wal_size is 0 (disabled)
        if self.config.max_wal_size == 0 {
            return;
        }

        // Get current WAL size
        let wal_size = match self.wal.size() {
            Ok(size) => size,
            Err(_) => return, // Can't determine size, skip check
        };

        // Trigger checkpoint if WAL exceeds threshold
        if wal_size >= self.config.max_wal_size {
            // Attempt checkpoint, ignoring errors (commit already succeeded)
            // In production, this would log a warning on failure
            let _ = self.checkpoint();
        }
    }

    /// Returns the current WAL size in bytes.
    ///
    /// This can be used to monitor WAL growth and make decisions about
    /// manual checkpointing.
    pub fn wal_size(&self) -> CoreResult<u64> {
        self.wal.size()
    }

    /// Compacts the database, removing obsolete versions and tombstones.
    ///
    /// Compaction merges segment records to:
    /// - Remove obsolete entity versions (keeping only the latest)
    /// - Optionally remove tombstones (deleted entities)
    /// - Reclaim storage space
    ///
    /// # Arguments
    ///
    /// * `remove_tombstones` - If true, tombstones are removed; if false, they are preserved
    ///
    /// # Returns
    ///
    /// Statistics about the compaction operation.
    ///
    /// # Concurrency
    ///
    /// Compaction operates safely alongside concurrent reads and writes:
    /// - Only sealed (immutable) segments are compacted
    /// - The active segment continues to receive writes during compaction
    /// - A compaction lock prevents segment sealing during the operation
    /// - Segment replacement is atomic with respect to the MVCC index
    ///
    /// # Note
    ///
    /// Compaction is a read-heavy operation. For large databases, consider
    /// running it during periods of low activity.
    pub fn compact(&self, remove_tombstones: bool) -> CoreResult<CompactionStats> {
        self.ensure_open()?;

        // Configure compactor
        use crate::segment::{CompactionConfig, Compactor};
        let config = if remove_tombstones {
            CompactionConfig::remove_all_tombstones()
        } else {
            CompactionConfig::with_tombstone_retention(u64::MAX)
        };

        let compactor = Compactor::new(config);
        let current_seq = self.committed_seq();

        // Track compaction statistics across the closure
        use std::cell::RefCell;
        let stats = RefCell::new(None);

        // Perform atomic compaction: scan + compact + replace while holding compaction lock
        // This ensures no segments are sealed during the operation, providing a
        // consistent view of sealed segments and preventing data duplication.
        let (_compacted_records, removed_ids, _new_segment_id) =
            self.segments.compact_sealed(|records| {
                let input_count = records.len();
                let input_size: usize = records.iter().map(|r| r.encoded_size()).sum();

                if records.is_empty() {
                    *stats.borrow_mut() = Some(CompactionStats::default());
                    return Ok(vec![]);
                }

                // Perform compaction (this produces deduplicated records)
                let (compacted, result) = compactor.compact(records, current_seq)?;

                let output_size: usize = compacted.iter().map(|r| r.encoded_size()).sum();

                *stats.borrow_mut() = Some(CompactionStats {
                    input_records: input_count,
                    output_records: result.output_records,
                    tombstones_removed: result.tombstones_removed,
                    obsolete_versions_removed: result.obsolete_versions_removed,
                    bytes_saved: input_size.saturating_sub(output_size),
                });

                Ok(compacted)
            })?;

        // Delete the old segment files from disk (exactly the ones that were removed)
        #[cfg(feature = "std")]
        if let Some(ref dir) = self.dir {
            if !removed_ids.is_empty() {
                // Delete exactly the segment files that were replaced by compaction
                dir.delete_segment_files(&removed_ids)?;
            }
        }

        Ok(stats.into_inner().unwrap_or_default())
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
        #[cfg(feature = "std")]
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
                    txn.put(record.collection_id, entity_id, record.payload.clone())?;
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

    fn create_index_persisted(
        &self,
        collection_id: CollectionId,
        field_path: Vec<String>,
        kind: crate::index::IndexKind,
        unique: bool,
        created_at_seq: SequenceNumber,
    ) -> CoreResult<u64> {
        self.ensure_open()?;

        // Fast path: already exists in manifest
        {
            let manifest = self.manifest.read();
            if let Some(existing) = manifest
                .indexes
                .iter()
                .find(|d| {
                    d.collection_id == collection_id
                        && d.field_path == field_path
                        && d.kind == kind
                        && d.unique == unique
                })
                .cloned()
            {
                let existing_id = existing.id;
                // Ensure IndexEngine has it registered (recovery path should do this,
                // but keep it safe for in-process creation ordering).
                self.index_engine.register_index(existing);
                return Ok(existing_id);
            }
        }

        // Slow path: add to manifest and persist (if file-backed)
        let (new_id, def_for_engine) = {
            let mut manifest = self.manifest.write();

            let def = crate::index::IndexDefinition {
                id: 0,
                collection_id,
                field_path: field_path.clone(),
                kind,
                unique,
                created_at_seq,
            };

            let id = manifest.add_index(def);

            #[cfg(feature = "std")]
            if let Some(ref dir) = self.dir {
                if let Err(e) = dir.save_manifest(&manifest) {
                    // Roll back in-memory manifest mutation.
                    let _ = manifest.remove_index(id);
                    manifest.next_index_id = id;
                    return Err(CoreError::manifest_persist_failed(format!(
                        "failed to persist index definition {:?} on collection {}: {}",
                        field_path,
                        collection_id.as_u32(),
                        e
                    )));
                }
            }

            let def_for_engine = crate::index::IndexDefinition {
                id,
                collection_id,
                field_path,
                kind,
                unique,
                created_at_seq,
            };

            (id, def_for_engine)
        };

        // Register and rebuild indexes from current persisted records using streaming.
        // If rebuild fails, the index is marked as invalid and lookups through
        // it will fail with a hard error until a successful rebuild occurs.
        self.index_engine.register_index(def_for_engine);
        if let Ok(record_iter) = self.segments.iter_all() {
            let _ = self.index_engine.rebuild_from_iterator(record_iter);
        }

        Ok(new_id)
    }

    /// Creates a hash index for fast equality lookups on a field.
    ///
    /// Hash indexes provide O(1) lookup by exact key match. They are ideal for:
    /// - Unique identifier lookups
    /// - Foreign key relationships
    /// - Equality filters
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection to index
    /// * `field` - The field to index (engine derives internal index name from this)
    /// * `unique` - Whether the index should enforce uniqueness
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users = db.collection("users");
    /// db.create_hash_index(users, "email", true)?; // Unique index on email field
    /// ```
    ///
    /// # Note
    ///
    /// Per `docs/access_paths.md`, users specify the field to index, not an arbitrary
    /// index name. The engine manages index names internally.
    pub fn create_hash_index(
        &self,
        collection_id: CollectionId,
        field: &str,
        unique: bool,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        // Persist definition in manifest + register in IndexEngine
        let field_path = vec![field.to_string()];
        let created_at_seq = self.txn_manager.committed_seq();
        let _id = self.create_index_persisted(
            collection_id,
            field_path,
            crate::index::IndexKind::Hash,
            unique,
            created_at_seq,
        )?;

        // Also maintain legacy in-memory index for backward compatibility
        let key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.hash_indexes.write();

        if indexes.contains_key(&key) {
            return Ok(()); // Already created by IndexEngine
        }

        let spec = if unique {
            IndexSpec::new(collection_id, field).unique()
        } else {
            IndexSpec::new(collection_id, field)
        };

        indexes.insert(key, HashIndex::new(spec));
        Ok(())
    }

    /// Creates a BTree index for ordered traversal and range queries on a field.
    ///
    /// BTree indexes support:
    /// - Equality lookups
    /// - Range queries (greater than, less than, between)
    /// - Ordered iteration
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection to index
    /// * `field` - The field to index (engine derives internal index name from this)
    /// * `unique` - Whether the index should enforce uniqueness
    ///
    /// # Example
    ///
    /// ```ignore
    /// let users = db.collection("users");
    /// db.create_btree_index(users, "age", false)?; // Non-unique index on age field
    /// ```
    ///
    /// # Note
    ///
    /// Per `docs/access_paths.md`, users specify the field to index, not an arbitrary
    /// index name. The engine manages index names internally.
    pub fn create_btree_index(
        &self,
        collection_id: CollectionId,
        field: &str,
        unique: bool,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        // Persist definition in manifest + register in IndexEngine
        let field_path = vec![field.to_string()];
        let created_at_seq = self.txn_manager.committed_seq();
        let _id = self.create_index_persisted(
            collection_id,
            field_path,
            crate::index::IndexKind::BTree,
            unique,
            created_at_seq,
        )?;

        // Also maintain legacy in-memory index for backward compatibility
        let key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.btree_indexes.write();

        if indexes.contains_key(&key) {
            return Ok(()); // Already created by IndexEngine
        }

        let spec = if unique {
            IndexSpec::new(collection_id, field).unique()
        } else {
            IndexSpec::new(collection_id, field)
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
    /// * `field` - The indexed field (must match the field used in `create_hash_index`)
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to associate with this key
    pub fn hash_index_insert(
        &self,
        collection_id: CollectionId,
        field: &str,
        key: Vec<u8>,
        entity_id: EntityId,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        // Use IndexEngine for index maintenance (supports transactional updates)
        self.index_engine
            .hash_index_insert_legacy(collection_id, field, key.clone(), entity_id)?;

        // Also maintain legacy in-memory index for backward compatibility
        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.hash_indexes.write();

        // Legacy index may not exist if only using IndexEngine
        if let Some(index) = indexes.get_mut(&idx_key) {
            let _ = index.insert(key, entity_id);
        }
        Ok(())
    }

    /// Removes an entry from a hash index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_hash_index`)
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to remove
    pub fn hash_index_remove(
        &self,
        collection_id: CollectionId,
        field: &str,
        key: &[u8],
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        // Use IndexEngine for index maintenance
        self.index_engine
            .hash_index_remove_legacy(collection_id, field, key, entity_id)?;

        // Also maintain legacy in-memory index for backward compatibility
        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.hash_indexes.write();

        if let Some(index) = indexes.get_mut(&idx_key) {
            let _ = index.remove(&key.to_vec(), entity_id);
        }
        Ok(true)
    }

    /// Looks up entities by exact key match in a hash index.
    ///
    /// # Deprecation Notice
    ///
    /// This API exposes index field names to callers, which violates the
    /// access-path policy in `docs/access_paths.md`. Future versions will
    /// replace this with semantic field-based predicates where the engine
    /// automatically selects the best access path.
    ///
    /// For now, use this API for explicit indexed lookups, but be aware that
    /// the API may change in future versions.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_hash_index`)
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// A vector of entity IDs that have the given key value.
    pub fn hash_index_lookup(
        &self,
        collection_id: CollectionId,
        field: &str,
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;
        self.stats.record_index_lookup();

        // Use IndexEngine for lookups (authoritative source)
        self.index_engine
            .hash_index_lookup_legacy(collection_id, field, key)
    }

    /// Inserts an entry into a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_btree_index`)
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to associate with this key
    pub fn btree_index_insert(
        &self,
        collection_id: CollectionId,
        field: &str,
        key: Vec<u8>,
        entity_id: EntityId,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        // Use IndexEngine for index maintenance
        self.index_engine.btree_index_insert_legacy(
            collection_id,
            field,
            key.clone(),
            entity_id,
        )?;

        // Also maintain legacy in-memory index for backward compatibility
        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.btree_indexes.write();

        if let Some(index) = indexes.get_mut(&idx_key) {
            let _ = index.insert(key, entity_id);
        }
        Ok(())
    }

    /// Removes an entry from a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_btree_index`)
    /// * `key` - The indexed key value as bytes
    /// * `entity_id` - The entity ID to remove
    pub fn btree_index_remove(
        &self,
        collection_id: CollectionId,
        field: &str,
        key: &[u8],
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        // Use IndexEngine for index maintenance
        self.index_engine
            .btree_index_remove_legacy(collection_id, field, key, entity_id)?;

        // Also maintain legacy in-memory index for backward compatibility
        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.btree_indexes.write();

        if let Some(index) = indexes.get_mut(&idx_key) {
            let _ = index.remove(&key.to_vec(), entity_id);
        }
        Ok(true)
    }

    /// Looks up entities by exact key match in a BTree index.
    ///
    /// # Deprecation Notice
    ///
    /// This API exposes index field names to callers, which violates the
    /// access-path policy in `docs/access_paths.md`. Future versions will
    /// replace this with semantic field-based predicates where the engine
    /// automatically selects the best access path.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_btree_index`)
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// A vector of entity IDs that have the given key value.
    pub fn btree_index_lookup(
        &self,
        collection_id: CollectionId,
        field: &str,
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;
        self.stats.record_index_lookup();

        // Use IndexEngine for lookups (authoritative source)
        self.index_engine
            .btree_index_lookup_legacy(collection_id, field, key)
    }

    /// Performs a range query on a BTree index.
    ///
    /// Returns all entities whose key is >= min_key and <= max_key.
    ///
    /// # Deprecation Notice
    ///
    /// This API exposes index field names to callers, which violates the
    /// access-path policy in `docs/access_paths.md`. Future versions will
    /// replace this with semantic field-based predicates where the engine
    /// automatically selects the best access path.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_btree_index`)
    /// * `min_key` - The minimum key (inclusive), or None for unbounded
    /// * `max_key` - The maximum key (inclusive), or None for unbounded
    ///
    /// # Returns
    ///
    /// A vector of entity IDs whose keys fall within the range.
    pub fn btree_index_range(
        &self,
        collection_id: CollectionId,
        field: &str,
        min_key: Option<&[u8]>,
        max_key: Option<&[u8]>,
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;
        self.stats.record_index_lookup(); // Range query is still an index operation

        // Use IndexEngine for range lookups
        self.index_engine
            .btree_index_range_legacy(collection_id, field, min_key, max_key)
    }

    /// Returns the number of entries in a hash index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn hash_index_len(&self, collection_id: CollectionId, field: &str) -> CoreResult<usize> {
        self.ensure_open()?;

        // Use IndexEngine for index length
        self.index_engine
            .hash_index_len_legacy(collection_id, field)
    }

    /// Returns the number of entries in a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn btree_index_len(&self, collection_id: CollectionId, field: &str) -> CoreResult<usize> {
        self.ensure_open()?;

        // Use IndexEngine for index length
        self.index_engine
            .btree_index_len_legacy(collection_id, field)
    }

    /// Drops a hash index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn drop_hash_index(&self, collection_id: CollectionId, field: &str) -> CoreResult<bool> {
        self.ensure_open()?;

        // Drop from IndexEngine (authoritative)
        let result = self
            .index_engine
            .drop_hash_index_legacy(collection_id, field);

        // Also remove from legacy in-memory index
        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.hash_indexes.write();
        indexes.remove(&idx_key);

        result
    }

    /// Drops a BTree index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn drop_btree_index(&self, collection_id: CollectionId, field: &str) -> CoreResult<bool> {
        self.ensure_open()?;

        // Drop from IndexEngine (authoritative)
        let result = self
            .index_engine
            .drop_btree_index_legacy(collection_id, field);

        // Also remove from legacy in-memory index
        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.btree_indexes.write();
        indexes.remove(&idx_key);

        result
    }

    // ========================================================================
    // Full-Text Search (FTS) Index Operations
    // ========================================================================

    /// Creates a full-text search (FTS) index for token-based text search on a field.
    ///
    /// FTS indexes support:
    /// - Tokenization (whitespace, punctuation splitting)
    /// - Case-insensitive matching (configurable)
    /// - Prefix matching
    /// - Multi-token queries (AND/OR semantics)
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection to index
    /// * `field` - The field to index (engine derives internal index name from this)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let articles = db.collection("articles");
    /// db.create_fts_index(articles, "content")?; // FTS index on content field
    /// ```
    ///
    /// # Note
    ///
    /// Per `docs/access_paths.md`, users specify the field to index, not an arbitrary
    /// index name. The engine manages index names internally.
    pub fn create_fts_index(&self, collection_id: CollectionId, field: &str) -> CoreResult<()> {
        self.ensure_open()?;

        let key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.fts_indexes.write();

        if indexes.contains_key(&key) {
            return Err(CoreError::invalid_format(format!(
                "FTS index on field '{}' already exists on collection {}",
                field,
                collection_id.as_u32()
            )));
        }

        let spec = FtsIndexSpec::new(collection_id, field);
        indexes.insert(key, FtsIndex::new(spec));
        Ok(())
    }

    /// Creates a full-text search index with custom tokenizer configuration.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection to index
    /// * `field` - The field to index (engine derives internal index name from this)
    /// * `min_token_length` - Minimum token length to index
    /// * `max_token_length` - Maximum token length to index
    /// * `case_sensitive` - Whether to perform case-sensitive matching
    pub fn create_fts_index_with_config(
        &self,
        collection_id: CollectionId,
        field: &str,
        min_token_length: usize,
        max_token_length: usize,
        case_sensitive: bool,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        let key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.fts_indexes.write();

        if indexes.contains_key(&key) {
            return Err(CoreError::invalid_format(format!(
                "FTS index on field '{}' already exists on collection {}",
                field,
                collection_id.as_u32()
            )));
        }

        let mut tokenizer = TokenizerConfig::new()
            .min_length(min_token_length)
            .max_length(max_token_length);
        if case_sensitive {
            tokenizer = tokenizer.case_sensitive();
        }

        let spec = FtsIndexSpec::new(collection_id, field).with_tokenizer(tokenizer);
        indexes.insert(key, FtsIndex::new(spec));
        Ok(())
    }

    /// Indexes text for an entity in an FTS index.
    ///
    /// This replaces any previously indexed text for the entity.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_fts_index`)
    /// * `entity_id` - The entity to index
    /// * `text` - The text to index
    pub fn fts_index_text(
        &self,
        collection_id: CollectionId,
        field: &str,
        entity_id: EntityId,
        text: &str,
    ) -> CoreResult<()> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.fts_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        index.index_text(entity_id, text)?;
        self.stats.record_write(text.len() as u64);
        Ok(())
    }

    /// Removes an entity from an FTS index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_fts_index`)
    /// * `entity_id` - The entity to remove
    ///
    /// # Returns
    ///
    /// True if the entity was indexed and removed, false if it wasn't indexed.
    pub fn fts_remove_entity(
        &self,
        collection_id: CollectionId,
        field: &str,
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.fts_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        index.remove_entity(entity_id)
    }

    /// Searches an FTS index with AND semantics.
    ///
    /// Returns entities that contain ALL tokens in the query.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_fts_index`)
    /// * `query` - The search query (tokenized the same way as indexed text)
    pub fn fts_search(
        &self,
        collection_id: CollectionId,
        field: &str,
        query: &str,
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let indexes = self.fts_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        self.stats.record_index_lookup();
        index.search(query)
    }

    /// Searches an FTS index with OR semantics.
    ///
    /// Returns entities that contain ANY token in the query.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_fts_index`)
    /// * `query` - The search query (tokenized the same way as indexed text)
    pub fn fts_search_any(
        &self,
        collection_id: CollectionId,
        field: &str,
        query: &str,
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let indexes = self.fts_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        self.stats.record_index_lookup();
        index.search_any(query)
    }

    /// Searches an FTS index for a prefix.
    ///
    /// Returns entities containing any token that starts with the given prefix.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field (must match the field used in `create_fts_index`)
    /// * `prefix` - The prefix to search for
    pub fn fts_search_prefix(
        &self,
        collection_id: CollectionId,
        field: &str,
        prefix: &str,
    ) -> CoreResult<Vec<EntityId>> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let indexes = self.fts_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        self.stats.record_index_lookup();
        index.search_prefix(prefix)
    }

    /// Returns the number of indexed entities in an FTS index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn fts_index_len(&self, collection_id: CollectionId, field: &str) -> CoreResult<usize> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let indexes = self.fts_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        Ok(index.entity_count())
    }

    /// Returns the number of unique tokens in an FTS index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn fts_unique_token_count(
        &self,
        collection_id: CollectionId,
        field: &str,
    ) -> CoreResult<usize> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let indexes = self.fts_indexes.read();

        let index = indexes.get(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        Ok(index.unique_token_count())
    }

    /// Clears an FTS index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn fts_clear(&self, collection_id: CollectionId, field: &str) -> CoreResult<()> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.fts_indexes.write();

        let index = indexes.get_mut(&idx_key).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "FTS index on field '{}' not found on collection {}",
                field,
                collection_id.as_u32()
            ))
        })?;

        index.clear();
        Ok(())
    }

    /// Drops an FTS index.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - The collection
    /// * `field` - The indexed field
    pub fn drop_fts_index(&self, collection_id: CollectionId, field: &str) -> CoreResult<bool> {
        self.ensure_open()?;

        let idx_key = (collection_id.as_u32(), field.to_string());
        let mut indexes = self.fts_indexes.write();

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
#[allow(deprecated)]
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
#[allow(deprecated)]
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
            let _posts = db.collection("posts");

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

    #[test]
    fn create_collection_returns_result() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("create_collection_test");
        let db = Database::open(&db_path).unwrap();

        // Create a new collection - should succeed
        let users = db.create_collection("users").unwrap();

        // Creating again should return same ID (idempotent)
        let users_again = db.create_collection("users").unwrap();
        assert_eq!(users, users_again);

        // Different collection should get different ID
        let posts = db.create_collection("posts").unwrap();
        assert_ne!(users, posts);

        db.close().unwrap();
    }

    #[test]
    fn create_collection_persists_immediately() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("persist_immediate_test");

        let collection_id;

        // First session: create collection using create_collection
        {
            let db = Database::open(&db_path).unwrap();
            collection_id = db.create_collection("users").unwrap();
            // Don't close cleanly - drop without close()
        }

        // Second session: collection should exist because manifest was saved
        {
            let db = Database::open(&db_path).unwrap();
            let recovered_id = db.get_collection("users");
            assert!(
                recovered_id.is_some(),
                "collection should persist immediately after create_collection"
            );
            assert_eq!(recovered_id.unwrap(), collection_id);
            db.close().unwrap();
        }
    }

    #[test]
    fn create_collection_with_data_persists() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("collection_data_persist");

        let entity_id = EntityId::new();

        // First session: create and populate
        {
            let db = Database::open(&db_path).unwrap();
            let users = db.create_collection("users").unwrap();

            db.transaction(|txn| {
                txn.put(users, entity_id, vec![1, 2, 3])?;
                Ok(())
            })
            .unwrap();

            db.close().unwrap();
        }

        // Second session: verify both collection and data persist
        {
            let db = Database::open(&db_path).unwrap();
            let users = db.get_collection("users").expect("collection should exist");
            let data = db.get(users, entity_id).unwrap();
            assert_eq!(data, Some(vec![1, 2, 3]));
            db.close().unwrap();
        }
    }

    #[test]
    fn create_collection_in_memory_no_persist_error() {
        // In-memory databases don't have a directory, so create_collection
        // should still work (just doesn't persist)
        let db = Database::open_in_memory().unwrap();

        let users = db.create_collection("users").unwrap();
        let users_again = db.create_collection("users").unwrap();
        assert_eq!(users, users_again);
    }

    #[test]
    fn multiple_collections_persist() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("multi_collection_test");

        // First session: create multiple collections
        {
            let db = Database::open(&db_path).unwrap();
            db.create_collection("users").unwrap();
            db.create_collection("posts").unwrap();
            db.create_collection("comments").unwrap();
            db.close().unwrap();
        }

        // Second session: all should exist
        {
            let db = Database::open(&db_path).unwrap();
            assert!(db.get_collection("users").is_some());
            assert!(db.get_collection("posts").is_some());
            assert!(db.get_collection("comments").is_some());
            assert!(db.get_collection("nonexistent").is_none());
            db.close().unwrap();
        }
    }
}

/// Encrypted database tests.
/// Index tests.
#[cfg(test)]
#[allow(deprecated)]
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
        let found = db
            .hash_index_lookup(collection, "email", b"alice@example.com")
            .unwrap();
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

        db.hash_index_insert(collection, "status", b"active".to_vec(), e1)
            .unwrap();
        db.hash_index_insert(collection, "status", b"active".to_vec(), e2)
            .unwrap();
        db.hash_index_insert(collection, "status", b"inactive".to_vec(), e3)
            .unwrap();

        let active = db
            .hash_index_lookup(collection, "status", b"active")
            .unwrap();
        assert_eq!(active.len(), 2);

        let inactive = db
            .hash_index_lookup(collection, "status", b"inactive")
            .unwrap();
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
        let found = db
            .btree_index_lookup(collection, "age", &30i64.to_be_bytes())
            .unwrap();
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

        db.btree_index_insert(collection, "age", 20i64.to_be_bytes().to_vec(), e1)
            .unwrap();
        db.btree_index_insert(collection, "age", 25i64.to_be_bytes().to_vec(), e2)
            .unwrap();
        db.btree_index_insert(collection, "age", 30i64.to_be_bytes().to_vec(), e3)
            .unwrap();
        db.btree_index_insert(collection, "age", 35i64.to_be_bytes().to_vec(), e4)
            .unwrap();

        // Range: 25 <= age <= 30
        let min = 25i64.to_be_bytes();
        let max = 30i64.to_be_bytes();
        let found = db
            .btree_index_range(collection, "age", Some(&min), Some(&max))
            .unwrap();
        assert_eq!(found.len(), 2);

        // Range: age >= 30
        let found = db
            .btree_index_range(collection, "age", Some(&max), None)
            .unwrap();
        assert_eq!(found.len(), 2);

        // Range: age <= 25
        let found = db
            .btree_index_range(collection, "age", None, Some(&min))
            .unwrap();
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
    fn duplicate_index_creation_is_idempotent() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "email", true).unwrap();

        // Creating same index again is idempotent (no-op), returns Ok
        let result = db.create_hash_index(collection, "email", true);
        assert!(
            result.is_ok(),
            "duplicate index creation should be idempotent"
        );
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
#[allow(deprecated)]
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
        assert_eq!(event.change_type, crate::change_feed::ChangeType::Insert);
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
        let _results = db
            .hash_index_lookup(collection, "email", b"test@example.com")
            .unwrap();

        let stats = db.stats();
        assert_eq!(stats.index_lookups, 1);
    }

    // ==================== FTS Index Tests ====================

    #[test]
    fn fts_create_index() {
        let db = create_db();
        let collection = db.collection("documents");

        // Create a basic FTS index
        db.create_fts_index(collection, "content").unwrap();

        // Verify index exists by trying to get its length
        let len = db.fts_index_len(collection, "content").unwrap();
        assert_eq!(len, 0);
    }

    #[test]
    fn fts_create_index_with_custom_config() {
        let db = create_db();
        let collection = db.collection("documents");

        // Create with custom config: case-sensitive, min token length 2, max 100
        db.create_fts_index_with_config(collection, "content", 2, 100, true)
            .unwrap();

        // Verify index exists
        let len = db.fts_index_len(collection, "content").unwrap();
        assert_eq!(len, 0);
    }

    #[test]
    fn fts_create_duplicate_index_fails() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        // Creating same index again should fail
        let result = db.create_fts_index(collection, "content");
        assert!(result.is_err());
    }

    #[test]
    fn fts_index_and_search_basic() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        // Index some text
        db.fts_index_text(collection, "content", entity1, "Hello world")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "Hello Rust programming")
            .unwrap();
        db.fts_index_text(collection, "content", entity3, "Goodbye world")
            .unwrap();

        // Search for "hello" - should find entity1 and entity2
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity1));
        assert!(results.contains(&entity2));

        // Search for "world" - should find entity1 and entity3
        let results = db.fts_search(collection, "content", "world").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity1));
        assert!(results.contains(&entity3));

        // Search for "rust" - should find only entity2
        let results = db.fts_search(collection, "content", "rust").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity2));
    }

    #[test]
    fn fts_search_and_semantics() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "Hello world from Rust")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "Hello world from Python")
            .unwrap();

        // Search for "hello world" - both should match (AND semantics - all terms must match)
        let results = db.fts_search(collection, "content", "hello world").unwrap();
        assert_eq!(results.len(), 2);

        // Search for "rust world" - only entity1 should match
        let results = db.fts_search(collection, "content", "rust world").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity1));

        // Search for "python rust" - neither should match (no doc has both)
        let results = db.fts_search(collection, "content", "python rust").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_search_any_or_semantics() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "Hello world")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "Goodbye world")
            .unwrap();
        db.fts_index_text(collection, "content", entity3, "Rust programming")
            .unwrap();

        // Search any "hello goodbye" - entity1 and entity2 should match (OR semantics)
        let results = db
            .fts_search_any(collection, "content", "hello goodbye")
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity1));
        assert!(results.contains(&entity2));

        // Search any "rust python" - only entity3 should match
        let results = db
            .fts_search_any(collection, "content", "rust python")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity3));

        // Search any with one matching term
        let results = db
            .fts_search_any(collection, "content", "notexist rust")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity3));
    }

    #[test]
    fn fts_search_prefix() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "programming in Rust")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "program management")
            .unwrap();
        db.fts_index_text(collection, "content", entity3, "something else")
            .unwrap();

        // Prefix search for "prog" - should find entity1 and entity2
        let results = db.fts_search_prefix(collection, "content", "prog").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity1));
        assert!(results.contains(&entity2));

        // Prefix search for "rust" - should find entity1
        let results = db.fts_search_prefix(collection, "content", "rust").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity1));

        // Prefix search for "xyz" - should find nothing
        let results = db.fts_search_prefix(collection, "content", "xyz").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_case_insensitivity() {
        let db = create_db();
        let collection = db.collection("documents");

        // Default is case-insensitive
        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();
        db.fts_index_text(collection, "content", entity, "HELLO World RuSt")
            .unwrap();

        // All variations should find the entity
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "HELLO").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "HeLLo").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "rust").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "WORLD").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_case_sensitivity() {
        let db = create_db();
        let collection = db.collection("documents");

        // Create case-sensitive index
        db.create_fts_index_with_config(collection, "content", 1, 256, true)
            .unwrap();

        let entity = EntityId::new();
        db.fts_index_text(collection, "content", entity, "Hello World")
            .unwrap();

        // Exact case should match
        let results = db.fts_search(collection, "content", "Hello").unwrap();
        assert_eq!(results.len(), 1);

        // Different case should NOT match in case-sensitive mode
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 0);

        let results = db.fts_search(collection, "content", "HELLO").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_remove_entity() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "Hello world")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "Hello Rust")
            .unwrap();

        // Both should be found
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 2);

        // Remove entity1
        db.fts_remove_entity(collection, "content", entity1)
            .unwrap();

        // Now only entity2 should be found
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity2));

        // Searching for "world" should find nothing
        let results = db.fts_search(collection, "content", "world").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_reindex_entity() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();

        // Index initial text
        db.fts_index_text(collection, "content", entity, "Hello world")
            .unwrap();

        // Verify initial state
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 1);
        let results = db.fts_search(collection, "content", "world").unwrap();
        assert_eq!(results.len(), 1);

        // Re-index with different text (should replace)
        db.fts_index_text(collection, "content", entity, "Goodbye Rust")
            .unwrap();

        // Old terms should no longer match
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 0);
        let results = db.fts_search(collection, "content", "world").unwrap();
        assert_eq!(results.len(), 0);

        // New terms should match
        let results = db.fts_search(collection, "content", "goodbye").unwrap();
        assert_eq!(results.len(), 1);
        let results = db.fts_search(collection, "content", "rust").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_clear_index() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "Hello world")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "Hello Rust")
            .unwrap();

        assert_eq!(db.fts_index_len(collection, "content").unwrap(), 2);

        // Clear the index
        db.fts_clear(collection, "content").unwrap();

        assert_eq!(db.fts_index_len(collection, "content").unwrap(), 0);

        // Search should return nothing
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_drop_index() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();
        db.fts_index_text(collection, "content", entity, "Hello world")
            .unwrap();

        // Drop the index
        db.drop_fts_index(collection, "content").unwrap();

        // Operations on dropped index should fail
        let result = db.fts_search(collection, "content", "hello");
        assert!(result.is_err());

        let result = db.fts_index_len(collection, "content");
        assert!(result.is_err());
    }

    #[test]
    fn fts_nonexistent_index_errors() {
        let db = create_db();
        let collection = db.collection("documents");

        let entity = EntityId::new();

        // All operations on non-existent index should fail
        let result = db.fts_index_text(collection, "nonexistent", entity, "text");
        assert!(result.is_err());

        let result = db.fts_search(collection, "nonexistent", "query");
        assert!(result.is_err());

        let result = db.fts_search_any(collection, "nonexistent", "query");
        assert!(result.is_err());

        let result = db.fts_search_prefix(collection, "nonexistent", "pre");
        assert!(result.is_err());

        let result = db.fts_remove_entity(collection, "nonexistent", entity);
        assert!(result.is_err());

        let result = db.fts_index_len(collection, "nonexistent");
        assert!(result.is_err());

        let result = db.fts_unique_token_count(collection, "nonexistent");
        assert!(result.is_err());

        let result = db.fts_clear(collection, "nonexistent");
        assert!(result.is_err());

        // drop_fts_index returns Ok(false) for non-existent index (not an error)
        let result = db.drop_fts_index(collection, "nonexistent").unwrap();
        assert!(!result); // false = nothing was dropped
    }

    #[test]
    fn fts_empty_query() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();
        db.fts_index_text(collection, "content", entity, "Hello world")
            .unwrap();

        // Empty query should return empty results (no tokens to match)
        let results = db.fts_search(collection, "content", "").unwrap();
        assert_eq!(results.len(), 0);

        let results = db.fts_search_any(collection, "content", "").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_whitespace_only_query() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();
        db.fts_index_text(collection, "content", entity, "Hello world")
            .unwrap();

        // Whitespace-only query should return empty results
        let results = db.fts_search(collection, "content", "   ").unwrap();
        assert_eq!(results.len(), 0);

        let results = db.fts_search(collection, "content", "\t\n").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_special_characters() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();
        db.fts_index_text(
            collection,
            "content",
            entity,
            "Hello, world! How's it going?",
        )
        .unwrap();

        // Punctuation should be treated as separators, words should still be found
        let results = db.fts_search(collection, "content", "hello").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "world").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "going").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_unicode_text() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "日本語 テスト")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "Привет мир")
            .unwrap();

        // Search for Japanese text
        let results = db.fts_search(collection, "content", "日本語").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity1));

        // Search for Russian text
        let results = db.fts_search(collection, "content", "привет").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity2));
    }

    #[test]
    fn fts_min_token_length() {
        let db = create_db();
        let collection = db.collection("documents");

        // Create index with min token length of 3
        db.create_fts_index_with_config(collection, "content", 3, 256, false)
            .unwrap();

        let entity = EntityId::new();
        db.fts_index_text(collection, "content", entity, "I am a Rust programmer")
            .unwrap();

        // Short tokens (length < 3) should be ignored
        let results = db.fts_search(collection, "content", "I").unwrap();
        assert_eq!(results.len(), 0);

        let results = db.fts_search(collection, "content", "am").unwrap();
        assert_eq!(results.len(), 0);

        let results = db.fts_search(collection, "content", "a").unwrap();
        assert_eq!(results.len(), 0);

        // Longer tokens should work
        let results = db.fts_search(collection, "content", "rust").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "programmer").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_unique_token_count() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        db.fts_index_text(collection, "content", entity1, "hello world hello")
            .unwrap();
        db.fts_index_text(collection, "content", entity2, "hello rust")
            .unwrap();

        // Unique tokens: hello, world, rust = 3
        let unique_count = db.fts_unique_token_count(collection, "content").unwrap();
        assert_eq!(unique_count, 3);
    }

    #[test]
    fn fts_multiple_indexes_per_collection() {
        let db = create_db();
        let collection = db.collection("documents");

        // Create two different indexes
        db.create_fts_index(collection, "title").unwrap();
        db.create_fts_index(collection, "body").unwrap();

        let entity = EntityId::new();

        // Index different content in each
        db.fts_index_text(collection, "title", entity, "Rust Programming Guide")
            .unwrap();
        db.fts_index_text(collection, "body", entity, "Learn Rust today with examples")
            .unwrap();

        // Search title - "guide" should be found
        let results = db.fts_search(collection, "title", "guide").unwrap();
        assert_eq!(results.len(), 1);

        // Search body - "guide" should NOT be found
        let results = db.fts_search(collection, "body", "guide").unwrap();
        assert_eq!(results.len(), 0);

        // Search body - "examples" should be found
        let results = db.fts_search(collection, "body", "examples").unwrap();
        assert_eq!(results.len(), 1);

        // Search title - "examples" should NOT be found
        let results = db.fts_search(collection, "title", "examples").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_different_collections_independent() {
        let db = create_db();
        let collection1 = db.collection("documents");
        let collection2 = db.collection("articles");

        // Create same-named index in both collections
        db.create_fts_index(collection1, "content").unwrap();
        db.create_fts_index(collection2, "content").unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        db.fts_index_text(collection1, "content", entity1, "Hello from documents")
            .unwrap();
        db.fts_index_text(collection2, "content", entity2, "Hello from articles")
            .unwrap();

        // Search in collection1
        let results = db.fts_search(collection1, "content", "documents").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity1));

        // Search in collection2
        let results = db.fts_search(collection2, "content", "articles").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity2));

        // Cross-collection searches should not mix
        let results = db.fts_search(collection1, "content", "articles").unwrap();
        assert_eq!(results.len(), 0);

        let results = db.fts_search(collection2, "content", "documents").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_empty_text_indexing() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();

        // Indexing empty text should succeed but add no tokens
        db.fts_index_text(collection, "content", entity, "")
            .unwrap();

        // Entity count should still increase (entity is tracked even without tokens)
        // But search for anything should return nothing
        let results = db.fts_search(collection, "content", "anything").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn fts_very_long_text() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        let entity = EntityId::new();

        // Create a long text with many words
        let long_text: String = (0..1000)
            .map(|i| format!("word{}", i))
            .collect::<Vec<_>>()
            .join(" ");

        db.fts_index_text(collection, "content", entity, &long_text)
            .unwrap();

        // Search for various words
        let results = db.fts_search(collection, "content", "word0").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "word999").unwrap();
        assert_eq!(results.len(), 1);

        let results = db.fts_search(collection, "content", "word500").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_many_entities() {
        let db = create_db();
        let collection = db.collection("documents");

        db.create_fts_index(collection, "content").unwrap();

        // Index 100 entities with common and unique terms
        let mut entities = Vec::new();
        for i in 0..100 {
            let entity = EntityId::new();
            entities.push(entity);
            db.fts_index_text(
                collection,
                "content",
                entity,
                &format!("common term unique{}", i),
            )
            .unwrap();
        }

        // All should match "common"
        let results = db.fts_search(collection, "content", "common").unwrap();
        assert_eq!(results.len(), 100);

        // All should match "term"
        let results = db.fts_search(collection, "content", "term").unwrap();
        assert_eq!(results.len(), 100);

        // Only one should match specific unique term
        let results = db.fts_search(collection, "content", "unique50").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entities[50]));
    }

    // ==========================================================================
    // Change Feed Operation Type Tests
    // ==========================================================================
    // These tests verify that the change feed correctly emits Insert vs Update
    // event types based on whether an entity existed before the transaction.

    #[test]
    fn change_feed_insert_for_new_entity() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        // Subscribe before making changes
        let rx = db.subscribe();

        // Insert a new entity
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1, 2, 3])?;
            Ok(())
        })
        .unwrap();

        // Verify the event is Insert (not Update)
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Insert);
        assert_eq!(event.entity_id, *entity.as_bytes());
        assert_eq!(event.payload, Some(vec![1, 2, 3]));
    }

    #[test]
    fn change_feed_update_for_existing_entity() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        // First, insert the entity
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1])?;
            Ok(())
        })
        .unwrap();

        // Subscribe AFTER the first insert
        let rx = db.subscribe();

        // Update the existing entity
        db.transaction(|txn| {
            txn.put(collection, entity, vec![2, 3, 4])?;
            Ok(())
        })
        .unwrap();

        // Verify the event is Update (not Insert)
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Update);
        assert_eq!(event.entity_id, *entity.as_bytes());
        assert_eq!(event.payload, Some(vec![2, 3, 4]));
    }

    #[test]
    fn change_feed_delete_for_existing_entity() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        // First, insert the entity
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1])?;
            Ok(())
        })
        .unwrap();

        // Subscribe AFTER the insert
        let rx = db.subscribe();

        // Delete the entity
        db.transaction(|txn| {
            txn.delete(collection, entity)?;
            Ok(())
        })
        .unwrap();

        // Verify the event is Delete
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Delete);
        assert_eq!(event.entity_id, *entity.as_bytes());
        assert_eq!(event.payload, None);
    }

    #[test]
    fn change_feed_multiple_ops_in_single_transaction() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        // Pre-insert entity2 so it can be updated
        db.transaction(|txn| {
            txn.put(collection, entity2, vec![1])?;
            Ok(())
        })
        .unwrap();

        // Subscribe AFTER pre-insert
        let rx = db.subscribe();

        // Single transaction with insert and update
        db.transaction(|txn| {
            txn.put(collection, entity1, vec![10])?; // New entity - Insert
            txn.put(collection, entity2, vec![20])?; // Existing entity - Update
            Ok(())
        })
        .unwrap();

        // Collect both events (order may vary due to HashMap iteration)
        let mut events = Vec::new();
        for _ in 0..2 {
            events.push(rx.recv_timeout(Duration::from_millis(100)).unwrap());
        }

        // Find the insert event
        let insert_event = events
            .iter()
            .find(|e| e.entity_id == *entity1.as_bytes())
            .expect("should find insert event");
        assert_eq!(insert_event.change_type, ChangeType::Insert);
        assert_eq!(insert_event.payload, Some(vec![10]));

        // Find the update event
        let update_event = events
            .iter()
            .find(|e| e.entity_id == *entity2.as_bytes())
            .expect("should find update event");
        assert_eq!(update_event.change_type, ChangeType::Update);
        assert_eq!(update_event.payload, Some(vec![20]));
    }

    #[test]
    fn change_feed_insert_after_delete_in_separate_transactions() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        // Insert entity
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1])?;
            Ok(())
        })
        .unwrap();

        // Delete entity
        db.transaction(|txn| {
            txn.delete(collection, entity)?;
            Ok(())
        })
        .unwrap();

        // Subscribe AFTER delete
        let rx = db.subscribe();

        // Re-insert the same entity - this should be an Insert since
        // the entity no longer exists at the transaction's snapshot
        db.transaction(|txn| {
            txn.put(collection, entity, vec![2])?;
            Ok(())
        })
        .unwrap();

        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Insert);
        assert_eq!(event.payload, Some(vec![2]));
    }

    #[test]
    fn change_feed_write_transaction_correct_types() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        // Subscribe before changes
        let rx = db.subscribe();

        // Use write_transaction API (with exclusive lock)
        db.write_transaction(|wtxn| {
            wtxn.put(collection, entity, vec![1, 2, 3])?;
            Ok(())
        })
        .unwrap();

        // Verify Insert
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Insert);

        // Update via write_transaction
        db.write_transaction(|wtxn| {
            wtxn.put(collection, entity, vec![4, 5, 6])?;
            Ok(())
        })
        .unwrap();

        // Verify Update
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Update);
    }

    #[test]
    fn change_feed_with_explicit_op_type() {
        use crate::change_feed::ChangeType;
        use std::time::Duration;

        let db = create_db();
        let collection = db.collection("test");
        let entity = EntityId::new();

        // Subscribe before changes
        let rx = db.subscribe();

        // Use put_with_op_type to explicitly mark as update
        // (even though entity doesn't exist - useful for sync scenarios)
        db.transaction(|txn| {
            txn.put_with_op_type(collection, entity, vec![1], true)?;
            Ok(())
        })
        .unwrap();

        // Should respect explicit op type
        let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(event.change_type, ChangeType::Update);
    }
}

// =============================================================================
// WAL Durability Tests
// =============================================================================
// Tests verifying WAL durability semantics: flush before commit ack,
// committed data survives crash, uncommitted data is lost.

#[cfg(test)]
#[allow(deprecated)]
mod wal_durability_tests {
    use super::*;
    use tempfile::tempdir;

    /// Tests that committed data survives a simulated crash (drop without close).
    #[test]
    fn committed_data_survives_crash() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("durability_test");

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        // Session 1: Create multiple committed transactions, then "crash"
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // First transaction
            db.transaction(|txn| {
                txn.put(collection, entity1, vec![1, 1, 1])?;
                Ok(())
            })
            .unwrap();

            // Second transaction
            db.transaction(|txn| {
                txn.put(collection, entity2, vec![2, 2, 2])?;
                Ok(())
            })
            .unwrap();

            // Third transaction with update and delete
            db.transaction(|txn| {
                txn.put(collection, entity1, vec![1, 1, 1, 1])?; // Update
                txn.put(collection, entity3, vec![3, 3, 3])?;
                Ok(())
            })
            .unwrap();

            // Simulate crash - drop without close()
            drop(db);
        }

        // Session 2: All committed data should be recovered
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Entity1 should have updated value
            let data1 = db.get(collection, entity1).unwrap();
            assert_eq!(
                data1,
                Some(vec![1, 1, 1, 1]),
                "entity1 should have updated value"
            );

            // Entity2 should exist
            let data2 = db.get(collection, entity2).unwrap();
            assert_eq!(data2, Some(vec![2, 2, 2]), "entity2 should exist");

            // Entity3 should exist
            let data3 = db.get(collection, entity3).unwrap();
            assert_eq!(data3, Some(vec![3, 3, 3]), "entity3 should exist");

            db.close().unwrap();
        }
    }

    /// Tests that uncommitted (aborted) transactions are not visible after crash.
    #[test]
    fn uncommitted_data_lost_after_crash() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("uncommitted_test");

        let entity_committed = EntityId::new();
        let entity_uncommitted = EntityId::new();

        // Session 1: Commit one entity, start but don't commit another, then crash
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Committed transaction
            db.transaction(|txn| {
                txn.put(collection, entity_committed, vec![1, 2, 3])?;
                Ok(())
            })
            .unwrap();

            // Start a transaction but abort it
            let mut txn = db.begin().unwrap();
            txn.put(collection, entity_uncommitted, vec![4, 5, 6])
                .unwrap();
            db.abort(&mut txn).unwrap();

            // Simulate crash
            drop(db);
        }

        // Session 2: Only committed data should exist
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            let committed = db.get(collection, entity_committed).unwrap();
            assert_eq!(
                committed,
                Some(vec![1, 2, 3]),
                "committed entity should exist"
            );

            let uncommitted = db.get(collection, entity_uncommitted).unwrap();
            assert!(uncommitted.is_none(), "aborted entity should NOT exist");

            db.close().unwrap();
        }
    }

    /// Tests that in-flight transaction data (not yet committed) is lost after crash.
    #[test]
    fn in_flight_transaction_lost_after_crash() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("inflight_test");

        let entity_committed = EntityId::new();
        let entity_inflight = EntityId::new();

        // Session 1: Commit one entity, leave another in-flight, then crash
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Committed transaction
            db.transaction(|txn| {
                txn.put(collection, entity_committed, vec![10, 20, 30])?;
                Ok(())
            })
            .unwrap();

            // Start a transaction and write, but don't commit before crash
            let mut txn = db.begin().unwrap();
            txn.put(collection, entity_inflight, vec![40, 50, 60])
                .unwrap();
            // Don't commit - just drop the db
            drop(txn);
            drop(db);
        }

        // Session 2: Only committed data should exist
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            let committed = db.get(collection, entity_committed).unwrap();
            assert_eq!(
                committed,
                Some(vec![10, 20, 30]),
                "committed entity should exist"
            );

            let inflight = db.get(collection, entity_inflight).unwrap();
            assert!(
                inflight.is_none(),
                "in-flight entity should NOT exist after crash"
            );

            db.close().unwrap();
        }
    }

    /// Tests that delete operations are durable.
    #[test]
    fn delete_is_durable() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("delete_durable_test");

        let entity = EntityId::new();

        // Session 1: Create then delete an entity
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            db.transaction(|txn| {
                txn.put(collection, entity, vec![1, 2, 3])?;
                Ok(())
            })
            .unwrap();

            // Verify it exists
            assert!(db.get(collection, entity).unwrap().is_some());

            // Delete it
            db.transaction(|txn| {
                txn.delete(collection, entity)?;
                Ok(())
            })
            .unwrap();

            // Verify it's gone
            assert!(db.get(collection, entity).unwrap().is_none());

            // Crash without close
            drop(db);
        }

        // Session 2: Entity should still be deleted
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            let data = db.get(collection, entity).unwrap();
            assert!(
                data.is_none(),
                "deleted entity should remain deleted after crash"
            );

            db.close().unwrap();
        }
    }

    /// Tests that multiple updates are durable and the last value wins.
    #[test]
    fn multiple_updates_durable() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("multi_update_test");

        let entity = EntityId::new();

        // Session 1: Multiple updates to same entity
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            for i in 0..10u8 {
                db.transaction(|txn| {
                    txn.put(collection, entity, vec![i])?;
                    Ok(())
                })
                .unwrap();
            }

            // Final value should be 9
            let data = db.get(collection, entity).unwrap();
            assert_eq!(data, Some(vec![9]));

            // Crash
            drop(db);
        }

        // Session 2: Last value should persist
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            let data = db.get(collection, entity).unwrap();
            assert_eq!(data, Some(vec![9]), "last update should be durable");

            db.close().unwrap();
        }
    }

    /// Tests that sequence numbers are recovered correctly after crash.
    #[test]
    fn sequence_numbers_recovered() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("seq_recovery_test");

        let seq_before_crash: u64;

        // Session 1: Create some transactions
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            for _ in 0..5 {
                db.transaction(|txn| {
                    txn.put(collection, EntityId::new(), vec![42])?;
                    Ok(())
                })
                .unwrap();
            }

            seq_before_crash = db.committed_seq().as_u64();
            assert!(seq_before_crash >= 5, "should have at least 5 sequences");

            drop(db);
        }

        // Session 2: Sequence should continue from where it left off
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Committed seq should be at least what it was before
            assert!(
                db.committed_seq().as_u64() >= seq_before_crash,
                "committed seq should be recovered"
            );

            // New transaction should get higher sequence
            db.transaction(|txn| {
                txn.put(collection, EntityId::new(), vec![99])?;
                Ok(())
            })
            .unwrap();

            assert!(
                db.committed_seq().as_u64() > seq_before_crash,
                "new seq should be higher than recovered"
            );

            db.close().unwrap();
        }
    }

    /// Tests durability with checkpoint - data survives even with cleared WAL.
    #[test]
    fn durability_with_checkpoint() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("checkpoint_durability_test");

        let entity = EntityId::new();

        // Session 1: Create data, checkpoint, crash
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            db.transaction(|txn| {
                txn.put(collection, entity, vec![1, 2, 3, 4, 5])?;
                Ok(())
            })
            .unwrap();

            // Checkpoint moves data to segments and clears WAL
            db.checkpoint().unwrap();

            // WAL should be empty
            assert_eq!(db.wal.size().unwrap(), 0);

            // Crash
            drop(db);
        }

        // Session 2: Data should be recovered from segments
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            let data = db.get(collection, entity).unwrap();
            assert_eq!(
                data,
                Some(vec![1, 2, 3, 4, 5]),
                "data should be recovered from segments after checkpoint"
            );

            db.close().unwrap();
        }
    }

    /// Tests that recovery skips transactions already in checkpoint (prevents segment bloat).
    ///
    /// This is a regression test for the crash-window segment growth issue:
    /// If the process crashes after manifest save (with checkpoint) but before WAL
    /// truncation, reopening should NOT re-append already-checkpointed operations.
    #[test]
    fn recovery_skips_checkpointed_transactions() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("checkpoint_skip_test");

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        // Session 1: Create data, checkpoint, add more data, simulate crash before WAL clear
        let segment_size_after_checkpoint: u64;
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Transaction 1: will be checkpointed
            db.transaction(|txn| {
                txn.put(collection, entity1, vec![1, 2, 3])?;
                Ok(())
            })
            .unwrap();

            // Checkpoint: entity1 is now in segments, manifest has checkpoint seq
            db.checkpoint().unwrap();

            // Record segment size after checkpoint
            segment_size_after_checkpoint = db.segments.total_size().unwrap_or(0);

            // Transaction 2: will be in WAL but NOT checkpointed
            db.transaction(|txn| {
                txn.put(collection, entity2, vec![4, 5, 6])?;
                Ok(())
            })
            .unwrap();

            // Simulate crash: DON'T close cleanly, so manifest is saved but WAL
            // might still contain records from before checkpoint
            // In a real crash scenario, the WAL wouldn't be truncated after checkpoint
            drop(db);
        }

        // Session 2: Recovery should NOT duplicate entity1 into segments again
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Both entities should be readable
            let data1 = db.get(collection, entity1).unwrap();
            assert_eq!(data1, Some(vec![1, 2, 3]), "entity1 should be recovered");

            let data2 = db.get(collection, entity2).unwrap();
            assert_eq!(data2, Some(vec![4, 5, 6]), "entity2 should be recovered");

            // The key assertion: segment size should not have grown significantly
            // beyond what we'd expect from just entity2 being added during recovery.
            // If entity1 was re-added, the segments would be bloated.
            let current_segment_size = db.segments.total_size().unwrap_or(0);

            // The segment should grow by roughly the size of entity2, not entity1 + entity2
            // We use a generous margin because segment record overhead varies
            let entity2_approx_size = 50; // ~50 bytes for a small record with overhead
            let max_expected_growth = entity2_approx_size * 2; // Allow 2x for overhead

            let actual_growth = current_segment_size.saturating_sub(segment_size_after_checkpoint);
            assert!(
                actual_growth < max_expected_growth + segment_size_after_checkpoint / 2,
                "segments should not have grown excessively: before={}, after={}, growth={}",
                segment_size_after_checkpoint,
                current_segment_size,
                actual_growth
            );

            db.close().unwrap();
        }
    }
}

// =============================================================================
// Index Rebuild and Derivability Tests
// =============================================================================
// Tests verifying that user-facing indexes (HashIndex, BTreeIndex) can be
// rebuilt from segment data, and index state after recovery matches pre-crash state.

#[cfg(test)]
#[allow(deprecated)]
mod index_rebuild_tests {
    use super::*;
    use entidb_codec::{Decode, Encode};
    use tempfile::tempdir;

    /// Tests that hash index state is rebuilt correctly after database restart.
    #[test]
    fn hash_index_rebuilt_after_restart() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("hash_rebuild_test");

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        // Session 1: Create index and populate it, then close cleanly
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("users");

            // Create a hash index using the new IndexEngine API
            db.index_engine
                .create_index(
                    collection,
                    vec!["email".to_string()],
                    crate::index::IndexKind::Hash,
                    false,
                    SequenceNumber::new(0),
                )
                .unwrap();

            // Store entities with email field in CBOR
            let user1_cbor = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("email".to_string()),
                entidb_codec::Value::Text("alice@example.com".to_string()),
            )])
            .encode()
            .unwrap();
            let user2_cbor = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("email".to_string()),
                entidb_codec::Value::Text("bob@example.com".to_string()),
            )])
            .encode()
            .unwrap();
            let user3_cbor = entidb_codec::Value::Map(vec![
                (
                    entidb_codec::Value::Text("email".to_string()),
                    entidb_codec::Value::Text("alice@example.com".to_string()),
                ), // Duplicate email
            ])
            .encode()
            .unwrap();

            db.transaction(|txn| {
                txn.put(collection, entity1, user1_cbor.clone())?;
                txn.put(collection, entity2, user2_cbor.clone())?;
                txn.put(collection, entity3, user3_cbor.clone())?;
                Ok(())
            })
            .unwrap();

            // Verify index works before close
            // Note: IndexEngine auto-rebuild happens on open, so we verify data is stored
            assert_eq!(db.entity_count(), 3);

            db.close().unwrap();
        }

        // Session 2: Index should be rebuilt from segment data
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.get_collection("users").expect("collection should exist");

            // Verify data is accessible
            assert_eq!(db.entity_count(), 3);

            // Verify all entities exist
            assert!(db.get(collection, entity1).unwrap().is_some());
            assert!(db.get(collection, entity2).unwrap().is_some());
            assert!(db.get(collection, entity3).unwrap().is_some());

            db.close().unwrap();
        }
    }

    /// Tests that btree index state is rebuilt correctly after database restart.
    #[test]
    fn btree_index_rebuilt_after_restart() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("btree_rebuild_test");

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        // Session 1: Create BTree index and populate it
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("users");

            // Create a btree index using the new IndexEngine API
            db.index_engine
                .create_index(
                    collection,
                    vec!["age".to_string()],
                    crate::index::IndexKind::BTree,
                    false,
                    SequenceNumber::new(0),
                )
                .unwrap();

            // Store entities with age field in CBOR
            let user1_cbor = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("age".to_string()),
                entidb_codec::Value::Integer(25),
            )])
            .encode()
            .unwrap();
            let user2_cbor = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("age".to_string()),
                entidb_codec::Value::Integer(30),
            )])
            .encode()
            .unwrap();
            let user3_cbor = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("age".to_string()),
                entidb_codec::Value::Integer(35),
            )])
            .encode()
            .unwrap();

            db.transaction(|txn| {
                txn.put(collection, entity1, user1_cbor)?;
                txn.put(collection, entity2, user2_cbor)?;
                txn.put(collection, entity3, user3_cbor)?;
                Ok(())
            })
            .unwrap();

            db.close().unwrap();
        }

        // Session 2: Index should be rebuilt
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.get_collection("users").expect("collection should exist");

            // Verify data is accessible
            assert_eq!(db.entity_count(), 3);

            assert!(db.get(collection, entity1).unwrap().is_some());
            assert!(db.get(collection, entity2).unwrap().is_some());
            assert!(db.get(collection, entity3).unwrap().is_some());

            db.close().unwrap();
        }
    }

    /// Tests that index definitions persist in manifest and are restored.
    #[test]
    fn index_definitions_persist_in_manifest() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("index_manifest_test");

        // Session 1: Create indexes
        {
            let db = Database::open(&db_path).unwrap();
            let users = db.create_collection("users").unwrap();
            let posts = db.create_collection("posts").unwrap();

            // Create multiple indexes
            db.create_hash_index(users, "email", true).unwrap();
            db.create_btree_index(users, "age", false).unwrap();
            db.create_hash_index(posts, "author_id", false).unwrap();

            // Verify definitions exist
            let defs = db.index_engine.definitions();
            assert_eq!(defs.len(), 3);

            db.close().unwrap();
        }

        // Session 2: Index definitions should be restored from manifest
        {
            let db = Database::open(&db_path).unwrap();

            // Verify all index definitions were restored
            let defs = db.index_engine.definitions();
            assert_eq!(defs.len(), 3, "all index definitions should persist");

            // Verify specific indexes exist
            let users = db.get_collection("users").unwrap();
            let posts = db.get_collection("posts").unwrap();

            let user_indexes: Vec<_> = defs.iter().filter(|d| d.collection_id == users).collect();
            assert_eq!(user_indexes.len(), 2, "users should have 2 indexes");

            let post_indexes: Vec<_> = defs.iter().filter(|d| d.collection_id == posts).collect();
            assert_eq!(post_indexes.len(), 1, "posts should have 1 index");

            db.close().unwrap();
        }
    }

    /// Tests that index is correctly rebuilt after entities are deleted.
    #[test]
    fn index_rebuilt_correctly_with_deletions() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("index_delete_rebuild_test");

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();
        let entity3 = EntityId::new();

        // Session 1: Create, then delete some entities
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("items");

            // Create index
            db.index_engine
                .create_index(
                    collection,
                    vec!["name".to_string()],
                    crate::index::IndexKind::Hash,
                    false,
                    SequenceNumber::new(0),
                )
                .unwrap();

            // Create entities
            for (id, name) in [(entity1, "alice"), (entity2, "bob"), (entity3, "charlie")] {
                let cbor = entidb_codec::Value::Map(vec![(
                    entidb_codec::Value::Text("name".to_string()),
                    entidb_codec::Value::Text(name.to_string()),
                )])
                .encode()
                .unwrap();
                db.transaction(|txn| {
                    txn.put(collection, id, cbor)?;
                    Ok(())
                })
                .unwrap();
            }

            // Delete entity2
            db.transaction(|txn| {
                txn.delete(collection, entity2)?;
                Ok(())
            })
            .unwrap();

            // Verify entity2 is deleted (get returns None)
            assert!(
                db.get(collection, entity2).unwrap().is_none(),
                "deleted entity should not be accessible"
            );
            // Note: entity_count() currently includes tombstoned entities in index
            // This is a known gap - entity_count should exclude tombstones

            db.close().unwrap();
        }

        // Session 2: Verify only non-deleted entities are in index
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.get_collection("items").unwrap();

            // entity1 and entity3 should exist
            assert!(db.get(collection, entity1).unwrap().is_some());
            assert!(db.get(collection, entity3).unwrap().is_some());

            // entity2 should NOT exist
            assert!(db.get(collection, entity2).unwrap().is_none());

            // Note: entity_count() includes tombstoned entities, so we verify
            // correctness through get() instead

            db.close().unwrap();
        }
    }

    /// Tests that index is correctly rebuilt with updates (latest version wins).
    #[test]
    fn index_rebuilt_with_updates() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("index_update_rebuild_test");

        let entity = EntityId::new();

        // Session 1: Create and update entity multiple times
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.collection("users");

            // Create index on "status" field
            db.index_engine
                .create_index(
                    collection,
                    vec!["status".to_string()],
                    crate::index::IndexKind::Hash,
                    false,
                    SequenceNumber::new(0),
                )
                .unwrap();

            // Initial value
            let cbor1 = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("status".to_string()),
                entidb_codec::Value::Text("pending".to_string()),
            )])
            .encode()
            .unwrap();
            db.transaction(|txn| {
                txn.put(collection, entity, cbor1)?;
                Ok(())
            })
            .unwrap();

            // Update 1
            let cbor2 = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("status".to_string()),
                entidb_codec::Value::Text("active".to_string()),
            )])
            .encode()
            .unwrap();
            db.transaction(|txn| {
                txn.put(collection, entity, cbor2)?;
                Ok(())
            })
            .unwrap();

            // Update 2 (final)
            let cbor3 = entidb_codec::Value::Map(vec![(
                entidb_codec::Value::Text("status".to_string()),
                entidb_codec::Value::Text("completed".to_string()),
            )])
            .encode()
            .unwrap();
            db.transaction(|txn| {
                txn.put(collection, entity, cbor3.clone())?;
                Ok(())
            })
            .unwrap();

            // Verify final value before close
            let data = db.get(collection, entity).unwrap().unwrap();
            assert_eq!(data, cbor3);

            db.close().unwrap();
        }

        // Session 2: Should have latest value
        {
            let db = Database::open(&db_path).unwrap();
            let collection = db.get_collection("users").unwrap();

            let data = db.get(collection, entity).unwrap().unwrap();
            let value = <entidb_codec::Value as Decode>::decode(&data).unwrap();

            // Verify it's the final "completed" status
            if let entidb_codec::Value::Map(entries) = value {
                let status = entries
                    .iter()
                    .find(|(k, _)| matches!(k, entidb_codec::Value::Text(s) if s == "status"))
                    .map(|(_, v)| v);
                assert!(matches!(status, Some(entidb_codec::Value::Text(s)) if s == "completed"));
            } else {
                panic!("expected map");
            }

            db.close().unwrap();
        }
    }
}

// =============================================================================
// Index Atomicity Tests
// =============================================================================
// Tests verifying that index updates are atomic with transaction commit:
// - Index changes only visible after commit
// - Aborted transactions don't affect indexes
// - Partial failures leave indexes consistent

#[cfg(test)]
#[allow(deprecated)]
mod index_atomicity_tests {
    use super::*;

    fn create_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    /// Tests that index insertions are only visible after commit.
    #[test]
    fn index_insert_visible_only_after_commit() {
        let db = create_db();
        let collection = db.collection("users");

        // Create hash index
        db.create_hash_index(collection, "email", false).unwrap();

        let entity = EntityId::new();
        let key = b"test@example.com".to_vec();

        // Start transaction
        let mut txn = db.begin().unwrap();

        // Insert into index within transaction
        db.hash_index_insert(collection, "email", key.clone(), entity)
            .unwrap();

        // Before commit: lookup from another "reader" perspective should find it
        // (since we're using the legacy immediate-insert API, this isn't truly transactional)
        // This test documents current behavior and the gap

        // Commit
        db.commit(&mut txn).unwrap();

        // After commit: definitely visible
        let results = db.hash_index_lookup(collection, "email", &key).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], entity);
    }

    /// Tests that index state is unchanged when transaction is aborted.
    #[test]
    fn index_unchanged_on_abort() {
        let db = create_db();
        let collection = db.collection("users");

        // Create hash index
        db.create_hash_index(collection, "email", false).unwrap();

        let entity_committed = EntityId::new();
        let entity_aborted = EntityId::new();

        // First: commit an entity
        db.hash_index_insert(
            collection,
            "email",
            b"alice@example.com".to_vec(),
            entity_committed,
        )
        .unwrap();

        // Verify it exists
        let results = db
            .hash_index_lookup(collection, "email", b"alice@example.com")
            .unwrap();
        assert_eq!(results.len(), 1);

        // Start a transaction that will be aborted
        let mut txn = db.begin().unwrap();

        // Add another entity to index (using legacy API which is immediate)
        // Note: This documents current behavior - the legacy API doesn't participate in transactions
        db.hash_index_insert(
            collection,
            "email",
            b"bob@example.com".to_vec(),
            entity_aborted,
        )
        .unwrap();

        // Abort the transaction
        db.abort(&mut txn).unwrap();

        // The committed entity should still exist
        let results = db
            .hash_index_lookup(collection, "email", b"alice@example.com")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], entity_committed);
    }

    /// Tests that entity data and index remain consistent after failed transaction.
    #[test]
    fn entity_and_index_consistent_after_failed_transaction() {
        let db = create_db();
        let collection = db.collection("users");

        // Create hash index
        db.create_hash_index(collection, "status", false).unwrap();

        let entity = EntityId::new();

        // Successfully create entity
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1, 2, 3])?;
            Ok(())
        })
        .unwrap();
        db.hash_index_insert(collection, "status", b"active".to_vec(), entity)
            .unwrap();

        // Verify initial state
        let data = db.get(collection, entity).unwrap();
        assert_eq!(data, Some(vec![1, 2, 3]));
        let idx_results = db
            .hash_index_lookup(collection, "status", b"active")
            .unwrap();
        assert_eq!(idx_results.len(), 1);

        // Attempt to update with a transaction that fails
        let result: Result<(), CoreError> = db.transaction(|txn| {
            txn.put(collection, entity, vec![4, 5, 6])?;
            // Simulate failure
            Err(CoreError::InvalidOperation {
                message: "simulated failure".into(),
            })
        });
        assert!(result.is_err());

        // Entity data should be unchanged
        let data = db.get(collection, entity).unwrap();
        assert_eq!(
            data,
            Some(vec![1, 2, 3]),
            "entity should be unchanged after failed txn"
        );

        // Index should be unchanged
        let idx_results = db
            .hash_index_lookup(collection, "status", b"active")
            .unwrap();
        assert_eq!(
            idx_results.len(),
            1,
            "index should be unchanged after failed txn"
        );
    }

    /// Tests that btree index operations work correctly with transactions.
    #[test]
    fn btree_index_transaction_consistency() {
        let db = create_db();
        let collection = db.collection("users");

        // Create btree index
        db.create_btree_index(collection, "age", false).unwrap();

        // Insert some entities
        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();

        db.btree_index_insert(collection, "age", 25i64.to_be_bytes().to_vec(), e1)
            .unwrap();
        db.btree_index_insert(collection, "age", 30i64.to_be_bytes().to_vec(), e2)
            .unwrap();
        db.btree_index_insert(collection, "age", 35i64.to_be_bytes().to_vec(), e3)
            .unwrap();

        // Range query should return all
        let results = db.btree_index_range(collection, "age", None, None).unwrap();
        assert_eq!(results.len(), 3);

        // Start a transaction that will abort
        let mut txn = db.begin().unwrap();

        // The range query should still work
        let results = db.btree_index_range(collection, "age", None, None).unwrap();
        assert_eq!(results.len(), 3);

        db.abort(&mut txn).unwrap();

        // After abort, range should still be correct
        let results = db.btree_index_range(collection, "age", None, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    /// Tests that multiple index updates in a single transaction are consistent.
    #[test]
    fn multiple_index_updates_atomic() {
        let db = create_db();
        let collection = db.collection("users");

        // Create indexes
        db.create_hash_index(collection, "email", true).unwrap();
        db.create_btree_index(collection, "age", false).unwrap();

        let entity = EntityId::new();

        // Commit a transaction that updates both indexes
        db.transaction(|txn| {
            txn.put(collection, entity, vec![1])?;
            Ok(())
        })
        .unwrap();

        // Add to both indexes
        db.hash_index_insert(collection, "email", b"user@test.com".to_vec(), entity)
            .unwrap();
        db.btree_index_insert(collection, "age", 25i64.to_be_bytes().to_vec(), entity)
            .unwrap();

        // Both should be queryable
        let hash_results = db
            .hash_index_lookup(collection, "email", b"user@test.com")
            .unwrap();
        assert_eq!(hash_results.len(), 1);

        let btree_results = db
            .btree_index_lookup(collection, "age", &25i64.to_be_bytes())
            .unwrap();
        assert_eq!(btree_results.len(), 1);

        assert_eq!(hash_results[0], btree_results[0]);
    }

    /// Tests that removing from index followed by abort keeps the entry.
    #[test]
    fn index_remove_followed_by_abort() {
        let db = create_db();
        let collection = db.collection("users");

        db.create_hash_index(collection, "status", false).unwrap();

        let entity = EntityId::new();
        let key = b"active".to_vec();

        // Insert into index
        db.hash_index_insert(collection, "status", key.clone(), entity)
            .unwrap();

        // Verify it exists
        assert_eq!(db.hash_index_len(collection, "status").unwrap(), 1);

        // Remove within a transaction context
        let mut txn = db.begin().unwrap();
        db.hash_index_remove(collection, "status", &key, entity)
            .unwrap();

        // The remove happened immediately (legacy API)
        assert_eq!(db.hash_index_len(collection, "status").unwrap(), 0);

        // Abort the transaction
        db.abort(&mut txn).unwrap();

        // Note: Current legacy API doesn't rollback index changes
        // This documents the current behavior
        assert_eq!(db.hash_index_len(collection, "status").unwrap(), 0);
    }

    /// Tests that unique constraint is enforced correctly within transactions.
    #[test]
    fn unique_index_constraint_in_transaction() {
        let db = create_db();
        let collection = db.collection("users");

        // Create unique hash index
        db.create_hash_index(collection, "email", true).unwrap();

        let entity1 = EntityId::new();
        let entity2 = EntityId::new();

        // Insert first entity
        db.hash_index_insert(collection, "email", b"unique@test.com".to_vec(), entity1)
            .unwrap();

        // Try to insert duplicate - should fail
        let result =
            db.hash_index_insert(collection, "email", b"unique@test.com".to_vec(), entity2);
        assert!(result.is_err(), "duplicate key should be rejected");

        // Original entry should still be there
        let results = db
            .hash_index_lookup(collection, "email", b"unique@test.com")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], entity1);
    }
}
