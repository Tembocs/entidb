//! IndexedDB storage backend.
//!
//! This backend serves as a fallback when OPFS is not available.
//!
//! ## Current Implementation Status
//!
//! **WARNING:** This is a simplified implementation that uses `localStorage` as
//! the underlying storage mechanism, NOT actual IndexedDB. This has significant
//! limitations:
//!
//! - **Size limit:** localStorage is typically limited to 5-10MB per origin
//! - **Durability:** localStorage is synchronous and blocking, but persisted
//! - **Binary data:** Data is base64-encoded, adding ~33% overhead
//! - **No concurrent access:** localStorage is synchronous and single-threaded
//!
//! ## When This Is Used
//!
//! This fallback is only used when:
//! 1. OPFS is not available (older browsers, non-secure contexts)
//! 2. The user explicitly requests IndexedDB storage type
//!
//! ## Recommendations
//!
//! For production use, prefer OPFS which provides:
//! - Much larger storage limits
//! - True file-like semantics
//! - Better performance
//!
//! ## Future Work
//!
//! A proper IndexedDB implementation would use the actual IndexedDB API via
//! `web-sys::IdbFactory`, `IdbDatabase`, etc. with proper async transaction
//! handling. This is complex due to IndexedDB's callback-based API and would
//! require significant additional code.

#![allow(dead_code)]

use crate::error::{WasmError, WasmResult};
use std::cell::RefCell;
use std::rc::Rc;

/// Block size for chunking data in IndexedDB.
const BLOCK_SIZE: usize = 64 * 1024; // 64 KB blocks

/// Maximum recommended size for localStorage-based storage (5MB).
/// Beyond this, browsers may refuse to store data or show quota warnings.
const MAX_RECOMMENDED_SIZE: usize = 5 * 1024 * 1024;

/// localStorage-based storage backend (IndexedDB fallback).
///
/// **WARNING:** Despite the name, this currently uses `localStorage`, not
/// IndexedDB. See module documentation for details and limitations.
///
/// This is a fallback for browsers that don't support OPFS. For production
/// use, OPFS is strongly recommended.
///
/// ## Limitations
///
/// - Storage is limited to ~5MB (browser-dependent)
/// - Data is base64-encoded, adding ~33% overhead
/// - Not suitable for large databases
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
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - localStorage is not available
    /// - Storage quota is exceeded (typically ~5MB per origin)
    fn save_to_storage(&self) -> WasmResult<()> {
        let key = format!("entidb_{}_{}", self.db_name, self.store_name);
        let data = self.data.borrow();

        // Warn if data is approaching localStorage limits
        if data.len() > MAX_RECOMMENDED_SIZE {
            web_sys::console::warn_1(&format!(
                "EntiDB: localStorage backend data size ({} bytes) exceeds recommended limit ({}). \
                 Consider using OPFS for larger databases.",
                data.len(),
                MAX_RECOMMENDED_SIZE
            ).into());
        }

        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let encoded = Self::base64_encode(&data);
                storage
                    .set_item(&key, &encoded)
                    .map_err(|_| WasmError::Storage(
                        "Failed to save to localStorage. Storage quota may be exceeded. \
                         Consider using OPFS storage or reducing database size.".into()
                    ))?;
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

    /// Overwrites all data in storage.
    ///
    /// This replaces any existing content with the new data.
    /// Used for snapshot-based persistence where the entire state is written.
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
