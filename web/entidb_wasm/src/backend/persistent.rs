//! Persistent storage backend wrapper.
//!
//! This module provides a wrapper that bridges async web storage (OPFS/IndexedDB)
//! with the sync `StorageBackend` trait used by EntiDB core.
//!
//! ## Strategy
//!
//! Since web storage APIs are async but `StorageBackend` is sync, we use:
//! 1. Async open: Load all data from OPFS/IndexedDB into memory
//! 2. Sync operations: Work with in-memory data (via WasmMemoryBackend)
//! 3. Async flush: Persist memory data back to OPFS/IndexedDB
//!
//! This is efficient for typical EntiDB use cases where databases are
//! relatively small (megabytes, not gigabytes).

use crate::backend::{IndexedDbBackend, OpfsBackend, WasmMemoryBackend};
use crate::error::{WasmError, WasmResult};
use entidb_storage::{StorageBackend, StorageResult};
use std::sync::RwLock;

/// Storage type selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageType {
    /// Origin Private File System (preferred for performance).
    Opfs,
    /// IndexedDB (fallback for older browsers).
    IndexedDb,
    /// In-memory only (no persistence).
    Memory,
}

impl StorageType {
    /// Detect the best available storage type.
    pub fn detect() -> Self {
        if OpfsBackend::is_available() {
            StorageType::Opfs
        } else if IndexedDbBackend::is_available() {
            StorageType::IndexedDb
        } else {
            StorageType::Memory
        }
    }
}

/// Persistent storage backend for web.
///
/// This backend wraps an in-memory backend and provides async methods
/// to load from and save to persistent web storage.
pub struct PersistentBackend {
    /// The underlying in-memory backend.
    memory: WasmMemoryBackend,
    /// Database name for persistence.
    db_name: String,
    /// File name within the database.
    file_name: String,
    /// Storage type being used.
    storage_type: StorageType,
    /// Whether there are unsaved changes.
    dirty: RwLock<bool>,
}

impl PersistentBackend {
    /// Creates a new persistent backend with empty data.
    pub fn new_empty(db_name: &str, file_name: &str, storage_type: StorageType) -> Self {
        Self {
            memory: WasmMemoryBackend::new(),
            db_name: db_name.to_string(),
            file_name: file_name.to_string(),
            storage_type,
            dirty: RwLock::new(false),
        }
    }

    /// Opens a persistent backend, loading existing data.
    pub async fn open(db_name: &str, file_name: &str) -> WasmResult<Self> {
        let storage_type = StorageType::detect();
        Self::open_with_type(db_name, file_name, storage_type).await
    }

    /// Opens a persistent backend with a specific storage type.
    pub async fn open_with_type(
        db_name: &str,
        file_name: &str,
        storage_type: StorageType,
    ) -> WasmResult<Self> {
        let data = match storage_type {
            StorageType::Opfs => Self::load_from_opfs(db_name, file_name).await?,
            StorageType::IndexedDb => Self::load_from_indexeddb(db_name, file_name).await?,
            StorageType::Memory => Vec::new(),
        };

        Ok(Self {
            memory: WasmMemoryBackend::with_data(data),
            db_name: db_name.to_string(),
            file_name: file_name.to_string(),
            storage_type,
            dirty: RwLock::new(false),
        })
    }

    /// Loads data from OPFS.
    async fn load_from_opfs(db_name: &str, file_name: &str) -> WasmResult<Vec<u8>> {
        match OpfsBackend::open(db_name, file_name).await {
            Ok(backend) => {
                let size = backend.size();
                if size == 0 {
                    Ok(Vec::new())
                } else {
                    backend.read_at_async(0, size as usize).await
                }
            }
            Err(_) => Ok(Vec::new()), // No existing data
        }
    }

    /// Loads data from IndexedDB.
    async fn load_from_indexeddb(db_name: &str, file_name: &str) -> WasmResult<Vec<u8>> {
        match IndexedDbBackend::open(db_name, file_name).await {
            Ok(backend) => {
                let size = backend.size();
                if size == 0 {
                    Ok(Vec::new())
                } else {
                    backend.read_at_async(0, size as usize).await
                }
            }
            Err(_) => Ok(Vec::new()), // No existing data
        }
    }

    /// Saves data to persistent storage.
    pub async fn save(&self) -> WasmResult<()> {
        let is_dirty = self.dirty.read().map_err(|_| {
            WasmError::Storage("Lock poisoned while checking dirty flag".to_string())
        })?;
        if !*is_dirty {
            return Ok(());
        }
        drop(is_dirty); // Release read lock before potentially acquiring write lock

        // Read all data from memory backend
        let size = self.memory.size().map_err(|e| {
            WasmError::Storage(format!("Failed to get size: {}", e))
        })? as usize;

        if size == 0 {
            let mut dirty = self.dirty.write().map_err(|_| {
                WasmError::Storage("Lock poisoned while clearing dirty flag".to_string())
            })?;
            *dirty = false;
            return Ok(());
        }

        let data = self.memory.read_at(0, size).map_err(|e| {
            WasmError::Storage(format!("Failed to read data: {}", e))
        })?;

        match self.storage_type {
            StorageType::Opfs => {
                let backend = OpfsBackend::open(&self.db_name, &self.file_name).await?;
                // Write all data (overwrites existing)
                backend.write_all(&data).await?;
                backend.flush_async().await?;
            }
            StorageType::IndexedDb => {
                let backend = IndexedDbBackend::open(&self.db_name, &self.file_name).await?;
                // Write all data (overwrites existing) - fixes append corruption bug
                backend.write_all(&data).await?;
                backend.flush_async().await?;
            }
            StorageType::Memory => {
                // Nothing to save
            }
        }

        let mut dirty = self.dirty.write().map_err(|_| {
            WasmError::Storage("Lock poisoned while clearing dirty flag".to_string())
        })?;
        *dirty = false;
        Ok(())
    }

    /// Returns the storage type.
    pub fn storage_type(&self) -> StorageType {
        self.storage_type
    }

    /// Returns the database name.
    pub fn db_name(&self) -> &str {
        &self.db_name
    }

    /// Returns the file name.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Deletes the persistent storage.
    pub async fn delete(db_name: &str) -> WasmResult<()> {
        // Try both storage types
        let _ = OpfsBackend::delete(db_name).await;
        let _ = IndexedDbBackend::delete(db_name).await;
        Ok(())
    }
}

// Note: We implement StorageBackend on a mutable reference since we need &mut self for append.
// The actual backend is used through interior mutability patterns in the database.
//
// IMPORTANT: Web Durability Limitation
// ------------------------------------
// The `flush()` and `sync()` methods below are synchronous stubs that only operate on
// the in-memory buffer. They do NOT persist data to disk.
//
// For WAL commit durability on web, callers MUST use the async `save()` method after
// committing. This is a fundamental limitation because:
// 1. OPFS and IndexedDB APIs are async-only in JavaScript
// 2. Rust's StorageBackend trait is synchronous
//
// The EntiDB WASM database wrapper handles this by calling `save()` after each commit.
// See docs/invariants.md section 11: "Browser storage MUST be treated as unreliable."
impl StorageBackend for PersistentBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        self.memory.read_at(offset, len)
    }

    fn append(&mut self, data: &[u8]) -> StorageResult<u64> {
        let mut dirty = self.dirty.write().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        *dirty = true;
        drop(dirty);
        self.memory.append(data)
    }

    fn flush(&mut self) -> StorageResult<()> {
        // Note: This is sync, so we can't actually persist here.
        // Use save() async method for persistence.
        self.memory.flush()
    }

    fn size(&self) -> StorageResult<u64> {
        self.memory.size()
    }

    fn sync(&mut self) -> StorageResult<()> {
        // Note: This is sync, so we can't actually persist here.
        // Use save() async method for persistence.
        self.memory.sync()
    }

    fn truncate(&mut self, new_size: u64) -> StorageResult<()> {
        let mut dirty = self.dirty.write().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        *dirty = true;
        drop(dirty);
        self.memory.truncate(new_size)
    }
}

/// A shared wrapper around `PersistentBackend` that implements `StorageBackend`.
///
/// This allows multiple owners (the WASM `Database` and the `CoreDatabase`) to
/// share access to the same underlying storage, which is required for the WASM
/// `Database::save()` method to persist the data stored by the core.
use std::sync::Arc;

/// Wrapper that provides shared access to a `PersistentBackend`.
///
/// This implements `StorageBackend` by delegating to the inner `PersistentBackend`,
/// using interior mutability through `RwLock` for mutation operations.
pub struct SharedPersistentBackend {
    inner: Arc<std::sync::RwLock<PersistentBackend>>,
}

impl SharedPersistentBackend {
    /// Creates a new shared backend wrapping the given `PersistentBackend`.
    pub fn new(backend: PersistentBackend) -> Self {
        Self {
            inner: Arc::new(std::sync::RwLock::new(backend)),
        }
    }

    /// Returns a clone of the inner Arc for shared access.
    pub fn shared(&self) -> Arc<std::sync::RwLock<PersistentBackend>> {
        Arc::clone(&self.inner)
    }

    /// Saves data to persistent storage asynchronously.
    ///
    /// This is the method that must be called to actually persist data
    /// to OPFS or IndexedDB.
    pub async fn save(&self) -> WasmResult<()> {
        let backend = self.inner.read().map_err(|_| {
            WasmError::Storage("Lock poisoned while reading backend for save".to_string())
        })?;
        backend.save().await
    }
}

impl StorageBackend for SharedPersistentBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        let backend = self.inner.read().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        backend.read_at(offset, len)
    }

    fn append(&mut self, data: &[u8]) -> StorageResult<u64> {
        let mut backend = self.inner.write().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        backend.append(data)
    }

    fn flush(&mut self) -> StorageResult<()> {
        let mut backend = self.inner.write().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        backend.flush()
    }

    fn size(&self) -> StorageResult<u64> {
        let backend = self.inner.read().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        backend.size()
    }

    fn sync(&mut self) -> StorageResult<()> {
        let mut backend = self.inner.write().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        backend.sync()
    }

    fn truncate(&mut self, new_size: u64) -> StorageResult<()> {
        let mut backend = self.inner.write().map_err(|_| {
            entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        backend.truncate(new_size)
    }
}
