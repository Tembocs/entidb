//! Database WASM bindings.
//!
//! This module provides the main JavaScript-facing API for EntiDB.

use crate::backend::{PersistentBackend, StorageType, WasmMemoryBackend};
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
/// ## Example
///
/// ```javascript
/// // In-memory database (no persistence)
/// const db = await Database.openMemory();
///
/// // Persistent database (auto-selects OPFS or IndexedDB)
/// const db = await Database.open("mydb");
///
/// const users = db.collection("users");
/// const id = EntityId.generate();
/// db.put(users, id, new Uint8Array([1, 2, 3]));
///
/// const data = db.get(users, id);
///
/// // Save to persistent storage (important!)
/// await db.save();
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
        // Load WAL and segment backends from persistent storage
        let wal_backend = PersistentBackend::open_with_type(name, "wal.log", storage_type)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to load WAL: {}", e)))?;

        let segment_backend = PersistentBackend::open_with_type(name, "segments.dat", storage_type)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to load segments: {}", e)))?;

        let db = CoreDatabase::open_with_backends(
            entidb_core::Config::default(),
            Box::new(wal_backend),
            Box::new(segment_backend),
        )
        .map_err(|e| JsValue::from_str(&format!("Failed to open database: {}", e)))?;

        Ok(Database {
            inner: Rc::new(RefCell::new(db)),
            collections: Rc::new(RefCell::new(HashMap::new())),
            db_name: Some(name.to_string()),
            storage_type,
            dirty: Rc::new(RefCell::new(false)),
        })
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
    #[wasm_bindgen]
    pub fn collection(&self, name: &str) -> Result<Collection, JsValue> {
        let mut collections = self.collections.borrow_mut();

        if let Some(&id) = collections.get(name) {
            return Ok(Collection::new(name.to_string(), id));
        }

        // Get or create the collection in the database
        let db = self.inner.borrow();
        let collection_id = db.collection(name);

        collections.insert(name.to_string(), collection_id.0);
        Ok(Collection::new(name.to_string(), collection_id.0))
    }

    /// Stores an entity in a collection.
    ///
    /// If an entity with the same ID already exists, it will be replaced.
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

    /// Saves the database to persistent storage.
    ///
    /// This must be called to persist changes to OPFS/IndexedDB.
    /// For in-memory databases, this is a no-op.
    ///
    /// **Important:** Changes are NOT automatically persisted.
    /// Call this method before closing the page or when you want
    /// to ensure data is saved.
    #[wasm_bindgen]
    pub fn save(&self) -> js_sys::Promise {
        let db_name = self.db_name.clone();
        let storage_type = self.storage_type;
        let dirty = Rc::clone(&self.dirty);

        // We need to checkpoint and get the backend data
        if let Err(e) = self.checkpoint() {
            return js_sys::Promise::reject(&e);
        }

        future_to_promise(async move {
            if db_name.is_none() || storage_type == StorageType::Memory {
                // Nothing to save for in-memory databases
                return Ok(JsValue::UNDEFINED);
            }

            if !*dirty.borrow() {
                // No changes to save
                return Ok(JsValue::UNDEFINED);
            }

            // Note: The actual save happens through the PersistentBackend's flush
            // which is called during checkpoint. This method is here for the API
            // and to ensure the user explicitly saves.
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
