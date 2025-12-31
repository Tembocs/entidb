//! Python bindings for EntiDB.
//!
//! This crate provides Python bindings using PyO3.

#[cfg(feature = "encryption")]
use entidb_core::crypto::{CryptoManager as CoreCryptoManager, EncryptionKey};
use entidb_core::{CollectionId, Config, Database as CoreDatabase, EntityId as CoreEntityId};
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyStopIteration, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::path::Path;
use std::sync::Arc;

/// Library version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Entity ID - a 16-byte unique identifier.
#[pyclass]
#[derive(Clone)]
pub struct EntityId {
    inner: CoreEntityId,
}

#[pymethods]
impl EntityId {
    /// Creates a new unique entity ID.
    #[new]
    fn new() -> Self {
        Self {
            inner: CoreEntityId::new(),
        }
    }

    /// Creates an entity ID from bytes.
    #[staticmethod]
    fn from_bytes(bytes: &[u8]) -> PyResult<Self> {
        if bytes.len() != 16 {
            return Err(PyValueError::new_err("EntityId must be exactly 16 bytes"));
        }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        Ok(Self {
            inner: CoreEntityId::from_bytes(arr),
        })
    }

    /// Returns the bytes of this entity ID.
    fn to_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.as_bytes())
    }

    /// Returns a hex string representation.
    fn to_hex(&self) -> String {
        self.inner
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn __repr__(&self) -> String {
        format!("EntityId({})", self.to_hex())
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        let bytes = self.inner.as_bytes();
        let mut hash = 0u64;
        for &b in bytes {
            hash = hash.wrapping_mul(31).wrapping_add(u64::from(b));
        }
        hash
    }
}

/// A collection of entities.
#[pyclass]
#[derive(Clone)]
pub struct Collection {
    id: u32,
    name: String,
}

#[pymethods]
impl Collection {
    /// The collection name.
    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    /// The internal collection ID.
    #[getter]
    fn id(&self) -> u32 {
        self.id
    }

    fn __repr__(&self) -> String {
        format!("Collection({}, id={})", self.name, self.id)
    }
}

/// A database transaction.
#[pyclass]
pub struct Transaction {
    db: Arc<CoreDatabase>,
    writes: Vec<(u32, [u8; 16], Option<Vec<u8>>)>,
    committed: bool,
    aborted: bool,
}

#[pymethods]
impl Transaction {
    /// Puts an entity in a collection.
    fn put(&mut self, collection: &Collection, entity_id: &EntityId, data: &[u8]) -> PyResult<()> {
        if self.committed {
            return Err(PyRuntimeError::new_err("Transaction already committed"));
        }
        if self.aborted {
            return Err(PyRuntimeError::new_err("Transaction already aborted"));
        }
        self.writes.push((
            collection.id,
            *entity_id.inner.as_bytes(),
            Some(data.to_vec()),
        ));
        Ok(())
    }

    /// Deletes an entity from a collection.
    fn delete(&mut self, collection: &Collection, entity_id: &EntityId) -> PyResult<()> {
        if self.committed {
            return Err(PyRuntimeError::new_err("Transaction already committed"));
        }
        if self.aborted {
            return Err(PyRuntimeError::new_err("Transaction already aborted"));
        }
        self.writes
            .push((collection.id, *entity_id.inner.as_bytes(), None));
        Ok(())
    }

    /// Gets an entity (sees uncommitted writes in this transaction).
    fn get<'py>(
        &self,
        py: Python<'py>,
        collection: &Collection,
        entity_id: &EntityId,
    ) -> PyResult<Option<Bound<'py, PyBytes>>> {
        // Check uncommitted writes first
        let key = (collection.id, *entity_id.inner.as_bytes());
        for (coll_id, ent_id, payload) in self.writes.iter().rev() {
            if *coll_id == key.0 && *ent_id == key.1 {
                return match payload {
                    Some(data) => Ok(Some(PyBytes::new(py, data))),
                    None => Ok(None), // Deleted in this transaction
                };
            }
        }

        // Not in transaction, check database
        let coll = CollectionId::new(collection.id);
        let ent = CoreEntityId::from_bytes(*entity_id.inner.as_bytes());

        self.db
            .get(coll, ent)
            .map(|opt| opt.map(|data| PyBytes::new(py, &data)))
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Commits the transaction.
    fn commit(&mut self) -> PyResult<()> {
        if self.committed {
            return Err(PyRuntimeError::new_err("Transaction already committed"));
        }
        if self.aborted {
            return Err(PyRuntimeError::new_err("Transaction already aborted"));
        }

        let writes = std::mem::take(&mut self.writes);
        self.committed = true;

        self.db
            .transaction(|core_txn| {
                for (coll_id, ent_id, payload) in writes {
                    let coll = CollectionId::new(coll_id);
                    let ent = CoreEntityId::from_bytes(ent_id);

                    match payload {
                        Some(data) => core_txn.put(coll, ent, data)?,
                        None => core_txn.delete(coll, ent)?,
                    }
                }
                Ok(())
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Aborts the transaction, discarding all changes.
    fn abort(&mut self) {
        self.writes.clear();
        self.aborted = true;
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &mut self,
        exc_type: Option<PyObject>,
        _exc_val: Option<PyObject>,
        _exc_tb: Option<PyObject>,
    ) -> PyResult<bool> {
        if !self.committed && !self.aborted {
            if exc_type.is_some() {
                // Exception occurred, abort the transaction
                self.abort();
            } else {
                // No exception, commit the transaction
                self.commit()?;
            }
        }
        Ok(false)
    }
}

/// Iterator over entities in a collection.
///
/// Memory-efficient iteration that doesn't load all entities at once.
/// Use `Database.iter()` to create an iterator.
#[pyclass]
pub struct EntityIterator {
    entities: Vec<(CoreEntityId, Vec<u8>)>,
    index: usize,
}

#[pymethods]
impl EntityIterator {
    /// Returns self as iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Returns the next entity or raises StopIteration.
    fn __next__<'py>(
        mut slf: PyRefMut<'py, Self>,
        py: Python<'py>,
    ) -> PyResult<(EntityId, Bound<'py, PyBytes>)> {
        if slf.index >= slf.entities.len() {
            return Err(PyStopIteration::new_err(()));
        }

        let index = slf.index;
        slf.index += 1;

        let (id, data) = &slf.entities[index];
        Ok((EntityId { inner: *id }, PyBytes::new(py, data)))
    }

    /// Returns the number of remaining entities.
    fn remaining(&self) -> usize {
        self.entities.len().saturating_sub(self.index)
    }

    /// Returns the total number of entities.
    fn count(&self) -> usize {
        self.entities.len()
    }
}

/// EntiDB database handle.
#[pyclass]
pub struct Database {
    inner: Arc<CoreDatabase>,
}

#[pymethods]
impl Database {
    /// Opens a file-based database at the given path.
    ///
    /// Creates the database directory if it doesn't exist.
    /// Data is persisted to disk and survives process restarts.
    ///
    /// This uses the standard directory-based layout:
    /// - LOCK file for single-writer guarantee
    /// - MANIFEST for metadata persistence
    /// - WAL/ directory for write-ahead log
    /// - SEGMENTS/ directory for segment files with proper rotation
    ///
    /// Args:
    ///     path: Path to the database directory.
    ///     max_segment_size: Maximum segment file size (default: 64MB).
    ///     sync_on_commit: Whether to sync to disk on every commit (default: True).
    ///     create_if_missing: Create database if it doesn't exist (default: True).
    #[staticmethod]
    #[pyo3(signature = (path, max_segment_size=67108864, sync_on_commit=true, create_if_missing=true))]
    fn open(
        path: &str,
        max_segment_size: u64,
        sync_on_commit: bool,
        create_if_missing: bool,
    ) -> PyResult<Self> {
        let db_path = Path::new(path);

        // Build core config
        let config = Config::default()
            .create_if_missing(create_if_missing)
            .sync_on_commit(sync_on_commit)
            .max_segment_size(max_segment_size);

        // Open database using directory-based path (ensures LOCK, MANIFEST, SEGMENTS/ layout)
        CoreDatabase::open_with_config(db_path, config)
            .map(|db| Self {
                inner: Arc::new(db),
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Opens an in-memory database.
    #[staticmethod]
    fn open_memory() -> PyResult<Self> {
        CoreDatabase::open_in_memory()
            .map(|db| Self {
                inner: Arc::new(db),
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Gets or creates a collection by name.
    fn collection(&self, name: &str) -> Collection {
        let id = self.inner.collection_unchecked(name);
        Collection {
            id: id.as_u32(),
            name: name.to_string(),
        }
    }

    /// Puts an entity in a collection.
    fn put(&self, collection: &Collection, entity_id: &EntityId, data: &[u8]) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        let ent = entity_id.inner;

        self.inner
            .transaction(|txn| {
                txn.put(coll, ent, data.to_vec())?;
                Ok(())
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Gets an entity from a collection.
    fn get<'py>(
        &self,
        py: Python<'py>,
        collection: &Collection,
        entity_id: &EntityId,
    ) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let coll = CollectionId::new(collection.id);
        let ent = entity_id.inner;

        self.inner
            .get(coll, ent)
            .map(|opt| opt.map(|data| PyBytes::new(py, &data)))
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Deletes an entity from a collection.
    fn delete(&self, collection: &Collection, entity_id: &EntityId) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        let ent = entity_id.inner;

        self.inner
            .transaction(|txn| {
                txn.delete(coll, ent)?;
                Ok(())
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Lists all entities in a collection.
    fn list<'py>(
        &self,
        py: Python<'py>,
        collection: &Collection,
    ) -> PyResult<Vec<(EntityId, Bound<'py, PyBytes>)>> {
        let coll = CollectionId::new(collection.id);

        self.inner
            .list(coll)
            .map(|entities| {
                entities
                    .into_iter()
                    .map(|(id, data)| {
                        (
                            EntityId { inner: id },
                            PyBytes::new(py, &data),
                        )
                    })
                    .collect()
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Gets the count of entities in a collection.
    fn count(&self, collection: &Collection) -> PyResult<usize> {
        let coll = CollectionId::new(collection.id);

        self.inner
            .list(coll)
            .map(|entities| entities.len())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns an iterator over entities in a collection.
    ///
    /// This is more memory-efficient than `list()` for large collections
    /// as it supports lazy iteration.
    fn iter(&self, collection: &Collection) -> PyResult<EntityIterator> {
        let coll = CollectionId::new(collection.id);

        self.inner
            .list(coll)
            .map(|entities| EntityIterator { entities, index: 0 })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Creates a new transaction.
    ///
    /// Transactions support context manager protocol and automatically
    /// commit on success or abort on exception.
    ///
    /// Usage:
    /// ```python
    /// with db.transaction() as txn:
    ///     txn.put(collection, entity_id, data)
    ///     # auto-commits on exit
    /// ```
    ///
    /// Or manually:
    /// ```python
    /// txn = db.transaction()
    /// txn.put(collection, entity_id, data)
    /// txn.commit()  # or txn.abort()
    /// ```
    fn transaction(&self) -> Transaction {
        Transaction {
            db: Arc::clone(&self.inner),
            writes: Vec::new(),
            committed: false,
            aborted: false,
        }
    }

    /// Closes the database.
    fn close(&self) -> PyResult<()> {
        self.inner
            .close()
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Whether the database is open.
    #[getter]
    fn is_open(&self) -> bool {
        self.inner.is_open()
    }

    /// Creates a checkpoint.
    ///
    /// A checkpoint persists all committed data and truncates the WAL.
    /// After a checkpoint:
    /// - All committed transactions are durable in segments
    /// - The WAL is cleared
    /// - The manifest is updated with the checkpoint sequence
    fn checkpoint(&self) -> PyResult<()> {
        self.inner
            .checkpoint()
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns a snapshot of database statistics.
    ///
    /// Statistics include counts of reads, writes, transactions, and other
    /// operations. This is useful for monitoring and diagnostics.
    ///
    /// Example:
    /// ```python
    /// stats = db.stats()
    /// print(f"Reads: {stats.reads}, Writes: {stats.writes}")
    /// print(f"Transactions committed: {stats.transactions_committed}")
    /// ```
    fn stats(&self) -> DatabaseStats {
        let s = self.inner.stats();
        DatabaseStats {
            reads: s.reads,
            writes: s.writes,
            deletes: s.deletes,
            scans: s.scans,
            index_lookups: s.index_lookups,
            transactions_started: s.transactions_started,
            transactions_committed: s.transactions_committed,
            transactions_aborted: s.transactions_aborted,
            bytes_read: s.bytes_read,
            bytes_written: s.bytes_written,
            checkpoints: s.checkpoints,
            errors: s.errors,
            entity_count: s.entity_count,
        }
    }

    /// Compacts the database, removing obsolete versions and optionally tombstones.
    ///
    /// Compaction merges segment records to:
    /// - Remove obsolete entity versions (keeping only the latest)
    /// - Optionally remove tombstones (deleted entity markers)
    /// - Reclaim disk space
    ///
    /// Args:
    ///     remove_tombstones: If True, removes tombstone records. Default is False.
    ///
    /// Returns:
    ///     CompactionStats with details about the compaction operation.
    ///
    /// Example:
    /// ```python
    /// stats = db.compact(remove_tombstones=True)
    /// print(f"Removed {stats.tombstones_removed} tombstones")
    /// print(f"Saved {stats.bytes_saved} bytes")
    /// ```
    #[pyo3(signature = (remove_tombstones=false))]
    fn compact(&self, remove_tombstones: bool) -> PyResult<CompactionStats> {
        self.inner
            .compact(remove_tombstones)
            .map(|s| CompactionStats {
                input_records: s.input_records,
                output_records: s.output_records,
                tombstones_removed: s.tombstones_removed,
                obsolete_versions_removed: s.obsolete_versions_removed,
                bytes_saved: s.bytes_saved,
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Creates a backup of the database.
    ///
    /// Returns the backup data as bytes that can be saved to a file.
    ///
    /// Example:
    /// ```python
    /// backup_data = db.backup()
    /// with open('backup.endb', 'wb') as f:
    ///     f.write(backup_data)
    /// ```
    fn backup<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        self.inner
            .backup()
            .map(|data| PyBytes::new(py, &data))
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Creates a backup with custom options.
    ///
    /// Args:
    ///     include_tombstones: Whether to include deleted entities in the backup.
    #[pyo3(signature = (include_tombstones=false))]
    fn backup_with_options<'py>(
        &self,
        py: Python<'py>,
        include_tombstones: bool,
    ) -> PyResult<Bound<'py, PyBytes>> {
        self.inner
            .backup_with_options(include_tombstones)
            .map(|data| PyBytes::new(py, &data))
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Restores entities from a backup into this database.
    ///
    /// This merges the backup data into the current database.
    /// Existing entities with the same ID will be overwritten.
    ///
    /// Args:
    ///     backup_data: The backup data bytes.
    ///
    /// Returns:
    ///     RestoreStats with information about the restore operation.
    ///
    /// Example:
    /// ```python
    /// with open('backup.endb', 'rb') as f:
    ///     backup_data = f.read()
    /// stats = db.restore(backup_data)
    /// print(f"Restored {stats.entities_restored} entities")
    /// ```
    fn restore(&self, backup_data: &[u8]) -> PyResult<RestoreStats> {
        self.inner
            .restore(backup_data)
            .map(|stats| RestoreStats {
                entities_restored: stats.entities_restored,
                tombstones_applied: stats.tombstones_applied,
                backup_timestamp: stats.backup_timestamp,
                backup_sequence: stats.backup_sequence,
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Validates a backup without restoring it.
    ///
    /// Returns the backup metadata if valid.
    ///
    /// Args:
    ///     backup_data: The backup data bytes.
    ///
    /// Returns:
    ///     BackupInfo with metadata about the backup.
    fn validate_backup(&self, backup_data: &[u8]) -> PyResult<BackupInfo> {
        self.inner
            .validate_backup(backup_data)
            .map(|info| BackupInfo {
                valid: info.valid,
                timestamp: info.timestamp,
                sequence: info.sequence,
                record_count: info.record_count,
                size: info.size,
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns the current committed sequence number.
    #[getter]
    fn committed_seq(&self) -> u64 {
        self.inner.committed_seq().as_u64()
    }

    /// Returns the total entity count.
    #[getter]
    fn entity_count(&self) -> usize {
        self.inner.entity_count()
    }

    // ========================================================================
    // Index Management
    // ========================================================================

    /// Creates a hash index for O(1) equality lookups.
    ///
    /// Args:
    ///     collection: The collection to create the index on.
    ///     name: The index name.
    ///     unique: Whether the index enforces unique keys.
    ///
    /// Example:
    /// ```python
    /// db.create_hash_index(users, "email", unique=True)
    /// ```
    #[pyo3(signature = (collection, name, unique=false))]
    fn create_hash_index(
        &self,
        collection: &Collection,
        name: &str,
        unique: bool,
    ) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .create_hash_index(coll, name, unique)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Creates a BTree index for ordered and range lookups.
    ///
    /// Args:
    ///     collection: The collection to create the index on.
    ///     name: The index name.
    ///     unique: Whether the index enforces unique keys.
    ///
    /// Example:
    /// ```python
    /// db.create_btree_index(users, "age", unique=False)
    /// ```
    #[pyo3(signature = (collection, name, unique=false))]
    fn create_btree_index(
        &self,
        collection: &Collection,
        name: &str,
        unique: bool,
    ) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .create_btree_index(coll, name, unique)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Inserts a key-entity pair into a hash index.
    ///
    /// Args:
    ///     collection: The collection the index belongs to.
    ///     index_name: The name of the index.
    ///     key: The key bytes.
    ///     entity_id: The entity to associate with the key.
    fn hash_index_insert(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .hash_index_insert(coll, index_name, key.to_vec(), entity_id.inner)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Inserts a key-entity pair into a BTree index.
    ///
    /// Args:
    ///     collection: The collection the index belongs to.
    ///     index_name: The name of the index.
    ///     key: The key bytes (should use big-endian encoding for proper ordering).
    ///     entity_id: The entity to associate with the key.
    fn btree_index_insert(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .btree_index_insert(coll, index_name, key.to_vec(), entity_id.inner)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Removes a key-entity pair from a hash index.
    fn hash_index_remove(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> PyResult<bool> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .hash_index_remove(coll, index_name, key, entity_id.inner)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Removes a key-entity pair from a BTree index.
    fn btree_index_remove(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> PyResult<bool> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .btree_index_remove(coll, index_name, key, entity_id.inner)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Looks up entities by key in a hash index.
    ///
    /// Returns a list of EntityIds matching the key.
    fn hash_index_lookup(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
    ) -> PyResult<Vec<EntityId>> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .hash_index_lookup(coll, index_name, key)
            .map(|ids| ids.into_iter().map(|id| EntityId { inner: id }).collect())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Looks up entities by key in a BTree index.
    ///
    /// Returns a list of EntityIds matching the key.
    fn btree_index_lookup(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
    ) -> PyResult<Vec<EntityId>> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .btree_index_lookup(coll, index_name, key)
            .map(|ids| ids.into_iter().map(|id| EntityId { inner: id }).collect())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Performs a range query on a BTree index.
    ///
    /// Args:
    ///     collection: The collection the index belongs to.
    ///     index_name: The name of the index.
    ///     min_key: Optional minimum key (inclusive). None for unbounded.
    ///     max_key: Optional maximum key (inclusive). None for unbounded.
    ///
    /// Returns a list of EntityIds in the range.
    #[pyo3(signature = (collection, index_name, min_key=None, max_key=None))]
    fn btree_index_range(
        &self,
        collection: &Collection,
        index_name: &str,
        min_key: Option<&[u8]>,
        max_key: Option<&[u8]>,
    ) -> PyResult<Vec<EntityId>> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .btree_index_range(coll, index_name, min_key, max_key)
            .map(|ids| ids.into_iter().map(|id| EntityId { inner: id }).collect())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns the number of entries in a hash index.
    fn hash_index_len(&self, collection: &Collection, index_name: &str) -> PyResult<usize> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .hash_index_len(coll, index_name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns the number of entries in a BTree index.
    fn btree_index_len(&self, collection: &Collection, index_name: &str) -> PyResult<usize> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .btree_index_len(coll, index_name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Drops a hash index.
    ///
    /// Returns True if the index existed and was dropped.
    fn drop_hash_index(&self, collection: &Collection, index_name: &str) -> PyResult<bool> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .drop_hash_index(coll, index_name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Drops a BTree index.
    ///
    /// Returns True if the index existed and was dropped.
    fn drop_btree_index(&self, collection: &Collection, index_name: &str) -> PyResult<bool> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .drop_btree_index(coll, index_name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    // ========================================================================
    // FTS (Full-Text Search) Index
    // ========================================================================

    /// Creates a full-text search index for text searching.
    ///
    /// The index uses default settings: case-insensitive, min token length 1,
    /// max token length 256.
    ///
    /// Args:
    ///     collection: The collection to create the index on.
    ///     name: The index name.
    ///
    /// Example:
    /// ```python
    /// db.create_fts_index(documents, "content")
    /// ```
    fn create_fts_index(&self, collection: &Collection, name: &str) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .create_fts_index(coll, name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Creates a full-text search index with custom configuration.
    ///
    /// Args:
    ///     collection: The collection to create the index on.
    ///     name: The index name.
    ///     min_token_length: Minimum token length (default 1).
    ///     max_token_length: Maximum token length (default 256).
    ///     case_sensitive: If True, matching is case-sensitive (default False).
    ///
    /// Example:
    /// ```python
    /// db.create_fts_index_with_config(documents, "content",
    ///     min_token_length=2, case_sensitive=True)
    /// ```
    #[pyo3(signature = (collection, name, min_token_length=1, max_token_length=256, case_sensitive=false))]
    fn create_fts_index_with_config(
        &self,
        collection: &Collection,
        name: &str,
        min_token_length: usize,
        max_token_length: usize,
        case_sensitive: bool,
    ) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .create_fts_index_with_config(coll, name, min_token_length, max_token_length, case_sensitive)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Indexes text for an entity in an FTS index.
    ///
    /// If the entity was previously indexed, its old tokens are replaced.
    ///
    /// Args:
    ///     collection: The collection.
    ///     name: The FTS index name.
    ///     entity_id: The entity to index.
    ///     text: The text content to index.
    ///
    /// Example:
    /// ```python
    /// db.fts_index_text(documents, "content", doc_id, "Hello world")
    /// ```
    fn fts_index_text(
        &self,
        collection: &Collection,
        name: &str,
        entity_id: &EntityId,
        text: &str,
    ) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_index_text(coll, name, entity_id.inner, text)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Removes an entity from an FTS index.
    ///
    /// Returns True if the entity was found and removed.
    ///
    /// Args:
    ///     collection: The collection.
    ///     name: The FTS index name.
    ///     entity_id: The entity to remove.
    fn fts_remove_entity(
        &self,
        collection: &Collection,
        name: &str,
        entity_id: &EntityId,
    ) -> PyResult<bool> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_remove_entity(coll, name, entity_id.inner)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Searches the FTS index with AND semantics.
    ///
    /// All query tokens must be present in an entity for it to match.
    ///
    /// Args:
    ///     collection: The collection.
    ///     name: The FTS index name.
    ///     query: The search query (whitespace-separated terms).
    ///
    /// Returns:
    ///     A list of EntityId objects that match all query terms.
    ///
    /// Example:
    /// ```python
    /// results = db.fts_search(documents, "content", "hello world")
    /// # Returns entities containing both "hello" AND "world"
    /// ```
    fn fts_search(&self, collection: &Collection, name: &str, query: &str) -> PyResult<Vec<EntityId>> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_search(coll, name, query)
            .map(|ids| ids.into_iter().map(|inner| EntityId { inner }).collect())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Searches the FTS index with OR semantics.
    ///
    /// An entity matches if it contains ANY of the query tokens.
    ///
    /// Args:
    ///     collection: The collection.
    ///     name: The FTS index name.
    ///     query: The search query (whitespace-separated terms).
    ///
    /// Returns:
    ///     A list of EntityId objects that match at least one query term.
    ///
    /// Example:
    /// ```python
    /// results = db.fts_search_any(documents, "content", "hello world")
    /// # Returns entities containing "hello" OR "world" (or both)
    /// ```
    fn fts_search_any(&self, collection: &Collection, name: &str, query: &str) -> PyResult<Vec<EntityId>> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_search_any(coll, name, query)
            .map(|ids| ids.into_iter().map(|inner| EntityId { inner }).collect())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Searches the FTS index with prefix matching.
    ///
    /// Finds entities where any indexed token starts with the given prefix.
    ///
    /// Args:
    ///     collection: The collection.
    ///     name: The FTS index name.
    ///     prefix: The prefix to search for.
    ///
    /// Returns:
    ///     A list of EntityId objects with tokens starting with the prefix.
    ///
    /// Example:
    /// ```python
    /// results = db.fts_search_prefix(documents, "content", "prog")
    /// # Returns entities with words like "program", "programming", etc.
    /// ```
    fn fts_search_prefix(&self, collection: &Collection, name: &str, prefix: &str) -> PyResult<Vec<EntityId>> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_search_prefix(coll, name, prefix)
            .map(|ids| ids.into_iter().map(|inner| EntityId { inner }).collect())
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns the number of entities indexed in the FTS index.
    fn fts_index_len(&self, collection: &Collection, name: &str) -> PyResult<usize> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_index_len(coll, name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Returns the number of unique tokens in the FTS index.
    fn fts_unique_token_count(&self, collection: &Collection, name: &str) -> PyResult<usize> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_unique_token_count(coll, name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Clears all entries from an FTS index.
    fn fts_clear(&self, collection: &Collection, name: &str) -> PyResult<()> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .fts_clear(coll, name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Drops an FTS index.
    ///
    /// Returns True if the index existed and was dropped.
    fn drop_fts_index(&self, collection: &Collection, name: &str) -> PyResult<bool> {
        let coll = CollectionId::new(collection.id);
        self.inner
            .drop_fts_index(coll, name)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    // ========================================================================
    // Change Feed
    // ========================================================================

    /// Polls for changes since the given cursor.
    ///
    /// Returns a list of ChangeEvent objects that occurred after the cursor position.
    /// Use `latest_sequence` to get the current cursor position.
    ///
    /// Args:
    ///     cursor: The sequence number to start from (exclusive).
    ///     limit: Maximum number of events to return (default 100).
    ///
    /// Returns:
    ///     A list of ChangeEvent objects.
    ///
    /// Example:
    /// ```python
    /// cursor = 0
    /// events = db.poll_changes(cursor, limit=50)
    /// for event in events:
    ///     print(f"Change: {event.change_type} on collection {event.collection_id}")
    ///     cursor = event.sequence
    /// ```
    #[pyo3(signature = (cursor, limit=100))]
    fn poll_changes(&self, cursor: u64, limit: usize) -> Vec<ChangeEvent> {
        let change_feed = self.inner.change_feed();
        change_feed
            .poll(cursor, limit)
            .into_iter()
            .map(|e| ChangeEvent {
                sequence: e.sequence,
                collection_id: e.collection_id,
                entity_id: EntityId {
                    inner: CoreEntityId::from_bytes(e.entity_id),
                },
                change_type: match e.change_type {
                    entidb_core::ChangeType::Insert => ChangeType::Insert,
                    entidb_core::ChangeType::Update => ChangeType::Update,
                    entidb_core::ChangeType::Delete => ChangeType::Delete,
                },
                payload: e.payload.unwrap_or_default(),
            })
            .collect()
    }

    /// Returns the latest sequence number in the change feed.
    ///
    /// This can be used as a starting cursor for `poll_changes`.
    #[getter]
    fn latest_sequence(&self) -> u64 {
        self.inner.change_feed().latest_sequence()
    }

    // ========================================================================
    // Schema Version
    // ========================================================================

    /// Gets the current schema version of the database.
    ///
    /// The schema version is user-managed and can be used for migrations.
    /// Returns 0 if no schema version has been set.
    ///
    /// Example:
    /// ```python
    /// version = db.schema_version
    /// if version < 2:
    ///     # Run migration
    ///     db.schema_version = 2
    /// ```
    #[getter]
    fn schema_version(&self) -> PyResult<u64> {
        // Read from the metadata collection (collection id 0 is reserved for metadata)
        let meta_coll_id = CollectionId::new(0);
        let key_id = CoreEntityId::from_bytes([
            0x73, 0x63, 0x68, 0x65, 0x6d, 0x61, 0x5f, 0x76,  // "schema_v"
            0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x00, 0x00,  // "ersion\0\0"
        ]);

        match self.inner.get(meta_coll_id, key_id) {
            Ok(Some(bytes)) => {
                if bytes.len() >= 8 {
                    let arr: [u8; 8] = bytes[0..8]
                        .try_into()
                        .map_err(|_| PyValueError::new_err("invalid schema_version encoding"))?;
                    Ok(u64::from_le_bytes(arr))
                } else {
                    Ok(0)
                }
            }
            Ok(None) => Ok(0),
            Err(e) => Err(PyIOError::new_err(e.to_string())),
        }
    }

    /// Sets the schema version of the database.
    ///
    /// The schema version is user-managed and can be used for migrations.
    #[setter]
    fn set_schema_version(&self, version: u64) -> PyResult<()> {
        // Write to the metadata collection
        let meta_coll_id = CollectionId::new(0);
        let key_id = CoreEntityId::from_bytes([
            0x73, 0x63, 0x68, 0x65, 0x6d, 0x61, 0x5f, 0x76,  // "schema_v"
            0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x00, 0x00,  // "ersion\0\0"
        ]);

        self.inner
            .transaction(|txn| {
                txn.put(meta_coll_id, key_id, version.to_le_bytes().to_vec())
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &self,
        _exc_type: Option<PyObject>,
        _exc_val: Option<PyObject>,
        _exc_tb: Option<PyObject>,
    ) -> PyResult<bool> {
        self.close()?;
        Ok(false)
    }
}

/// Statistics from a restore operation.
#[pyclass]
#[derive(Clone)]
pub struct RestoreStats {
    /// Number of entities restored.
    #[pyo3(get)]
    pub entities_restored: u64,
    /// Number of tombstones (deletions) applied.
    #[pyo3(get)]
    pub tombstones_applied: u64,
    /// Timestamp when the backup was created (Unix millis).
    #[pyo3(get)]
    pub backup_timestamp: u64,
    /// Sequence number at the time of backup.
    #[pyo3(get)]
    pub backup_sequence: u64,
}

#[pymethods]
impl RestoreStats {
    fn __repr__(&self) -> String {
        format!(
            "RestoreStats(entities_restored={}, tombstones_applied={}, backup_timestamp={}, backup_sequence={})",
            self.entities_restored,
            self.tombstones_applied,
            self.backup_timestamp,
            self.backup_sequence
        )
    }
}

/// Information about a backup.
#[pyclass]
#[derive(Clone)]
pub struct BackupInfo {
    /// Whether the backup checksum is valid.
    #[pyo3(get)]
    pub valid: bool,
    /// Timestamp when the backup was created (Unix millis).
    #[pyo3(get)]
    pub timestamp: u64,
    /// Sequence number at the time of backup.
    #[pyo3(get)]
    pub sequence: u64,
    /// Number of records in the backup.
    #[pyo3(get)]
    pub record_count: u32,
    /// Size of the backup in bytes.
    #[pyo3(get)]
    pub size: usize,
}

#[pymethods]
impl BackupInfo {
    fn __repr__(&self) -> String {
        format!(
            "BackupInfo(valid={}, timestamp={}, sequence={}, record_count={}, size={})",
            self.valid,
            self.timestamp,
            self.sequence,
            self.record_count,
            self.size
        )
    }
}

/// Database statistics snapshot.
///
/// Contains counters for various database operations, useful for
/// monitoring and diagnostics.
#[pyclass]
#[derive(Clone)]
pub struct DatabaseStats {
    /// Number of entity read operations.
    #[pyo3(get)]
    pub reads: u64,
    /// Number of entity write operations (put).
    #[pyo3(get)]
    pub writes: u64,
    /// Number of entity delete operations.
    #[pyo3(get)]
    pub deletes: u64,
    /// Number of full collection scans.
    #[pyo3(get)]
    pub scans: u64,
    /// Number of index lookup operations.
    #[pyo3(get)]
    pub index_lookups: u64,
    /// Number of transactions started.
    #[pyo3(get)]
    pub transactions_started: u64,
    /// Number of transactions committed.
    #[pyo3(get)]
    pub transactions_committed: u64,
    /// Number of transactions aborted.
    #[pyo3(get)]
    pub transactions_aborted: u64,
    /// Total bytes read from entities.
    #[pyo3(get)]
    pub bytes_read: u64,
    /// Total bytes written to entities.
    #[pyo3(get)]
    pub bytes_written: u64,
    /// Number of checkpoints performed.
    #[pyo3(get)]
    pub checkpoints: u64,
    /// Number of errors recorded.
    #[pyo3(get)]
    pub errors: u64,
    /// Total entity count (as of last update).
    #[pyo3(get)]
    pub entity_count: u64,
}

#[pymethods]
impl DatabaseStats {
    fn __repr__(&self) -> String {
        format!(
            "DatabaseStats(reads={}, writes={}, deletes={}, scans={}, index_lookups={}, \
             transactions_started={}, transactions_committed={}, transactions_aborted={}, \
             bytes_read={}, bytes_written={}, checkpoints={}, errors={}, entity_count={})",
            self.reads,
            self.writes,
            self.deletes,
            self.scans,
            self.index_lookups,
            self.transactions_started,
            self.transactions_committed,
            self.transactions_aborted,
            self.bytes_read,
            self.bytes_written,
            self.checkpoints,
            self.errors,
            self.entity_count
        )
    }
}

/// Statistics from a compaction operation.
///
/// Contains information about what was removed during compaction
/// and how much space was saved.
#[pyclass]
#[derive(Clone)]
pub struct CompactionStats {
    /// Number of records in the input.
    #[pyo3(get)]
    pub input_records: usize,
    /// Number of records in the output.
    #[pyo3(get)]
    pub output_records: usize,
    /// Number of tombstones removed.
    #[pyo3(get)]
    pub tombstones_removed: usize,
    /// Number of obsolete versions removed.
    #[pyo3(get)]
    pub obsolete_versions_removed: usize,
    /// Bytes saved (estimated).
    #[pyo3(get)]
    pub bytes_saved: usize,
}

#[pymethods]
impl CompactionStats {
    fn __repr__(&self) -> String {
        format!(
            "CompactionStats(input_records={}, output_records={}, tombstones_removed={}, \
             obsolete_versions_removed={}, bytes_saved={})",
            self.input_records,
            self.output_records,
            self.tombstones_removed,
            self.obsolete_versions_removed,
            self.bytes_saved
        )
    }
}

/// Type of change event.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeType {
    /// Entity was inserted (no previous version existed).
    Insert,
    /// Entity was updated (previous version existed).
    Update,
    /// Entity was deleted.
    Delete,
}

#[pymethods]
impl ChangeType {
    fn __repr__(&self) -> String {
        match self {
            ChangeType::Insert => "ChangeType.Insert".to_string(),
            ChangeType::Update => "ChangeType.Update".to_string(),
            ChangeType::Delete => "ChangeType.Delete".to_string(),
        }
    }
}

/// A change event from the change feed.
///
/// Change events are emitted when entities are created, updated, or deleted.
#[pyclass]
#[derive(Clone)]
pub struct ChangeEvent {
    /// The sequence number of this change.
    #[pyo3(get)]
    pub sequence: u64,
    /// The collection ID where the change occurred.
    #[pyo3(get)]
    pub collection_id: u32,
    /// The entity ID that was changed.
    #[pyo3(get)]
    pub entity_id: EntityId,
    /// The type of change (Insert, Update, or Delete).
    #[pyo3(get)]
    pub change_type: ChangeType,
    /// The payload bytes (CBOR-encoded entity data). Empty for delete events.
    pub payload: Vec<u8>,
}

#[pymethods]
impl ChangeEvent {
    /// Returns the payload as bytes.
    fn get_payload<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.payload)
    }

    fn __repr__(&self) -> String {
        format!(
            "ChangeEvent(sequence={}, collection_id={}, entity_id={}, change_type={:?}, payload_len={})",
            self.sequence,
            self.collection_id,
            self.entity_id.to_hex(),
            self.change_type,
            self.payload.len()
        )
    }
}

// ============================================================================
// Crypto Manager
// ============================================================================

/// Encryption manager for AES-256-GCM encryption.
///
/// Provides encryption and decryption capabilities using AES-256-GCM.
/// Keys are 32 bytes (256 bits) and are zeroized when the manager is closed.
///
/// Example:
///     ```python
///     from entidb import CryptoManager
///
///     # Create with a generated key
///     crypto = CryptoManager.create()
///     key = crypto.get_key()  # Save this key securely!
///
///     # Encrypt data
///     encrypted = crypto.encrypt(b"secret message")
///
///     # Decrypt data
///     decrypted = crypto.decrypt(encrypted)
///
///     # Close when done
///     crypto.close()
///     ```
#[cfg(feature = "encryption")]
#[pyclass]
pub struct CryptoManager {
    inner: Option<CoreCryptoManager>,
    key: [u8; 32],
}

#[cfg(feature = "encryption")]
#[pymethods]
impl CryptoManager {
    /// Returns True if encryption is available.
    #[staticmethod]
    fn is_available() -> bool {
        true
    }

    /// Creates a new CryptoManager with a generated random key.
    ///
    /// The generated key can be accessed via get_key() and should be
    /// stored securely for future use.
    #[staticmethod]
    fn create() -> PyResult<Self> {
        let key = EncryptionKey::generate();
        let key_bytes = *key.as_bytes();
        let manager = CoreCryptoManager::new(key);
        Ok(Self {
            inner: Some(manager),
            key: key_bytes,
        })
    }

    /// Creates a CryptoManager from an existing key.
    ///
    /// The key must be exactly 32 bytes (256 bits).
    #[staticmethod]
    fn from_key(key: &[u8]) -> PyResult<Self> {
        if key.len() != 32 {
            return Err(PyValueError::new_err(format!(
                "Key must be exactly 32 bytes, got {}",
                key.len()
            )));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(key);
        let enc_key = EncryptionKey::from_bytes(&key_bytes)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let manager = CoreCryptoManager::new(enc_key);
        Ok(Self {
            inner: Some(manager),
            key: key_bytes,
        })
    }

    /// Creates a CryptoManager from a password and salt.
    ///
    /// The password and salt are used to derive a key.
    /// The same password and salt will always produce the same key.
    #[staticmethod]
    fn from_password(password: &[u8], salt: &[u8]) -> PyResult<Self> {
        let key = EncryptionKey::derive_from_password(password, salt)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let key_bytes = *key.as_bytes();
        let manager = CoreCryptoManager::new(key);
        Ok(Self {
            inner: Some(manager),
            key: key_bytes,
        })
    }

    /// Returns the encryption key (32 bytes).
    ///
    /// This key should be stored securely if you need to decrypt data later.
    fn get_key<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.key)
    }

    /// Encrypts data using AES-256-GCM.
    ///
    /// Returns the encrypted data with nonce prepended:
    /// nonce (12 bytes) || ciphertext || tag (16 bytes)
    fn encrypt<'py>(&self, py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        let manager = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("CryptoManager has been closed"))?;

        manager
            .encrypt(data)
            .map(|encrypted| PyBytes::new(py, &encrypted))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Decrypts data that was encrypted with encrypt().
    ///
    /// Raises an exception if decryption fails (wrong key, corrupted data, etc.).
    fn decrypt<'py>(&self, py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        let manager = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("CryptoManager has been closed"))?;

        manager
            .decrypt(data)
            .map(|decrypted| PyBytes::new(py, &decrypted))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Encrypts data with associated authenticated data (AAD).
    ///
    /// The AAD is authenticated but not encrypted. This is useful for binding
    /// the ciphertext to metadata (e.g., entity ID, collection name).
    fn encrypt_with_aad<'py>(
        &self,
        py: Python<'py>,
        data: &[u8],
        aad: &[u8],
    ) -> PyResult<Bound<'py, PyBytes>> {
        let manager = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("CryptoManager has been closed"))?;

        manager
            .encrypt_with_aad(data, aad)
            .map(|encrypted| PyBytes::new(py, &encrypted))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Decrypts data with associated authenticated data (AAD).
    ///
    /// The same AAD that was used during encryption must be provided.
    fn decrypt_with_aad<'py>(
        &self,
        py: Python<'py>,
        data: &[u8],
        aad: &[u8],
    ) -> PyResult<Bound<'py, PyBytes>> {
        let manager = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("CryptoManager has been closed"))?;

        manager
            .decrypt_with_aad(data, aad)
            .map(|decrypted| PyBytes::new(py, &decrypted))
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Closes the crypto manager and releases resources.
    ///
    /// The manager should not be used after calling this method.
    fn close(&mut self) {
        self.inner = None;
    }

    /// Returns True if the manager has been closed.
    #[getter]
    fn is_closed(&self) -> bool {
        self.inner.is_none()
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &mut self,
        _exc_type: Option<PyObject>,
        _exc_val: Option<PyObject>,
        _exc_tb: Option<PyObject>,
    ) -> bool {
        self.close();
        false
    }

    fn __repr__(&self) -> String {
        if self.inner.is_some() {
            "CryptoManager(active)".to_string()
        } else {
            "CryptoManager(closed)".to_string()
        }
    }
}

/// Python module initialization.
#[pymodule]
fn entidb(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<EntityId>()?;
    m.add_class::<Collection>()?;
    m.add_class::<Transaction>()?;
    m.add_class::<Database>()?;
    m.add_class::<EntityIterator>()?;
    m.add_class::<RestoreStats>()?;
    m.add_class::<BackupInfo>()?;
    m.add_class::<DatabaseStats>()?;
    m.add_class::<CompactionStats>()?;
    m.add_class::<ChangeType>()?;
    m.add_class::<ChangeEvent>()?;
    #[cfg(feature = "encryption")]
    m.add_class::<CryptoManager>()?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    #[cfg(feature = "encryption")]
    m.add_function(wrap_pyfunction!(crypto_available, m)?)?;
    Ok(())
}

/// Returns the EntiDB library version.
#[pyfunction]
fn version() -> &'static str {
    VERSION
}

/// Returns True if encryption support is available.
#[cfg(feature = "encryption")]
#[pyfunction]
fn crypto_available() -> bool {
    true
}
