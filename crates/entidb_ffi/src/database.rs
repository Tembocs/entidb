//! Database FFI functions.

use crate::buffer::EntiDbBuffer;
use crate::error::{clear_last_error, set_last_error, EntiDbResult};
use crate::types::{EntiDbCollectionId, EntiDbConfig, EntiDbEntityId, EntiDbHandle};
use entidb_storage::FileBackend;
use std::ffi::CStr;
use std::path::Path;

/// Opens a database.
///
/// # Arguments
///
/// * `config` - Configuration for the database
/// * `out_handle` - Output pointer for the database handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `config.path` must be a valid null-terminated UTF-8 string or null
/// - `out_handle` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_open(
    config: *const EntiDbConfig,
    out_handle: *mut *mut EntiDbHandle,
) -> EntiDbResult {
    clear_last_error();

    if config.is_null() || out_handle.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let config = &*config;

    // Check if path is provided for file-based database
    if !config.path.is_null() {
        let path_cstr = CStr::from_ptr(config.path);
        let path_str = match path_cstr.to_str() {
            Ok(s) => s,
            Err(_) => {
                set_last_error("invalid UTF-8 in path");
                return EntiDbResult::InvalidArgument;
            }
        };

        let path = Path::new(path_str);

        // Create directory structure if needed
        if config.create_if_missing {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        set_last_error(format!("failed to create directory: {e}"));
                        return EntiDbResult::Error;
                    }
                }
            }
        }

        // Open file backends for WAL and segments
        let wal_path = path.join("wal.log");
        let segment_path = path.join("segments.dat");

        let wal_backend = if config.create_if_missing {
            FileBackend::open_with_create_dirs(&wal_path)
        } else {
            FileBackend::open(&wal_path)
        };

        let wal_backend = match wal_backend {
            Ok(b) => Box::new(b) as Box<dyn entidb_storage::StorageBackend>,
            Err(e) => {
                set_last_error(format!("failed to open WAL: {e}"));
                return EntiDbResult::Error;
            }
        };

        let segment_backend = if config.create_if_missing {
            FileBackend::open_with_create_dirs(&segment_path)
        } else {
            FileBackend::open(&segment_path)
        };

        let segment_backend = match segment_backend {
            Ok(b) => Box::new(b) as Box<dyn entidb_storage::StorageBackend>,
            Err(e) => {
                set_last_error(format!("failed to open segments: {e}"));
                return EntiDbResult::Error;
            }
        };

        // Build core config
        let mut core_config = entidb_core::Config::default();
        if config.max_segment_size > 0 {
            core_config.max_segment_size = config.max_segment_size;
        }
        core_config.sync_on_commit = config.sync_on_commit;

        // Open database with file backends
        match entidb_core::Database::open_with_backends(core_config, wal_backend, segment_backend) {
            Ok(db) => {
                let boxed = Box::new(db);
                *out_handle = Box::into_raw(boxed) as *mut EntiDbHandle;
                EntiDbResult::Ok
            }
            Err(e) => {
                set_last_error(e.to_string());
                EntiDbResult::Error
            }
        }
    } else {
        // Create in-memory database
        match entidb_core::Database::open_in_memory() {
            Ok(db) => {
                let boxed = Box::new(db);
                *out_handle = Box::into_raw(boxed) as *mut EntiDbHandle;
                EntiDbResult::Ok
            }
            Err(e) => {
                set_last_error(e.to_string());
                EntiDbResult::Error
            }
        }
    }
}

/// Opens an in-memory database.
///
/// # Arguments
///
/// * `out_handle` - Output pointer for the database handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// `out_handle` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn entidb_open_memory(out_handle: *mut *mut EntiDbHandle) -> EntiDbResult {
    clear_last_error();

    if out_handle.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    match entidb_core::Database::open_in_memory() {
        Ok(db) => {
            let boxed = Box::new(db);
            *out_handle = Box::into_raw(boxed) as *mut EntiDbHandle;
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Closes a database.
///
/// # Arguments
///
/// * `handle` - The database handle to close
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// The handle must have been returned by `entidb_open` or `entidb_open_memory`.
/// The handle must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn entidb_close(handle: *mut EntiDbHandle) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    // Take ownership and drop
    let db = Box::from_raw(handle as *mut entidb_core::Database);
    match db.close() {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Gets or creates a collection by name.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `name` - Null-terminated collection name
/// * `out_collection_id` - Output pointer for the collection ID
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `out_collection_id` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_collection(
    handle: *mut EntiDbHandle,
    name: *const std::ffi::c_char,
    out_collection_id: *mut EntiDbCollectionId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() || out_collection_id.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in collection name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let id = db.collection(name_str);
    *out_collection_id = EntiDbCollectionId::new(id.as_u32());
    EntiDbResult::Ok
}

/// Puts an entity in a collection.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `entity_id` - The entity ID
/// * `data` - Pointer to entity data
/// * `data_len` - Length of entity data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `data` must be valid for `data_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_put(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    entity_id: EntiDbEntityId,
    data: *const u8,
    data_len: usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() {
        set_last_error("null database handle");
        return EntiDbResult::NullPointer;
    }

    if data.is_null() && data_len > 0 {
        set_last_error("null data pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let payload = if data_len > 0 {
        std::slice::from_raw_parts(data, data_len).to_vec()
    } else {
        Vec::new()
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    let result = db.transaction(|txn| {
        txn.put(coll_id, ent_id, payload)?;
        Ok(())
    });

    match result {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Gets an entity from a collection.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `entity_id` - The entity ID
/// * `out_buffer` - Output buffer for entity data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success (buffer will be filled)
/// `EntiDbResult::NotFound` if entity doesn't exist
/// Error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_get(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    entity_id: EntiDbEntityId,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.get(coll_id, ent_id) {
        Ok(Some(data)) => {
            *out_buffer = EntiDbBuffer::from_vec(data);
            EntiDbResult::Ok
        }
        Ok(None) => {
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::NotFound
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::Error
        }
    }
}

/// Deletes an entity from a collection.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `entity_id` - The entity ID
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// `handle` must be a valid database handle.
#[no_mangle]
pub unsafe extern "C" fn entidb_delete(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() {
        set_last_error("null database handle");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    let result = db.transaction(|txn| {
        txn.delete(coll_id, ent_id)?;
        Ok(())
    });

    match result {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Generates a new entity ID.
///
/// # Arguments
///
/// * `out_id` - Output pointer for the new entity ID
///
/// # Returns
///
/// `EntiDbResult::Ok` on success.
///
/// # Safety
///
/// `out_id` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn entidb_generate_id(out_id: *mut EntiDbEntityId) -> EntiDbResult {
    if out_id.is_null() {
        return EntiDbResult::NullPointer;
    }

    let id = entidb_core::EntityId::new();
    *out_id = EntiDbEntityId::from_bytes(*id.as_bytes());
    EntiDbResult::Ok
}

/// Returns the library version as a null-terminated string.
///
/// The returned pointer is static and should not be freed.
#[no_mangle]
pub extern "C" fn entidb_version() -> *const std::ffi::c_char {
    // Static version string
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const std::ffi::c_char
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::entidb_free_buffer;

    #[test]
    fn open_memory_and_close() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        let result = unsafe { entidb_open_memory(&mut handle) };
        assert_eq!(result, EntiDbResult::Ok);
        assert!(!handle.is_null());

        let result = unsafe { entidb_close(handle) };
        assert_eq!(result, EntiDbResult::Ok);
    }

    #[test]
    fn put_and_get() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            // Open
            let result = entidb_open_memory(&mut handle);
            assert_eq!(result, EntiDbResult::Ok);

            // Get collection
            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            let result = entidb_collection(handle, name.as_ptr(), &mut coll_id);
            assert_eq!(result, EntiDbResult::Ok);

            // Generate ID
            let mut entity_id = EntiDbEntityId::zero();
            let result = entidb_generate_id(&mut entity_id);
            assert_eq!(result, EntiDbResult::Ok);

            // Put
            let data = b"hello world";
            let result = entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());
            assert_eq!(result, EntiDbResult::Ok);

            // Get
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(!buffer.is_null());
            assert_eq!(buffer.len, 11);

            // Verify data
            let slice = std::slice::from_raw_parts(buffer.data, buffer.len);
            assert_eq!(slice, b"hello world");

            // Free buffer
            entidb_free_buffer(buffer);

            // Close
            entidb_close(handle);
        }
    }

    #[test]
    fn get_not_found() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let entity_id = EntiDbEntityId::from_bytes([1u8; 16]);
            let mut buffer = EntiDbBuffer::empty();

            let result = entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::NotFound);
            assert!(buffer.is_null());

            entidb_close(handle);
        }
    }

    #[test]
    fn delete_entity() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            // Put
            let data = b"test";
            entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Delete
            let result = entidb_delete(handle, coll_id, entity_id);
            assert_eq!(result, EntiDbResult::Ok);

            // Get should return not found
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::NotFound);

            entidb_close(handle);
        }
    }

    #[test]
    fn version() {
        let ver = entidb_version();
        assert!(!ver.is_null());

        let s = unsafe { std::ffi::CStr::from_ptr(ver) };
        assert_eq!(s.to_str().unwrap(), "0.1.0");
    }

    #[test]
    fn null_pointer_handling() {
        let result = unsafe { entidb_open_memory(std::ptr::null_mut()) };
        assert_eq!(result, EntiDbResult::NullPointer);

        let result = unsafe { entidb_close(std::ptr::null_mut()) };
        assert_eq!(result, EntiDbResult::NullPointer);
    }
}
