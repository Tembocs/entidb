//! Database WASM bindings.
//!
//! This module provides the main JavaScript-facing API for EntiDB.

use crate::backend::{PersistentBackend, SharedPersistentBackend, StorageType};
use crate::entity::{Collection, EntityId};
use entidb_core::{Database as CoreDatabase, EntityId as CoreEntityId};
use js_sys::Array;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

/// Storage type for JavaScript.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsStorageType {
    /// OPFS - Origin Private File System (best performance).
    Opfs,
    /// IndexedDB - Fallback for older browsers.
    IndexedDb,
    /// Memory - No persistence, lost when page closes.
    Memory,
}

impl From<StorageType> for JsStorageType {
    fn from(st: StorageType) -> Self {
        match st {
            StorageType::Opfs => JsStorageType::Opfs,
            StorageType::IndexedDb => JsStorageType::IndexedDb,
            StorageType::Memory => JsStorageType::Memory,
        }
    }
}

impl From<JsStorageType> for StorageType {
    fn from(js: JsStorageType) -> Self {
        match js {
            JsStorageType::Opfs => StorageType::Opfs,
            JsStorageType::IndexedDb => StorageType::IndexedDb,
            JsStorageType::Memory => StorageType::Memory,
        }
    }
}

/// An EntiDB database instance.
///
/// This is the main entry point for interacting with EntiDB from JavaScript.
/// It provides methods for storing and retrieving entities.
///
/// ## Durability Model
///
/// EntiDB WASM uses a **two-phase durability** model for performance:
///
/// 1. **Fast operations** (`put`, `delete`): Write to in-memory WAL only.
///    These are fast but NOT durable - data may be lost if the browser crashes
///    or the tab is closed before `save()` is called.
///
/// 2. **Durable operations** (`putDurable`, `deleteDurable`, `save`): Persist
///    data to OPFS/IndexedDB. These are async and slower, but guarantee that
///    data survives browser crashes.
///
/// **Choose your approach:**
/// - For high-throughput batching: Use `put`/`delete`, then call `save()` periodically
/// - For critical single writes: Use `putDurable`/`deleteDurable`
///
/// ## Example
///
/// ```javascript
/// // Persistent database (auto-selects OPFS or IndexedDB)
/// const db = await Database.open("mydb");
///
/// const users = db.collection("users");
/// const id = EntityId.generate();
///
/// // Option 1: Fast batch writes with explicit save
/// db.put(users, id, new Uint8Array([1, 2, 3]));
/// db.put(users, id2, new Uint8Array([4, 5, 6]));
/// await db.save(); // Persist all changes
///
/// // Option 2: Durable single write
/// await db.putDurable(users, id3, new Uint8Array([7, 8, 9]));
///
/// db.close();
/// ```
#[wasm_bindgen]
pub struct Database {
    inner: Rc<RefCell<CoreDatabase>>,
    collections: Rc<RefCell<HashMap<String, u32>>>,
    /// Database name (for persistence).
    db_name: Option<String>,
    /// Storage type used.
    storage_type: StorageType,
    /// Whether the database has unsaved changes.
    dirty: Rc<RefCell<bool>>,
    /// WAL backend for persistence (None for in-memory databases).
    /// This is an Arc to the same PersistentBackend used by CoreDatabase.
    wal_backend: Option<std::sync::Arc<std::sync::RwLock<PersistentBackend>>>,
    /// Segment backend for persistence (None for in-memory databases).
    /// This is an Arc to the same PersistentBackend used by CoreDatabase.
    segment_backend: Option<std::sync::Arc<std::sync::RwLock<PersistentBackend>>>,
}

#[wasm_bindgen]
impl Database {
    /// Opens an in-memory database.
    ///
    /// The database is stored entirely in memory and is lost when the
    /// page is closed. This is useful for testing or temporary data.
    #[wasm_bindgen(js_name = openMemory)]
    pub fn open_memory() -> Result<Database, JsValue> {
        let db = CoreDatabase::open_in_memory()
            .map_err(|e| JsValue::from_str(&format!("Failed to open database: {}", e)))?;

        Ok(Database {
            inner: Rc::new(RefCell::new(db)),
            collections: Rc::new(RefCell::new(HashMap::new())),
            db_name: None,
            storage_type: StorageType::Memory,
            dirty: Rc::new(RefCell::new(false)),
            wal_backend: None,
            segment_backend: None,
        })
    }

    /// Opens a persistent database.
    ///
    /// The database is stored in OPFS (preferred) or IndexedDB (fallback).
    /// Call `save()` to persist changes to storage.
    ///
    /// # Arguments
    ///
    /// * `name` - The database name (used for storage)
    #[wasm_bindgen]
    pub fn open(name: &str) -> js_sys::Promise {
        let name = name.to_string();
        future_to_promise(async move {
            let storage_type = StorageType::detect();
            Self::open_with_storage_type_internal(&name, storage_type)
                .await
                .map(|db| JsValue::from(db))
        })
    }

    /// Opens a persistent database with a specific storage type.
    ///
    /// # Arguments
    ///
    /// * `name` - The database name
    /// * `storage_type` - The storage type to use
    #[wasm_bindgen(js_name = openWithStorageType)]
    pub fn open_with_storage_type(name: &str, storage_type: JsStorageType) -> js_sys::Promise {
        let name = name.to_string();
        future_to_promise(async move {
            Self::open_with_storage_type_internal(&name, storage_type.into())
                .await
                .map(|db| JsValue::from(db))
        })
    }

    /// Internal async open implementation.
    async fn open_with_storage_type_internal(
        name: &str,
        storage_type: StorageType,
    ) -> Result<Database, JsValue> {
        // Load manifest from persistent storage (if exists)
        let manifest = Self::load_manifest(name, storage_type).await?;

        // Load WAL and segment backends from persistent storage
        let wal_persistent = PersistentBackend::open_with_type(name, "wal.log", storage_type)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to load WAL: {}", e)))?;

        let segment_persistent = PersistentBackend::open_with_type(name, "segments.dat", storage_type)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to load segments: {}", e)))?;

        // Wrap backends in SharedPersistentBackend for shared ownership.
        // This allows us to:
        // 1. Pass the backend to CoreDatabase (which needs Box<dyn StorageBackend>)
        // 2. Retain access via the Arc for calling save() later
        let wal_backend = SharedPersistentBackend::new(wal_persistent);
        let wal_arc = wal_backend.shared(); // Get Arc for later save()
        
        let segment_backend = SharedPersistentBackend::new(segment_persistent);
        let segment_arc = segment_backend.shared(); // Get Arc for later save()

        // Configure for web: use very large segment size to disable rotation.
        // Web storage uses a single file approach with in-memory backend,
        // so we cannot create multiple segment files. Setting max_segment_size
        // to u64::MAX effectively prevents rotation.
        let config = entidb_core::Config::default().max_segment_size(u64::MAX);

        // Open database with pre-loaded manifest for metadata persistence
        let db = CoreDatabase::open_with_backends_and_manifest(
            config,
            Box::new(wal_backend),
            Box::new(segment_backend),
            manifest,
        )
        .map_err(|e| JsValue::from_str(&format!("Failed to open database: {}", e)))?;

        Ok(Database {
            inner: Rc::new(RefCell::new(db)),
            collections: Rc::new(RefCell::new(HashMap::new())),
            db_name: Some(name.to_string()),
            storage_type,
            dirty: Rc::new(RefCell::new(false)),
            wal_backend: Some(wal_arc),
            segment_backend: Some(segment_arc),
        })
    }

    /// Loads the manifest from persistent storage.
    async fn load_manifest(
        db_name: &str,
        storage_type: StorageType,
    ) -> Result<Option<entidb_core::Manifest>, JsValue> {
        use crate::backend::{IndexedDbBackend, OpfsBackend};

        let manifest_bytes = match storage_type {
            StorageType::Opfs => {
                let backend = OpfsBackend::open(db_name, "manifest.bin").await;
                match backend {
                    Ok(b) => {
                        let size = b.size();
                        if size > 0 {
                            b.read_at_async(0, size as usize).await.ok()
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            }
            StorageType::IndexedDb => {
                let backend = IndexedDbBackend::open(db_name, "manifest.bin").await;
                match backend {
                    Ok(b) => {
                        let size = b.size();
                        if size > 0 {
                            b.read_at_async(0, size as usize).await.ok()
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            }
            StorageType::Memory => None,
        };

        if let Some(bytes) = manifest_bytes {
            if !bytes.is_empty() {
                match entidb_core::Manifest::decode(&bytes) {
                    Ok(m) => return Ok(Some(m)),
                    Err(e) => {
                        // Log warning but continue with fresh manifest
                        web_sys::console::warn_1(
                            &format!("Failed to decode manifest, starting fresh: {}", e).into(),
                        );
                    }
                }
            }
        }

        Ok(None)
    }

    /// Saves the manifest to persistent storage.
    async fn save_manifest_internal(
        db_name: &str,
        storage_type: StorageType,
        manifest: &entidb_core::Manifest,
    ) -> Result<(), JsValue> {
        use crate::backend::{IndexedDbBackend, OpfsBackend};

        let bytes = manifest
            .encode()
            .map_err(|e| JsValue::from_str(&format!("Failed to encode manifest: {}", e)))?;

        match storage_type {
            StorageType::Opfs => {
                let backend = OpfsBackend::open(db_name, "manifest.bin")
                    .await
                    .map_err(|e| JsValue::from_str(&format!("Failed to open manifest: {}", e)))?;
                backend
                    .write_all(&bytes)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("Failed to write manifest: {}", e)))?;
                backend
                    .flush_async()
                    .await
                    .map_err(|e| JsValue::from_str(&format!("Failed to flush manifest: {}", e)))?;
            }
            StorageType::IndexedDb => {
                let backend = IndexedDbBackend::open(db_name, "manifest.bin")
                    .await
                    .map_err(|e| JsValue::from_str(&format!("Failed to open manifest: {}", e)))?;
                backend
                    .write_all(&bytes)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("Failed to write manifest: {}", e)))?;
                backend
                    .flush_async()
                    .await
                    .map_err(|e| JsValue::from_str(&format!("Failed to flush manifest: {}", e)))?;
            }
            StorageType::Memory => {
                // Nothing to save
            }
        }

        Ok(())
    }

    /// Returns the storage type used by this database.
    #[wasm_bindgen(getter, js_name = storageType)]
    pub fn storage_type(&self) -> JsStorageType {
        self.storage_type.into()
    }

    /// Returns whether the database is persistent.
    #[wasm_bindgen(getter, js_name = isPersistent)]
    pub fn is_persistent(&self) -> bool {
        self.db_name.is_some() && self.storage_type != StorageType::Memory
    }

    /// Checks if OPFS is available in the current browser.
    #[wasm_bindgen(js_name = isOpfsAvailable)]
    pub fn is_opfs_available() -> bool {
        crate::backend::is_opfs_available()
    }

    /// Checks if IndexedDB is available.
    #[wasm_bindgen(js_name = isIndexedDbAvailable)]
    pub fn is_indexeddb_available() -> bool {
        crate::backend::is_indexeddb_available()
    }

    /// Gets or creates a collection by name.
    ///
    /// Collections are created automatically when first accessed.
    /// The returned Collection object can be used for subsequent operations.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the collection
    ///
    /// # Errors
    ///
    /// Returns an error if collection creation fails (e.g., storage error).
    #[wasm_bindgen]
    pub fn collection(&self, name: &str) -> Result<Collection, JsValue> {
        let mut collections = self.collections.borrow_mut();

        if let Some(&id) = collections.get(name) {
            return Ok(Collection::new(name.to_string(), id));
        }

        // Get or create the collection in the database using the proper Result-returning API
        let db = self.inner.borrow();
        let collection_id = db.create_collection(name)
            .map_err(|e| JsValue::from_str(&format!("Failed to create collection '{}': {}", name, e)))?;

        collections.insert(name.to_string(), collection_id.0);
        Ok(Collection::new(name.to_string(), collection_id.0))
    }

    /// Stores an entity in a collection.
    ///
    /// If an entity with the same ID already exists, it will be replaced.
    ///
    /// **Important:** This method does NOT guarantee durability. The data is
    /// written to the in-memory WAL but not persisted to storage until `save()`
    /// is called. For durable writes, use `putDurable()` instead.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to store in
    /// * `id` - The entity ID
    /// * `data` - The entity data as bytes (should be CBOR-encoded)
    #[wasm_bindgen]
    pub fn put(&self, collection: &Collection, id: &EntityId, data: &[u8]) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let entity_id: CoreEntityId = (*id).into();
        let data_vec = data.to_vec();

        db.transaction(|txn| {
            txn.put(collection_id, entity_id, data_vec.clone())?;
            Ok(())
        })
        .map_err(|e| JsValue::from_str(&format!("Failed to put entity: {}", e)))?;

        *self.dirty.borrow_mut() = true;
        Ok(())
    }

    /// Stores an entity with guaranteed durability.
    ///
    /// This is an async method that writes the entity AND persists to storage
    /// before returning. Use this when you need to ensure data survives browser
    /// crashes or tab closures.
    ///
    /// This is equivalent to calling `put()` followed by `save()`.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to store in
    /// * `id` - The entity ID
    /// * `data` - The entity data as bytes (should be CBOR-encoded)
    #[wasm_bindgen(js_name = putDurable)]
    pub fn put_durable(
        &self,
        collection: &Collection,
        id: &EntityId,
        data: &[u8],
    ) -> js_sys::Promise {
        // Perform the put synchronously first
        let put_result = self.put(collection, id, data);
        if let Err(e) = put_result {
            return js_sys::Promise::reject(&e);
        }

        // Then save asynchronously
        self.save()
    }

    /// Retrieves an entity from a collection.
    ///
    /// Returns `null` if the entity does not exist.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to read from
    /// * `id` - The entity ID
    #[wasm_bindgen]
    pub fn get(&self, collection: &Collection, id: &EntityId) -> Result<Option<Vec<u8>>, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let entity_id: CoreEntityId = (*id).into();

        db.get(collection_id, entity_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to get entity: {}", e)))
    }

    /// Deletes an entity from a collection.
    ///
    /// Does nothing if the entity does not exist.
    ///
    /// **Important:** This method does NOT guarantee durability. The delete is
    /// written to the in-memory WAL but not persisted to storage until `save()`
    /// is called. For durable deletes, use `deleteDurable()` instead.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to delete from
    /// * `id` - The entity ID
    #[wasm_bindgen]
    pub fn delete(&self, collection: &Collection, id: &EntityId) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let entity_id: CoreEntityId = (*id).into();

        db.transaction(|txn| {
            txn.delete(collection_id, entity_id)?;
            Ok(())
        })
        .map_err(|e| JsValue::from_str(&format!("Failed to delete entity: {}", e)))?;

        *self.dirty.borrow_mut() = true;
        Ok(())
    }

    /// Deletes an entity with guaranteed durability.
    ///
    /// This is an async method that deletes the entity AND persists to storage
    /// before returning. Use this when you need to ensure the deletion survives
    /// browser crashes or tab closures.
    ///
    /// This is equivalent to calling `delete()` followed by `save()`.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to delete from
    /// * `id` - The entity ID
    #[wasm_bindgen(js_name = deleteDurable)]
    pub fn delete_durable(&self, collection: &Collection, id: &EntityId) -> js_sys::Promise {
        // Perform the delete synchronously first
        let delete_result = self.delete(collection, id);
        if let Err(e) = delete_result {
            return js_sys::Promise::reject(&e);
        }

        // Then save asynchronously
        self.save()
    }

    /// Lists all entities in a collection.
    ///
    /// Returns an array of `[EntityId, Uint8Array]` pairs.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to list
    #[wasm_bindgen]
    pub fn list(&self, collection: &Collection) -> Result<Array, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());

        let entities = db
            .list(collection_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to list entities: {}", e)))?;

        let result = Array::new();
        for (id, data) in entities {
            let pair = Array::new();
            let wasm_id: EntityId = id.into();
            pair.push(&wasm_id.into());
            let uint8_array = js_sys::Uint8Array::from(data.as_slice());
            pair.push(&uint8_array.into());
            result.push(&pair);
        }

        Ok(result)
    }

    /// Counts entities in a collection.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to count
    #[wasm_bindgen]
    pub fn count(&self, collection: &Collection) -> Result<u32, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());

        let entities = db
            .list(collection_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to count entities: {}", e)))?;

        Ok(entities.len() as u32)
    }

    /// Creates a checkpoint for crash recovery.
    #[wasm_bindgen]
    pub fn checkpoint(&self) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        db.checkpoint()
            .map_err(|e| JsValue::from_str(&format!("Failed to checkpoint: {}", e)))?;
        Ok(())
    }

    /// Creates a backup of the database.
    ///
    /// Returns the backup data as a Uint8Array that can be saved or transferred.
    /// Use `restore()` to restore from a backup.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const backup = db.backup();
    /// // Save backup to a file or send to server
    /// const blob = new Blob([backup], { type: 'application/octet-stream' });
    /// ```
    #[wasm_bindgen]
    pub fn backup(&self) -> Result<js_sys::Uint8Array, JsValue> {
        let db = self.inner.borrow();
        let data = db
            .backup()
            .map_err(|e| JsValue::from_str(&format!("Failed to create backup: {}", e)))?;
        Ok(js_sys::Uint8Array::from(data.as_slice()))
    }

    /// Creates a backup with custom options.
    ///
    /// # Arguments
    ///
    /// * `include_tombstones` - Whether to include deleted entities
    #[wasm_bindgen(js_name = backupWithOptions)]
    pub fn backup_with_options(&self, include_tombstones: bool) -> Result<js_sys::Uint8Array, JsValue> {
        let db = self.inner.borrow();
        let data = db
            .backup_with_options(include_tombstones)
            .map_err(|e| JsValue::from_str(&format!("Failed to create backup: {}", e)))?;
        Ok(js_sys::Uint8Array::from(data.as_slice()))
    }

    /// Restores the database from a backup.
    ///
    /// This replaces all current data with the backup data.
    /// The backup must have been created with the `backup()` method.
    ///
    /// # Arguments
    ///
    /// * `data` - The backup data as a Uint8Array
    ///
    /// # Returns
    ///
    /// The number of entities restored.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const restoredCount = db.restore(backupData);
    /// console.log(`Restored ${restoredCount} entities`);
    /// ```
    #[wasm_bindgen]
    pub fn restore(&self, data: &[u8]) -> Result<u32, JsValue> {
        let db = self.inner.borrow();
        let stats = db
            .restore(data)
            .map_err(|e| JsValue::from_str(&format!("Failed to restore from backup: {}", e)))?;
        Ok(stats.entities_restored as u32)
    }

    /// Validates backup data without restoring it.
    ///
    /// Returns information about the backup if valid.
    ///
    /// # Arguments
    ///
    /// * `data` - The backup data to validate
    ///
    /// # Returns
    ///
    /// An object with backup information: { valid: boolean, recordCount: number, timestamp: number, sequence: number, size: number }
    #[wasm_bindgen(js_name = validateBackup)]
    pub fn validate_backup(&self, data: &[u8]) -> Result<JsValue, JsValue> {
        let db = self.inner.borrow();
        match db.validate_backup(data) {
            Ok(info) => {
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"valid".into(), &JsValue::from(info.valid))?;
                js_sys::Reflect::set(&obj, &"recordCount".into(), &JsValue::from(info.record_count))?;
                js_sys::Reflect::set(&obj, &"timestamp".into(), &JsValue::from(info.timestamp as f64))?;
                js_sys::Reflect::set(&obj, &"sequence".into(), &JsValue::from(info.sequence as f64))?;
                js_sys::Reflect::set(&obj, &"size".into(), &JsValue::from(info.size as u32))?;
                Ok(obj.into())
            }
            Err(e) => {
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"valid".into(), &JsValue::FALSE)?;
                js_sys::Reflect::set(&obj, &"error".into(), &JsValue::from_str(&e.to_string()))?;
                Ok(obj.into())
            }
        }
    }

    /// Compacts the database, removing obsolete versions and optionally tombstones.
    ///
    /// # Arguments
    ///
    /// * `remove_tombstones` - If true, tombstones (deleted entities) are removed
    ///
    /// # Returns
    ///
    /// An object with compaction statistics.
    #[wasm_bindgen]
    pub fn compact(&self, remove_tombstones: bool) -> Result<JsValue, JsValue> {
        let db = self.inner.borrow();
        let stats = db
            .compact(remove_tombstones)
            .map_err(|e| JsValue::from_str(&format!("Failed to compact database: {}", e)))?;

        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"inputRecords".into(), &JsValue::from(stats.input_records as u32))?;
        js_sys::Reflect::set(&obj, &"outputRecords".into(), &JsValue::from(stats.output_records as u32))?;
        js_sys::Reflect::set(&obj, &"tombstonesRemoved".into(), &JsValue::from(stats.tombstones_removed as u32))?;
        js_sys::Reflect::set(&obj, &"obsoleteVersionsRemoved".into(), &JsValue::from(stats.obsolete_versions_removed as u32))?;
        js_sys::Reflect::set(&obj, &"bytesSaved".into(), &JsValue::from(stats.bytes_saved as u32))?;
        Ok(obj.into())
    }

    /// Saves the database to persistent storage.
    ///
    /// This must be called to persist changes to OPFS/IndexedDB.
    /// For in-memory databases, this is a no-op.
    ///
    /// **Important:** Changes are NOT automatically persisted.
    /// Call this method before closing the page or when you want
    /// to ensure data is saved.
    ///
    /// This method ensures that all committed data is written to durable
    /// storage. It first creates a checkpoint, then persists the WAL,
    /// segment data, and manifest to OPFS or IndexedDB. A commit is not
    /// considered durable until save() completes successfully.
    #[wasm_bindgen]
    pub fn save(&self) -> js_sys::Promise {
        let storage_type = self.storage_type;
        let dirty = Rc::clone(&self.dirty);
        let db_name = self.db_name.clone();

        // Clone the backend Arcs for use in the async closure
        let wal_backend = self.wal_backend.clone();
        let segment_backend = self.segment_backend.clone();

        // Create a checkpoint to ensure all committed data is flushed to the backends
        if let Err(e) = self.checkpoint() {
            return js_sys::Promise::reject(&e);
        }

        // Get the current manifest for persistence
        let manifest = self.inner.borrow().get_manifest();

        future_to_promise(async move {
            if storage_type == StorageType::Memory {
                // Nothing to save for in-memory databases
                return Ok(JsValue::UNDEFINED);
            }

            let db_name = db_name.ok_or_else(|| {
                JsValue::from_str("Cannot save: database has no name")
            })?;

            // Persist the manifest first (metadata is critical)
            Self::save_manifest_internal(&db_name, storage_type, &manifest).await?;

            // Persist the WAL backend to durable storage
            if let Some(wal_arc) = wal_backend {
                let backend = wal_arc.read().map_err(|_| {
                    JsValue::from_str("Failed to acquire lock on WAL backend for save")
                })?;
                backend.save().await.map_err(|e| {
                    JsValue::from_str(&format!("Failed to save WAL: {}", e))
                })?;
            }

            // Persist the segment backend to durable storage
            if let Some(segment_arc) = segment_backend {
                let backend = segment_arc.read().map_err(|_| {
                    JsValue::from_str("Failed to acquire lock on segment backend for save")
                })?;
                backend.save().await.map_err(|e| {
                    JsValue::from_str(&format!("Failed to save segments: {}", e))
                })?;
            }

            // Clear the dirty flag only after successful persistence
            *dirty.borrow_mut() = false;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Returns whether there are unsaved changes.
    #[wasm_bindgen(getter, js_name = hasUnsavedChanges)]
    pub fn has_unsaved_changes(&self) -> bool {
        *self.dirty.borrow()
    }

    /// Closes the database.
    ///
    /// After calling this, the database cannot be used anymore.
    /// **Note:** This does NOT automatically save. Call `save()` first
    /// if you need to persist changes.
    #[wasm_bindgen]
    pub fn close(&self) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        db.close()
            .map_err(|e| JsValue::from_str(&format!("Failed to close database: {}", e)))?;
        Ok(())
    }

    /// Deletes a database from persistent storage.
    ///
    /// This permanently removes all data for the specified database.
    /// The database must be closed first.
    ///
    /// # Arguments
    ///
    /// * `name` - The database name to delete
    #[wasm_bindgen(js_name = deleteDatabase)]
    pub fn delete_database(name: &str) -> js_sys::Promise {
        let name = name.to_string();
        future_to_promise(async move {
            PersistentBackend::delete(&name)
                .await
                .map_err(|e| JsValue::from_str(&format!("Failed to delete database: {}", e)))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Returns the EntiDB version.
    #[wasm_bindgen(getter)]
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    // =========================================================================
    // Statistics API
    // =========================================================================

    /// Returns database statistics.
    ///
    /// Returns an object with:
    /// - `reads`: Number of read operations
    /// - `writes`: Number of write operations
    /// - `deletes`: Number of delete operations
    /// - `scans`: Number of scan operations
    /// - `indexLookups`: Number of index lookups
    /// - `transactionsStarted`: Number of transactions started
    /// - `transactionsCommitted`: Number of transactions committed
    /// - `transactionsAborted`: Number of transactions aborted
    /// - `bytesRead`: Total bytes read
    /// - `bytesWritten`: Total bytes written
    /// - `checkpoints`: Number of checkpoints
    /// - `errors`: Number of errors
    /// - `entityCount`: Total entity count
    ///
    /// # Example
    ///
    /// ```javascript
    /// const stats = db.stats();
    /// console.log(`Reads: ${stats.reads}, Writes: ${stats.writes}`);
    /// ```
    #[wasm_bindgen]
    pub fn stats(&self) -> Result<JsValue, JsValue> {
        let db = self.inner.borrow();
        let s = db.stats();

        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"reads".into(), &JsValue::from(s.reads as u32))?;
        js_sys::Reflect::set(&obj, &"writes".into(), &JsValue::from(s.writes as u32))?;
        js_sys::Reflect::set(&obj, &"deletes".into(), &JsValue::from(s.deletes as u32))?;
        js_sys::Reflect::set(&obj, &"scans".into(), &JsValue::from(s.scans as u32))?;
        js_sys::Reflect::set(&obj, &"indexLookups".into(), &JsValue::from(s.index_lookups as u32))?;
        js_sys::Reflect::set(&obj, &"transactionsStarted".into(), &JsValue::from(s.transactions_started as u32))?;
        js_sys::Reflect::set(&obj, &"transactionsCommitted".into(), &JsValue::from(s.transactions_committed as u32))?;
        js_sys::Reflect::set(&obj, &"transactionsAborted".into(), &JsValue::from(s.transactions_aborted as u32))?;
        js_sys::Reflect::set(&obj, &"bytesRead".into(), &JsValue::from(s.bytes_read as f64))?;
        js_sys::Reflect::set(&obj, &"bytesWritten".into(), &JsValue::from(s.bytes_written as f64))?;
        js_sys::Reflect::set(&obj, &"checkpoints".into(), &JsValue::from(s.checkpoints as u32))?;
        js_sys::Reflect::set(&obj, &"errors".into(), &JsValue::from(s.errors as u32))?;
        js_sys::Reflect::set(&obj, &"entityCount".into(), &JsValue::from(s.entity_count as u32))?;
        Ok(obj.into())
    }

    // =========================================================================
    // Index Management API
    // =========================================================================

    /// Creates a hash index for O(1) equality lookups.
    ///
    /// Hash indexes are ideal for exact-match queries like looking up
    /// entities by email, username, or any unique identifier.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to create the index on
    /// * `name` - The index name (unique within the collection)
    /// * `unique` - Whether the index should enforce unique keys
    ///
    /// # Example
    ///
    /// ```javascript
    /// const users = db.collection("users");
    /// db.createHashIndex(users, "email", true); // unique email index
    /// ```
    #[wasm_bindgen(js_name = createHashIndex)]
    pub fn create_hash_index(
        &self,
        collection: &Collection,
        name: &str,
        unique: bool,
    ) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        db.create_hash_index(collection_id, name, unique)
            .map_err(|e| JsValue::from_str(&format!("Failed to create hash index: {}", e)))
    }

    /// Creates a BTree index for range queries and ordered traversal.
    ///
    /// BTree indexes support:
    /// - Exact-match queries
    /// - Range queries (greater than, less than, between)
    /// - Ordered iteration
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection to create the index on
    /// * `name` - The index name (unique within the collection)
    /// * `unique` - Whether the index should enforce unique keys
    ///
    /// # Example
    ///
    /// ```javascript
    /// const users = db.collection("users");
    /// db.createBTreeIndex(users, "age", false); // non-unique age index
    /// ```
    #[wasm_bindgen(js_name = createBTreeIndex)]
    pub fn create_btree_index(
        &self,
        collection: &Collection,
        name: &str,
        unique: bool,
    ) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        db.create_btree_index(collection_id, name, unique)
            .map_err(|e| JsValue::from_str(&format!("Failed to create btree index: {}", e)))
    }

    /// Inserts a key-entity pair into a hash index.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection the index belongs to
    /// * `index_name` - The name of the hash index
    /// * `key` - The key bytes
    /// * `entity_id` - The entity to associate with the key
    #[wasm_bindgen(js_name = hashIndexInsert)]
    pub fn hash_index_insert(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ent_id: entidb_core::EntityId = (*entity_id).into();
        db.hash_index_insert(collection_id, index_name, key.to_vec(), ent_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to insert into hash index: {}", e)))
    }

    /// Inserts a key-entity pair into a BTree index.
    ///
    /// Note: For proper ordering, use big-endian encoding for numeric keys.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection the index belongs to
    /// * `index_name` - The name of the BTree index
    /// * `key` - The key bytes
    /// * `entity_id` - The entity to associate with the key
    #[wasm_bindgen(js_name = btreeIndexInsert)]
    pub fn btree_index_insert(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ent_id: entidb_core::EntityId = (*entity_id).into();
        db.btree_index_insert(collection_id, index_name, key.to_vec(), ent_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to insert into btree index: {}", e)))
    }

    /// Removes a key-entity pair from a hash index.
    ///
    /// Returns true if the entry was found and removed.
    #[wasm_bindgen(js_name = hashIndexRemove)]
    pub fn hash_index_remove(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> Result<bool, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ent_id: entidb_core::EntityId = (*entity_id).into();
        db.hash_index_remove(collection_id, index_name, key, ent_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to remove from hash index: {}", e)))
    }

    /// Removes a key-entity pair from a BTree index.
    ///
    /// Returns true if the entry was found and removed.
    #[wasm_bindgen(js_name = btreeIndexRemove)]
    pub fn btree_index_remove(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
        entity_id: &EntityId,
    ) -> Result<bool, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ent_id: entidb_core::EntityId = (*entity_id).into();
        db.btree_index_remove(collection_id, index_name, key, ent_id)
            .map_err(|e| JsValue::from_str(&format!("Failed to remove from btree index: {}", e)))
    }

    /// Looks up entities by key in a hash index.
    ///
    /// Returns an array of EntityIds matching the key.
    #[wasm_bindgen(js_name = hashIndexLookup)]
    pub fn hash_index_lookup(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
    ) -> Result<Array, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ids = db
            .hash_index_lookup(collection_id, index_name, key)
            .map_err(|e| JsValue::from_str(&format!("Failed to lookup in hash index: {}", e)))?;

        let result = Array::new();
        for id in ids {
            let wasm_id: EntityId = id.into();
            result.push(&wasm_id.into());
        }
        Ok(result)
    }

    /// Looks up entities by key in a BTree index.
    ///
    /// Returns an array of EntityIds matching the key.
    #[wasm_bindgen(js_name = btreeIndexLookup)]
    pub fn btree_index_lookup(
        &self,
        collection: &Collection,
        index_name: &str,
        key: &[u8],
    ) -> Result<Array, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ids = db
            .btree_index_lookup(collection_id, index_name, key)
            .map_err(|e| JsValue::from_str(&format!("Failed to lookup in btree index: {}", e)))?;

        let result = Array::new();
        for id in ids {
            let wasm_id: EntityId = id.into();
            result.push(&wasm_id.into());
        }
        Ok(result)
    }

    /// Performs a range query on a BTree index.
    ///
    /// Returns all entities whose key is >= minKey and <= maxKey.
    /// Pass null/undefined for unbounded ends.
    ///
    /// # Arguments
    ///
    /// * `collection` - The collection the index belongs to
    /// * `index_name` - The name of the BTree index
    /// * `min_key` - The minimum key (inclusive), or null for unbounded
    /// * `max_key` - The maximum key (inclusive), or null for unbounded
    ///
    /// # Example
    ///
    /// ```javascript
    /// // Find all users with age between 18 and 65
    /// const ageMin = new Uint8Array([0, 0, 0, 18]); // big-endian
    /// const ageMax = new Uint8Array([0, 0, 0, 65]);
    /// const ids = db.btreeIndexRange(users, "age", ageMin, ageMax);
    /// ```
    #[wasm_bindgen(js_name = btreeIndexRange)]
    pub fn btree_index_range(
        &self,
        collection: &Collection,
        index_name: &str,
        min_key: Option<Vec<u8>>,
        max_key: Option<Vec<u8>>,
    ) -> Result<Array, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        let ids = db
            .btree_index_range(
                collection_id,
                index_name,
                min_key.as_deref(),
                max_key.as_deref(),
            )
            .map_err(|e| JsValue::from_str(&format!("Failed to perform range query: {}", e)))?;

        let result = Array::new();
        for id in ids {
            let wasm_id: EntityId = id.into();
            result.push(&wasm_id.into());
        }
        Ok(result)
    }

    /// Returns the number of entries in a hash index.
    #[wasm_bindgen(js_name = hashIndexLen)]
    pub fn hash_index_len(
        &self,
        collection: &Collection,
        index_name: &str,
    ) -> Result<u32, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        db.hash_index_len(collection_id, index_name)
            .map(|len| len as u32)
            .map_err(|e| JsValue::from_str(&format!("Failed to get hash index length: {}", e)))
    }

    /// Returns the number of entries in a BTree index.
    #[wasm_bindgen(js_name = btreeIndexLen)]
    pub fn btree_index_len(
        &self,
        collection: &Collection,
        index_name: &str,
    ) -> Result<u32, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        db.btree_index_len(collection_id, index_name)
            .map(|len| len as u32)
            .map_err(|e| JsValue::from_str(&format!("Failed to get btree index length: {}", e)))
    }

    /// Drops a hash index.
    ///
    /// Returns true if the index existed and was dropped.
    #[wasm_bindgen(js_name = dropHashIndex)]
    pub fn drop_hash_index(
        &self,
        collection: &Collection,
        index_name: &str,
    ) -> Result<bool, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        db.drop_hash_index(collection_id, index_name)
            .map_err(|e| JsValue::from_str(&format!("Failed to drop hash index: {}", e)))
    }

    /// Drops a BTree index.
    ///
    /// Returns true if the index existed and was dropped.
    #[wasm_bindgen(js_name = dropBTreeIndex)]
    pub fn drop_btree_index(
        &self,
        collection: &Collection,
        index_name: &str,
    ) -> Result<bool, JsValue> {
        let db = self.inner.borrow();
        let collection_id = entidb_core::CollectionId(collection.id());
        db.drop_btree_index(collection_id, index_name)
            .map_err(|e| JsValue::from_str(&format!("Failed to drop btree index: {}", e)))
    }
}

/// A transaction for atomic operations.
///
/// Transactions ensure that multiple operations are applied atomically.
/// Either all operations succeed, or none are applied.
#[wasm_bindgen]
pub struct Transaction {
    database: Rc<RefCell<CoreDatabase>>,
    pending_puts: RefCell<Vec<(u32, CoreEntityId, Vec<u8>)>>,
    pending_deletes: RefCell<Vec<(u32, CoreEntityId)>>,
    committed: RefCell<bool>,
}

#[wasm_bindgen]
impl Transaction {
    /// Stages a put operation in the transaction.
    #[wasm_bindgen]
    pub fn put(&self, collection: &Collection, id: &EntityId, data: &[u8]) -> Result<(), JsValue> {
        if *self.committed.borrow() {
            return Err(JsValue::from_str("Transaction already committed"));
        }

        let entity_id: CoreEntityId = (*id).into();
        self.pending_puts
            .borrow_mut()
            .push((collection.id(), entity_id, data.to_vec()));
        Ok(())
    }

    /// Stages a delete operation in the transaction.
    #[wasm_bindgen]
    pub fn delete(&self, collection: &Collection, id: &EntityId) -> Result<(), JsValue> {
        if *self.committed.borrow() {
            return Err(JsValue::from_str("Transaction already committed"));
        }

        let entity_id: CoreEntityId = (*id).into();
        self.pending_deletes
            .borrow_mut()
            .push((collection.id(), entity_id));
        Ok(())
    }

    /// Commits the transaction.
    ///
    /// All staged operations are applied atomically.
    #[wasm_bindgen]
    pub fn commit(&self) -> Result<(), JsValue> {
        if *self.committed.borrow() {
            return Err(JsValue::from_str("Transaction already committed"));
        }

        let db = self.database.borrow();
        let pending_puts: Vec<_> = self.pending_puts.borrow().clone();
        let pending_deletes: Vec<_> = self.pending_deletes.borrow().clone();

        db.transaction(|txn| {
            // Apply all puts
            for (collection_id, entity_id, data) in &pending_puts {
                txn.put(entidb_core::CollectionId(*collection_id), *entity_id, data.clone())?;
            }

            // Apply all deletes
            for (collection_id, entity_id) in &pending_deletes {
                txn.delete(entidb_core::CollectionId(*collection_id), *entity_id)?;
            }

            Ok(())
        })
        .map_err(|e| JsValue::from_str(&format!("Transaction commit failed: {}", e)))?;

        *self.committed.borrow_mut() = true;
        Ok(())
    }

    /// Aborts the transaction.
    ///
    /// All staged operations are discarded.
    #[wasm_bindgen]
    pub fn abort(&self) -> Result<(), JsValue> {
        if *self.committed.borrow() {
            return Err(JsValue::from_str("Transaction already committed"));
        }

        self.pending_puts.borrow_mut().clear();
        self.pending_deletes.borrow_mut().clear();
        *self.committed.borrow_mut() = true;
        Ok(())
    }
}
