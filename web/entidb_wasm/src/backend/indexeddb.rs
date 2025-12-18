//! IndexedDB storage backend.
//!
//! This backend uses IndexedDB as a fallback when OPFS is not available.
//! IndexedDB is a key-value database built into browsers and is widely
//! supported.
//!
//! ## How It Works
//!
//! Since IndexedDB is key-value based, we simulate file-like storage by:
//! - Storing data in fixed-size chunks (blocks)
//! - Using numeric keys for block ordering
//! - Maintaining metadata about total size
//!
//! ## Implementation
//!
//! This implementation uses a single object store with:
//! - Key 0: metadata (total size)
//! - Key 1..N: data blocks
//!
//! ## Note
//!
//! This is a simplified implementation that stores data in localStorage
//! as a fallback. Full IndexedDB support requires more complex async handling.

#![allow(dead_code)]

use crate::error::{WasmError, WasmResult};
use std::cell::RefCell;
use std::rc::Rc;

/// Block size for chunking data in IndexedDB.
const BLOCK_SIZE: usize = 64 * 1024; // 64 KB blocks

/// IndexedDB-based storage backend.
///
/// This backend stores data in IndexedDB using a block-based approach.
/// It's a fallback for browsers that don't support OPFS.
///
/// ## Current Status
///
/// This is a simplified implementation using in-memory storage.
/// Data is persisted to localStorage as a base64-encoded string.
pub struct IndexedDbBackend {
    /// Database name.
    db_name: String,
    /// Store name.
    store_name: String,
    /// Cached data.
    data: Rc<RefCell<Vec<u8>>>,
    /// Whether cache is dirty.
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
            return Err(WasmError::NotSupported(
                "IndexedDB/localStorage not available".into(),
            ));
        }

        // Try to load from localStorage
        let key = format!("entidb_{}_{}", db_name, store_name);
        let data = Self::load_from_storage(&key)?;

        Ok(Self {
            db_name: db_name.to_string(),
            store_name: store_name.to_string(),
            data: Rc::new(RefCell::new(data)),
            dirty: Rc::new(RefCell::new(false)),
        })
    }

    /// Loads data from localStorage.
    fn load_from_storage(key: &str) -> WasmResult<Vec<u8>> {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                if let Ok(Some(value)) = storage.get_item(key) {
                    // Decode from base64
                    if let Ok(decoded) = Self::base64_decode(&value) {
                        return Ok(decoded);
                    }
                }
            }
        }
        Ok(Vec::new())
    }

    /// Saves data to localStorage.
    fn save_to_storage(&self) -> WasmResult<()> {
        let key = format!("entidb_{}_{}", self.db_name, self.store_name);
        let data = self.data.borrow();

        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let encoded = Self::base64_encode(&data);
                storage
                    .set_item(&key, &encoded)
                    .map_err(|_| WasmError::Storage("Failed to save to localStorage".into()))?;
            }
        }
        Ok(())
    }

    /// Simple base64 encode.
    fn base64_encode(data: &[u8]) -> String {
        use js_sys::Uint8Array;
        let array = Uint8Array::from(data);
        let blob_parts = js_sys::Array::new();
        blob_parts.push(&array);

        // Use btoa for simple encoding
        if let Some(window) = web_sys::window() {
            if let Ok(str_data) = String::from_utf8(data.to_vec()) {
                if let Ok(encoded) = window.btoa(&str_data) {
                    return encoded;
                }
            }
        }

        // Fallback: hex encoding
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Simple base64 decode.
    fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
        if let Some(window) = web_sys::window() {
            if let Ok(decoded) = window.atob(s) {
                return Ok(decoded.into_bytes());
            }
        }

        // Fallback: hex decoding
        let mut result = Vec::new();
        let chars: Vec<char> = s.chars().collect();
        for chunk in chars.chunks(2) {
            if chunk.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&chunk.iter().collect::<String>(), 16) {
                    result.push(byte);
                }
            }
        }
        Ok(result)
    }

    /// Checks if localStorage is available.
    pub fn is_available() -> bool {
        if let Some(window) = web_sys::window() {
            window.local_storage().ok().flatten().is_some()
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

    /// Returns the current size.
    pub fn size(&self) -> u64 {
        self.data.borrow().len() as u64
    }

    /// Flushes data to localStorage.
    pub async fn flush_async(&self) -> WasmResult<()> {
        if *self.dirty.borrow() {
            self.save_to_storage()?;
            *self.dirty.borrow_mut() = false;
        }
        Ok(())
    }

    /// Closes the database.
    pub fn close(&self) {
        // Save on close
        let _ = self.save_to_storage();
    }

    /// Deletes the database.
    pub async fn delete(db_name: &str) -> WasmResult<()> {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                // Remove all keys that start with this db_name
                let prefix = format!("entidb_{}_", db_name);
                let mut keys_to_remove = Vec::new();

                // Get length and iterate
                if let Ok(len) = storage.length() {
                    for i in 0..len {
                        if let Ok(Some(key)) = storage.key(i) {
                            if key.starts_with(&prefix) {
                                keys_to_remove.push(key);
                            }
                        }
                    }
                }

                // Remove the keys
                for key in keys_to_remove {
                    let _ = storage.remove_item(&key);
                }
            }
        }
        Ok(())
    }
}

impl Drop for IndexedDbBackend {
    fn drop(&mut self) {
        self.close();
    }
}
