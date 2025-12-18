//! OPFS (Origin Private File System) storage backend.
//!
//! This backend uses the modern Origin Private File System API available
//! in browsers. OPFS provides file-like storage with better performance
//! than IndexedDB, especially for sequential access patterns.
//!
//! ## Requirements
//!
//! - Modern browser with OPFS support (Chrome 86+, Firefox 111+, Safari 15.2+)
//! - For synchronous access (best performance), must run in a Web Worker
//!
//! ## Note
//!
//! This is a placeholder implementation. Full OPFS support requires
//! running in a Web Worker context with FileSystemSyncAccessHandle.
//! For now, we provide the structure and async API that can be used
//! once the full implementation is complete.

#![allow(dead_code)]

use crate::error::{WasmError, WasmResult};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsValue;
use web_sys::FileSystemDirectoryHandle;

/// OPFS-based storage backend.
///
/// This backend stores data in the Origin Private File System.
/// It's the preferred storage backend for web applications due to
/// its file-like semantics and better performance.
///
/// ## Current Status
///
/// This is a placeholder that provides the async API structure.
/// Full implementation requires Web Worker integration for
/// synchronous file access.
pub struct OpfsBackend {
    /// The directory handle for the database.
    _dir_handle: Option<FileSystemDirectoryHandle>,
    /// Database name.
    db_name: String,
    /// File name.
    file_name: String,
    /// Cached data (for now, we simulate with in-memory storage).
    data: Rc<RefCell<Vec<u8>>>,
}

impl OpfsBackend {
    /// Opens or creates an OPFS storage backend.
    ///
    /// # Arguments
    ///
    /// * `db_name` - Name of the database (used as directory name)
    /// * `file_name` - Name of the file within the database directory
    ///
    /// # Note
    ///
    /// Currently returns an in-memory simulation until full OPFS
    /// integration is implemented with Web Workers.
    pub async fn open(db_name: &str, file_name: &str) -> WasmResult<Self> {
        // Check if OPFS is available
        if !Self::is_available() {
            return Err(WasmError::NotSupported(
                "OPFS not available in this browser".into(),
            ));
        }

        // For now, we just create an in-memory simulation
        // Full OPFS implementation would get the directory handle here
        Ok(Self {
            _dir_handle: None,
            db_name: db_name.to_string(),
            file_name: file_name.to_string(),
            data: Rc::new(RefCell::new(Vec::new())),
        })
    }

    /// Checks if OPFS is available in the current browser.
    pub fn is_available() -> bool {
        if let Some(window) = web_sys::window() {
            let navigator = window.navigator();
            let storage = navigator.storage();
            js_sys::Reflect::has(&storage, &JsValue::from_str("getDirectory")).unwrap_or(false)
        } else {
            false
        }
    }

    /// Returns the database name.
    pub fn db_name(&self) -> &str {
        &self.db_name
    }

    /// Returns the file name.
    pub fn file_name(&self) -> &str {
        &self.file_name
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
        // In a real implementation, this would sync to OPFS
        Ok(())
    }
}
