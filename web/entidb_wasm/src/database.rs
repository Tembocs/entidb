//! Database WASM bindings.
//!
//! This module provides the main JavaScript-facing API for EntiDB.

use crate::entity::{Collection, EntityId};
use entidb_core::{Database as CoreDatabase, EntityId as CoreEntityId};
use js_sys::Array;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// An EntiDB database instance.
///
/// This is the main entry point for interacting with EntiDB from JavaScript.
/// It provides methods for storing and retrieving entities.
///
/// ## Example
///
/// ```javascript
/// const db = await Database.openMemory();
/// const users = db.collection("users");
///
/// const id = EntityId.generate();
/// db.put(users, id, new Uint8Array([1, 2, 3]));
///
/// const data = db.get(users, id);
/// db.close();
/// ```
#[wasm_bindgen]
pub struct Database {
    inner: Rc<RefCell<CoreDatabase>>,
    collections: Rc<RefCell<HashMap<String, u32>>>,
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
        })
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

    /// Closes the database.
    ///
    /// After calling this, the database cannot be used anymore.
    #[wasm_bindgen]
    pub fn close(&self) -> Result<(), JsValue> {
        let db = self.inner.borrow();
        db.close()
            .map_err(|e| JsValue::from_str(&format!("Failed to close database: {}", e)))?;
        Ok(())
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
