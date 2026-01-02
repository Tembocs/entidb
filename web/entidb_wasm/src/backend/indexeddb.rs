//! Real IndexedDB storage backend.
//!
//! This module provides a proper IndexedDB-backed byte store that:
//! - Uses actual IndexedDB (not localStorage)
//! - Supports large data (limited by browser quotas, typically GBs)
//! - Provides durable persistence
//! - Works in all modern browsers
//!
//! ## Design
//!
//! The backend stores data as a single binary blob in an IndexedDB object store.
//! This matches the byte-store abstraction expected by EntiDB - the backend
//! doesn't interpret the data, just stores and retrieves bytes.
//!
//! ## Usage
//!
//! ```ignore
//! let backend = IndexedDbBackend::open("mydb", "wal").await?;
//! backend.write_all(&data).await?;
//! backend.flush_async().await?;
//! ```

use crate::error::{WasmError, WasmResult};
use futures_channel::oneshot;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Key used to store the data blob in IndexedDB.
const DATA_KEY: &str = "data";

/// Real IndexedDB storage backend.
///
/// This backend stores data as a single binary blob in an IndexedDB object store,
/// providing a proper byte-store abstraction for EntiDB.
pub struct IndexedDbBackend {
    /// Database name.
    db_name: String,
    /// Object store name (acts like a file name).
    store_name: String,
    /// Cached data (loaded on open, written on save).
    data: Rc<RefCell<Vec<u8>>>,
    /// Whether the cache has uncommitted changes.
    dirty: Rc<RefCell<bool>>,
}

impl IndexedDbBackend {
    /// Opens or creates an IndexedDB storage backend.
    ///
    /// # Arguments
    ///
    /// * `db_name` - Name of the IndexedDB database
    /// * `store_name` - Name of the object store (like a file name)
    pub async fn open(db_name: &str, store_name: &str) -> WasmResult<Self> {
        if !Self::is_available() {
            return Err(WasmError::NotSupported("IndexedDB not available".into()));
        }

        // Open/create the database
        let db = Self::open_database(db_name, store_name).await?;
        
        // Load existing data
        let data = Self::load_data(&db, store_name).await?;
        
        // Close the database connection (we'll reopen on save)
        db.close();

        Ok(Self {
            db_name: db_name.to_string(),
            store_name: store_name.to_string(),
            data: Rc::new(RefCell::new(data)),
            dirty: Rc::new(RefCell::new(false)),
        })
    }

    /// Opens the IndexedDB database, creating object stores if needed.
    async fn open_database(
        db_name: &str,
        store_name: &str,
    ) -> WasmResult<web_sys::IdbDatabase> {
        let window = web_sys::window()
            .ok_or_else(|| WasmError::NotSupported("No window object".into()))?;
        
        let idb_factory = window
            .indexed_db()
            .map_err(|e| WasmError::Storage(format!("IndexedDB access error: {:?}", e)))?
            .ok_or_else(|| WasmError::NotSupported("IndexedDB not available".into()))?;

        // Use a combined database name to namespace stores
        let full_db_name = format!("entidb_{}", db_name);

        // Create a channel to receive the result
        let (tx, rx) = oneshot::channel::<Result<web_sys::IdbDatabase, WasmError>>();
        let tx = Rc::new(RefCell::new(Some(tx)));

        // Open request - we always use version 1 and create stores on upgrade
        let open_request = idb_factory
            .open_with_u32(&full_db_name, 1)
            .map_err(|e| WasmError::Storage(format!("Failed to open IndexedDB: {:?}", e)))?;

        // Handle upgrade needed (creates object stores)
        let store_name_clone = store_name.to_string();
        let upgrade_closure = Closure::once(move |event: web_sys::IdbVersionChangeEvent| {
            let target = event.target().expect("upgrade event target");
            let request: web_sys::IdbOpenDbRequest = target.unchecked_into();
            let db = request.result().expect("upgrade result");
            let db: web_sys::IdbDatabase = db.unchecked_into();

            // Create object store if it doesn't exist
            if !db.object_store_names().contains(&store_name_clone) {
                let _ = db.create_object_store(&store_name_clone);
            }
        });
        open_request.set_onupgradeneeded(Some(upgrade_closure.as_ref().unchecked_ref()));
        upgrade_closure.forget();

        // Handle success
        let tx_success = tx.clone();
        let success_closure = Closure::once(move |event: web_sys::Event| {
            let target = event.target().expect("success event target");
            let request: web_sys::IdbOpenDbRequest = target.unchecked_into();
            let db = request.result().expect("success result");
            let db: web_sys::IdbDatabase = db.unchecked_into();
            
            if let Some(sender) = tx_success.borrow_mut().take() {
                let _ = sender.send(Ok(db));
            }
        });
        open_request.set_onsuccess(Some(success_closure.as_ref().unchecked_ref()));
        success_closure.forget();

        // Handle error
        let tx_error = tx;
        let error_closure = Closure::once(move |event: web_sys::Event| {
            let target = event.target().expect("error event target");
            let request: web_sys::IdbOpenDbRequest = target.unchecked_into();
            // error() returns Result<Option<DomException>, JsValue>
            let msg = match request.error() {
                Ok(Some(e)) => e.message(),
                _ => "Unknown IndexedDB error".to_string(),
            };
            
            if let Some(sender) = tx_error.borrow_mut().take() {
                let _ = sender.send(Err(WasmError::Storage(msg)));
            }
        });
        open_request.set_onerror(Some(error_closure.as_ref().unchecked_ref()));
        error_closure.forget();

        // Wait for result
        rx.await.map_err(|_| WasmError::Storage("Channel closed".into()))?
    }

    /// Loads data from IndexedDB.
    async fn load_data(db: &web_sys::IdbDatabase, store_name: &str) -> WasmResult<Vec<u8>> {
        let (tx, rx) = oneshot::channel::<Result<Vec<u8>, WasmError>>();
        let tx = Rc::new(RefCell::new(Some(tx)));

        // Start a readonly transaction
        let transaction = db
            .transaction_with_str(store_name)
            .map_err(|e| WasmError::Storage(format!("Transaction error: {:?}", e)))?;
        
        let store = transaction
            .object_store(store_name)
            .map_err(|e| WasmError::Storage(format!("Store error: {:?}", e)))?;

        // Get the data
        let request = store
            .get(&JsValue::from_str(DATA_KEY))
            .map_err(|e| WasmError::Storage(format!("Get error: {:?}", e)))?;

        // Handle success
        let tx_success = tx.clone();
        let success_closure = Closure::once(move |event: web_sys::Event| {
            let target = event.target().expect("success target");
            let request: web_sys::IdbRequest = target.unchecked_into();
            let result = request.result().ok();
            
            let data = if let Some(value) = result {
                if value.is_undefined() || value.is_null() {
                    Vec::new()
                } else {
                    // Result should be a Uint8Array
                    let array = js_sys::Uint8Array::new(&value);
                    array.to_vec()
                }
            } else {
                Vec::new()
            };
            
            if let Some(sender) = tx_success.borrow_mut().take() {
                let _ = sender.send(Ok(data));
            }
        });
        request.set_onsuccess(Some(success_closure.as_ref().unchecked_ref()));
        success_closure.forget();

        // Handle error
        let tx_error = tx;
        let error_closure = Closure::once(move |event: web_sys::Event| {
            let target = event.target().expect("error target");
            let request: web_sys::IdbRequest = target.unchecked_into();
            let msg = match request.error() {
                Ok(Some(e)) => e.message(),
                _ => "Unknown error".to_string(),
            };
            
            if let Some(sender) = tx_error.borrow_mut().take() {
                let _ = sender.send(Err(WasmError::Storage(msg)));
            }
        });
        request.set_onerror(Some(error_closure.as_ref().unchecked_ref()));
        error_closure.forget();

        rx.await.map_err(|_| WasmError::Storage("Channel closed".into()))?
    }

    /// Saves data to IndexedDB.
    async fn save_data(&self) -> WasmResult<()> {
        let db = Self::open_database(&self.db_name, &self.store_name).await?;
        
        let (tx, rx) = oneshot::channel::<Result<(), WasmError>>();
        let tx = Rc::new(RefCell::new(Some(tx)));

        // Start a readwrite transaction
        let transaction = db
            .transaction_with_str_and_mode(&self.store_name, web_sys::IdbTransactionMode::Readwrite)
            .map_err(|e| WasmError::Storage(format!("Transaction error: {:?}", e)))?;
        
        let store = transaction
            .object_store(&self.store_name)
            .map_err(|e| WasmError::Storage(format!("Store error: {:?}", e)))?;

        // Convert data to Uint8Array
        let data = self.data.borrow();
        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(&data);

        // Put the data
        let request = store
            .put_with_key(&array, &JsValue::from_str(DATA_KEY))
            .map_err(|e| WasmError::Storage(format!("Put error: {:?}", e)))?;

        // Handle success
        let tx_success = tx.clone();
        let success_closure = Closure::once(move |_event: web_sys::Event| {
            if let Some(sender) = tx_success.borrow_mut().take() {
                let _ = sender.send(Ok(()));
            }
        });
        request.set_onsuccess(Some(success_closure.as_ref().unchecked_ref()));
        success_closure.forget();

        // Handle error
        let tx_error = tx;
        let error_closure = Closure::once(move |event: web_sys::Event| {
            let target = event.target().expect("error target");
            let request: web_sys::IdbRequest = target.unchecked_into();
            let msg = match request.error() {
                Ok(Some(e)) => e.message(),
                _ => "Unknown error".to_string(),
            };
            
            if let Some(sender) = tx_error.borrow_mut().take() {
                let _ = sender.send(Err(WasmError::Storage(msg)));
            }
        });
        request.set_onerror(Some(error_closure.as_ref().unchecked_ref()));
        error_closure.forget();

        let result = rx.await.map_err(|_| WasmError::Storage("Channel closed".into()))?;
        
        // Close the database
        db.close();
        
        result
    }

    /// Checks if IndexedDB is available.
    pub fn is_available() -> bool {
        if let Some(window) = web_sys::window() {
            window.indexed_db().ok().flatten().is_some()
        } else {
            false
        }
    }

    /// Returns the database name.
    pub fn db_name(&self) -> &str {
        &self.db_name
    }

    /// Returns the store name.
    pub fn store_name(&self) -> &str {
        &self.store_name
    }

    /// Reads data from a specific offset.
    pub async fn read_at_async(&self, offset: u64, len: usize) -> WasmResult<Vec<u8>> {
        let data = self.data.borrow();
        let offset = offset as usize;

        if offset > data.len() {
            return Err(WasmError::Storage(format!(
                "Offset {} beyond size {}",
                offset,
                data.len()
            )));
        }

        let end = (offset + len).min(data.len());
        Ok(data[offset..end].to_vec())
    }

    /// Appends data to the end of storage.
    pub async fn append_async(&self, bytes: &[u8]) -> WasmResult<u64> {
        let mut data = self.data.borrow_mut();
        let offset = data.len() as u64;
        data.extend_from_slice(bytes);
        *self.dirty.borrow_mut() = true;
        Ok(offset)
    }

    /// Overwrites all data in storage.
    pub async fn write_all(&self, bytes: &[u8]) -> WasmResult<()> {
        let mut data = self.data.borrow_mut();
        data.clear();
        data.extend_from_slice(bytes);
        *self.dirty.borrow_mut() = true;
        Ok(())
    }

    /// Returns the current size.
    pub fn size(&self) -> u64 {
        self.data.borrow().len() as u64
    }

    /// Flushes data to IndexedDB.
    pub async fn flush_async(&self) -> WasmResult<()> {
        if *self.dirty.borrow() {
            self.save_data().await?;
            *self.dirty.borrow_mut() = false;
        }
        Ok(())
    }

    /// Deletes the database.
    pub async fn delete(db_name: &str) -> WasmResult<()> {
        let window = web_sys::window()
            .ok_or_else(|| WasmError::NotSupported("No window object".into()))?;
        
        let idb_factory = window
            .indexed_db()
            .map_err(|e| WasmError::Storage(format!("IndexedDB access error: {:?}", e)))?
            .ok_or_else(|| WasmError::NotSupported("IndexedDB not available".into()))?;

        let full_db_name = format!("entidb_{}", db_name);

        let (tx, rx) = oneshot::channel::<Result<(), WasmError>>();
        let tx = Rc::new(RefCell::new(Some(tx)));

        let delete_request = idb_factory
            .delete_database(&full_db_name)
            .map_err(|e| WasmError::Storage(format!("Delete error: {:?}", e)))?;

        // Handle success
        let tx_success = tx.clone();
        let success_closure = Closure::once(move |_event: web_sys::Event| {
            if let Some(sender) = tx_success.borrow_mut().take() {
                let _ = sender.send(Ok(()));
            }
        });
        delete_request.set_onsuccess(Some(success_closure.as_ref().unchecked_ref()));
        success_closure.forget();

        // Handle error
        let tx_error = tx;
        let error_closure = Closure::once(move |event: web_sys::Event| {
            let target = event.target().expect("error target");
            let request: web_sys::IdbOpenDbRequest = target.unchecked_into();
            let msg = match request.error() {
                Ok(Some(e)) => e.message(),
                _ => "Unknown error".to_string(),
            };
            
            if let Some(sender) = tx_error.borrow_mut().take() {
                let _ = sender.send(Err(WasmError::Storage(msg)));
            }
        });
        delete_request.set_onerror(Some(error_closure.as_ref().unchecked_ref()));
        error_closure.forget();

        rx.await.map_err(|_| WasmError::Storage("Channel closed".into()))?
    }
}
