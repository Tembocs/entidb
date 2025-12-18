//! OPFS (Origin Private File System) storage backend.
//!
//! This backend uses the modern Origin Private File System API for
//! high-performance file storage in web browsers.

#![allow(dead_code)]

use crate::error::{WasmError, WasmResult};
use js_sys::{ArrayBuffer, Object, Reflect, Uint8Array};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemWritableFileStream};

/// OPFS-based storage backend.
pub struct OpfsBackend {
    /// The root directory handle for the database.
    root_handle: FileSystemDirectoryHandle,
    /// The file handle for the data file.
    file_handle: FileSystemFileHandle,
    /// Database name.
    db_name: String,
    /// File name.
    file_name: String,
    /// Cached data for read operations.
    cache: Rc<RefCell<Vec<u8>>>,
    /// Whether cache is dirty.
    cache_dirty: Rc<RefCell<bool>>,
}

impl OpfsBackend {
    /// Opens or creates an OPFS storage backend.
    pub async fn open(db_name: &str, file_name: &str) -> WasmResult<Self> {
        if !Self::is_available() {
            return Err(WasmError::NotSupported("OPFS not available".into()));
        }

        let window = web_sys::window().ok_or_else(|| {
            WasmError::NotSupported("No window object".into())
        })?;
        let navigator = window.navigator();
        let storage = navigator.storage();

        // Get OPFS root directory using dynamic call
        let get_directory = Reflect::get(&storage, &JsValue::from_str("getDirectory"))
            .map_err(|_| WasmError::NotSupported("getDirectory not available".into()))?;
        let get_directory_fn: js_sys::Function = get_directory.dyn_into()
            .map_err(|_| WasmError::NotSupported("getDirectory is not a function".into()))?;
        
        let root_promise = get_directory_fn.call0(&storage)
            .map_err(|e| WasmError::Opfs(format!("Failed to call getDirectory: {:?}", e)))?;
        let root_handle: FileSystemDirectoryHandle = JsFuture::from(js_sys::Promise::from(root_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to get OPFS root: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast to directory handle".into()))?;

        // Create options object
        let create_opts = Object::new();
        Reflect::set(&create_opts, &JsValue::from_str("create"), &JsValue::TRUE).ok();

        // Get or create database directory using dynamic call
        let get_dir_handle = Reflect::get(&root_handle, &JsValue::from_str("getDirectoryHandle"))
            .map_err(|_| WasmError::Opfs("getDirectoryHandle not found".into()))?;
        let get_dir_fn: js_sys::Function = get_dir_handle.dyn_into()
            .map_err(|_| WasmError::Opfs("getDirectoryHandle is not a function".into()))?;
        
        let db_dir_promise = get_dir_fn.call2(&root_handle, &JsValue::from_str(db_name), &create_opts)
            .map_err(|e| WasmError::Opfs(format!("Failed to call getDirectoryHandle: {:?}", e)))?;
        let db_dir: FileSystemDirectoryHandle = JsFuture::from(js_sys::Promise::from(db_dir_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to get db directory: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast to directory handle".into()))?;

        // Get or create file using dynamic call
        let get_file_handle = Reflect::get(&db_dir, &JsValue::from_str("getFileHandle"))
            .map_err(|_| WasmError::Opfs("getFileHandle not found".into()))?;
        let get_file_fn: js_sys::Function = get_file_handle.dyn_into()
            .map_err(|_| WasmError::Opfs("getFileHandle is not a function".into()))?;
        
        let file_opts = Object::new();
        Reflect::set(&file_opts, &JsValue::from_str("create"), &JsValue::TRUE).ok();
        
        let file_promise = get_file_fn.call2(&db_dir, &JsValue::from_str(file_name), &file_opts)
            .map_err(|e| WasmError::Opfs(format!("Failed to call getFileHandle: {:?}", e)))?;
        let file_handle: FileSystemFileHandle = JsFuture::from(js_sys::Promise::from(file_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to get file: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast to file handle".into()))?;

        // Load existing data
        let cache = Self::load_file_content(&file_handle).await?;

        Ok(Self {
            root_handle: db_dir,
            file_handle,
            db_name: db_name.to_string(),
            file_name: file_name.to_string(),
            cache: Rc::new(RefCell::new(cache)),
            cache_dirty: Rc::new(RefCell::new(false)),
        })
    }

    /// Loads file content into memory.
    async fn load_file_content(file_handle: &FileSystemFileHandle) -> WasmResult<Vec<u8>> {
        let file_promise = file_handle.get_file();
        let file: web_sys::File = JsFuture::from(file_promise)
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to get file: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast to File".into()))?;

        let array_buffer_promise = file.array_buffer();
        let array_buffer: ArrayBuffer = JsFuture::from(array_buffer_promise)
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to read: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast to ArrayBuffer".into()))?;

        let uint8_array = Uint8Array::new(&array_buffer);
        Ok(uint8_array.to_vec())
    }

    /// Checks if OPFS is available.
    pub fn is_available() -> bool {
        if let Some(window) = web_sys::window() {
            let navigator = window.navigator();
            let storage = navigator.storage();
            Reflect::has(&storage, &JsValue::from_str("getDirectory")).unwrap_or(false)
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
        let cache = self.cache.borrow();
        let offset = offset as usize;

        if offset > cache.len() {
            return Err(WasmError::Storage(format!(
                "Offset {} beyond size {}",
                offset,
                cache.len()
            )));
        }

        let end = (offset + len).min(cache.len());
        Ok(cache[offset..end].to_vec())
    }

    /// Appends data to the end of storage.
    pub async fn append_async(&self, bytes: &[u8]) -> WasmResult<u64> {
        let mut cache = self.cache.borrow_mut();
        let offset = cache.len() as u64;
        cache.extend_from_slice(bytes);
        *self.cache_dirty.borrow_mut() = true;
        Ok(offset)
    }

    /// Writes all data, replacing existing content.
    pub async fn write_all(&self, bytes: &[u8]) -> WasmResult<()> {
        let mut cache = self.cache.borrow_mut();
        cache.clear();
        cache.extend_from_slice(bytes);
        *self.cache_dirty.borrow_mut() = true;
        Ok(())
    }

    /// Returns the current size.
    pub fn size(&self) -> u64 {
        self.cache.borrow().len() as u64
    }

    /// Flushes data to OPFS.
    pub async fn flush_async(&self) -> WasmResult<()> {
        if !*self.cache_dirty.borrow() {
            return Ok(());
        }

        // Create writable stream using dynamic call
        let create_writable = Reflect::get(&self.file_handle, &JsValue::from_str("createWritable"))
            .map_err(|_| WasmError::Opfs("createWritable not found".into()))?;
        let create_writable_fn: js_sys::Function = create_writable.dyn_into()
            .map_err(|_| WasmError::Opfs("createWritable is not a function".into()))?;
        
        let writable_promise = create_writable_fn.call0(&self.file_handle)
            .map_err(|e| WasmError::Opfs(format!("Failed to call createWritable: {:?}", e)))?;
        let writable: FileSystemWritableFileStream = JsFuture::from(js_sys::Promise::from(writable_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to create writable: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast to writable stream".into()))?;

        // Write data using dynamic call
        let cache = self.cache.borrow();
        let uint8_array = Uint8Array::from(cache.as_slice());
        
        let write_method = Reflect::get(&writable, &JsValue::from_str("write"))
            .map_err(|_| WasmError::Opfs("write not found".into()))?;
        let write_fn: js_sys::Function = write_method.dyn_into()
            .map_err(|_| WasmError::Opfs("write is not a function".into()))?;
        
        let write_promise = write_fn.call1(&writable, &uint8_array)
            .map_err(|e| WasmError::Opfs(format!("Failed to call write: {:?}", e)))?;
        JsFuture::from(js_sys::Promise::from(write_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to write: {:?}", e)))?;

        // Close the stream
        let close_method = Reflect::get(&writable, &JsValue::from_str("close"))
            .map_err(|_| WasmError::Opfs("close not found".into()))?;
        let close_fn: js_sys::Function = close_method.dyn_into()
            .map_err(|_| WasmError::Opfs("close is not a function".into()))?;
        
        let close_promise = close_fn.call0(&writable)
            .map_err(|e| WasmError::Opfs(format!("Failed to call close: {:?}", e)))?;
        JsFuture::from(js_sys::Promise::from(close_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to close: {:?}", e)))?;

        *self.cache_dirty.borrow_mut() = false;
        Ok(())
    }

    /// Deletes the database directory.
    pub async fn delete(db_name: &str) -> WasmResult<()> {
        if !Self::is_available() {
            return Err(WasmError::NotSupported("OPFS not available".into()));
        }

        let window = web_sys::window().ok_or_else(|| {
            WasmError::NotSupported("No window object".into())
        })?;
        let navigator = window.navigator();
        let storage = navigator.storage();

        // Get OPFS root
        let get_directory = Reflect::get(&storage, &JsValue::from_str("getDirectory"))
            .map_err(|_| WasmError::NotSupported("getDirectory not available".into()))?;
        let get_directory_fn: js_sys::Function = get_directory.dyn_into()
            .map_err(|_| WasmError::NotSupported("getDirectory is not a function".into()))?;
        
        let root_promise = get_directory_fn.call0(&storage)
            .map_err(|e| WasmError::Opfs(format!("Failed to call getDirectory: {:?}", e)))?;
        let root_handle: FileSystemDirectoryHandle = JsFuture::from(js_sys::Promise::from(root_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to get root: {:?}", e)))?
            .dyn_into()
            .map_err(|_| WasmError::Opfs("Failed to cast".into()))?;

        // Remove entry with recursive option using dynamic call
        let remove_entry = Reflect::get(&root_handle, &JsValue::from_str("removeEntry"))
            .map_err(|_| WasmError::Opfs("removeEntry not found".into()))?;
        let remove_fn: js_sys::Function = remove_entry.dyn_into()
            .map_err(|_| WasmError::Opfs("removeEntry is not a function".into()))?;
        
        let remove_opts = Object::new();
        Reflect::set(&remove_opts, &JsValue::from_str("recursive"), &JsValue::TRUE).ok();
        
        let remove_promise = remove_fn.call2(&root_handle, &JsValue::from_str(db_name), &remove_opts)
            .map_err(|e| WasmError::Opfs(format!("Failed to call removeEntry: {:?}", e)))?;
        JsFuture::from(js_sys::Promise::from(remove_promise))
            .await
            .map_err(|e| WasmError::Opfs(format!("Failed to remove: {:?}", e)))?;

        Ok(())
    }
}
