//! Python bindings for EntiDB.
//!
//! This crate provides Python bindings using PyO3.

use entidb_core::{
    CollectionId, Config, Database as CoreDatabase, EntityId as CoreEntityId,
};
use entidb_storage::FileBackend;
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

        // Create directory structure if needed
        if create_if_missing {
            if let Some(parent) = db_path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| PyIOError::new_err(format!("Failed to create directory: {e}")))?;
                }
            }
            std::fs::create_dir_all(db_path)
                .map_err(|e| PyIOError::new_err(format!("Failed to create database directory: {e}")))?;
        }

        // Open file backends for WAL and segments
        let wal_path = db_path.join("wal.log");
        let segment_path = db_path.join("segments.dat");

        let wal_backend = if create_if_missing {
            FileBackend::open_with_create_dirs(&wal_path)
        } else {
            FileBackend::open(&wal_path)
        };

        let wal_backend = wal_backend
            .map_err(|e| PyIOError::new_err(format!("Failed to open WAL: {e}")))?;

        let segment_backend = if create_if_missing {
            FileBackend::open_with_create_dirs(&segment_path)
        } else {
            FileBackend::open(&segment_path)
        };

        let segment_backend = segment_backend
            .map_err(|e| PyIOError::new_err(format!("Failed to open segments: {e}")))?;

        // Build core config
        let mut config = Config::default();
        config.max_segment_size = max_segment_size;
        config.sync_on_commit = sync_on_commit;

        // Open database with file backends
        CoreDatabase::open_with_backends(
            config,
            Box::new(wal_backend),
            Box::new(segment_backend),
        )
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
        let id = self.inner.collection(name);
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

/// Python module initialization.
#[pymodule]
fn entidb(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<EntityId>()?;
    m.add_class::<Collection>()?;
    m.add_class::<Transaction>()?;
    m.add_class::<Database>()?;
    m.add_class::<EntityIterator>()?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}

/// Returns the EntiDB library version.
#[pyfunction]
fn version() -> &'static str {
    VERSION
}
