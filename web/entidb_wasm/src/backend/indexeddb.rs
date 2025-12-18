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
//! ## Note
//!
//! This is a placeholder implementation that uses in-memory storage.
//! Full IndexedDB support requires async operations throughout.

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
/// This is a placeholder that provides the async API structure.
/// Currently uses in-memory storage for the actual data.
pub struct IndexedDbBackend {
    /// Database name.
    db_name: String,
    /// Store name.
    store_name: String,
    /// Cached data (for now, we simulate with in-memory storage).
    data: Rc<RefCell<Vec<u8>>>,
}

impl IndexedDbBackend {
    /// Opens or creates an IndexedDB storage backend.
    ///
    /// # Arguments
    ///
    /// * `db_name` - Name of the IndexedDB database
    /// * `store_name` - Name of the object store (like a file name)
    ///
    /// # Note
    ///
    /// Currently returns an in-memory simulation until full IndexedDB
    /// integration is implemented.
    pub async fn open(db_name: &str, store_name: &str) -> WasmResult<Self> {
        if !Self::is_available() {
            return Err(WasmError::NotSupported(
                "IndexedDB not available".into(),
            ));
        }

        Ok(Self {
            db_name: db_name.to_string(),
            store_name: store_name.to_string(),
            data: Rc::new(RefCell::new(Vec::new())),
        })
    }

    /// Checks if IndexedDB is available.
    ///
    /// This is a placeholder check. Full implementation would use
    /// the js_sys::Reflect API to check for indexedDB presence.
    pub fn is_available() -> bool {
        // Simplified check: if we have a window, assume IndexedDB is available
        // Most modern browsers support IndexedDB
        web_sys::window().is_some()
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

        if offset >= data.len() {
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
        Ok(offset)
    }

    /// Returns the current size.
    pub fn size(&self) -> u64 {
        self.data.borrow().len() as u64
    }

    /// Flushes data (no-op for simulation).
    pub async fn flush_async(&self) -> WasmResult<()> {
        // In a real implementation, this would sync to IndexedDB
        Ok(())
    }

    /// Closes the database.
    pub fn close(&self) {
        // In a real implementation, this would close the IDB connection
    }

    /// Deletes the database.
    pub async fn delete(_db_name: &str) -> WasmResult<()> {
        // In a real implementation, this would delete the IDB database
        Ok(())
    }
}

impl Drop for IndexedDbBackend {
    fn drop(&mut self) {
        self.close();
    }
}
