//! Transaction FFI functions.

use crate::buffer::EntiDbBuffer;
use crate::error::{clear_last_error, set_last_error, EntiDbResult};
use crate::types::{EntiDbCollectionId, EntiDbEntityId, EntiDbHandle, EntiDbTransaction};
use entidb_core::{CollectionId, Database, EntityId};
use std::collections::HashMap;

/// A wrapper holding transaction state for FFI.
pub struct FfiTransaction {
    /// Pending writes: (collection_id, entity_id) -> payload
    pub writes: HashMap<(u32, [u8; 16]), Option<Vec<u8>>>,
    /// Whether the transaction is still active.
    pub active: bool,
}

impl FfiTransaction {
    /// Creates a new transaction.
    pub fn new() -> Self {
        Self {
            writes: HashMap::new(),
            active: true,
        }
    }
}

/// Begins a new transaction.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_txn` - Output pointer for the transaction handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_txn` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_txn_begin(
    handle: *mut EntiDbHandle,
    out_txn: *mut *mut EntiDbTransaction,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_txn.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    // Verify database is valid by accessing it
    let _db = &*(handle as *mut Database);

    let txn = Box::new(FfiTransaction::new());
    *out_txn = Box::into_raw(txn) as *mut EntiDbTransaction;
    EntiDbResult::Ok
}

/// Commits a transaction.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `txn` - The transaction handle (consumed)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `txn` must be a valid transaction handle
/// - `txn` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn entidb_txn_commit(
    handle: *mut EntiDbHandle,
    txn: *mut EntiDbTransaction,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || txn.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut Database);
    let ffi_txn = Box::from_raw(txn as *mut FfiTransaction);

    if !ffi_txn.active {
        set_last_error("transaction is no longer active");
        return EntiDbResult::InvalidArgument;
    }

    // Apply all writes in a single transaction
    let result = db.transaction(|core_txn| {
        for ((coll_id, ent_id), payload) in ffi_txn.writes.iter() {
            let coll = CollectionId::new(*coll_id);
            let ent = EntityId::from_bytes(*ent_id);

            match payload {
                Some(data) => core_txn.put(coll, ent, data.clone())?,
                None => core_txn.delete(coll, ent)?,
            }
        }
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

/// Aborts a transaction.
///
/// # Arguments
///
/// * `txn` - The transaction handle (consumed)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `txn` must be a valid transaction handle
/// - `txn` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn entidb_txn_abort(txn: *mut EntiDbTransaction) -> EntiDbResult {
    clear_last_error();

    if txn.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    // Just drop the transaction
    let _ = Box::from_raw(txn as *mut FfiTransaction);
    EntiDbResult::Ok
}

/// Puts an entity within a transaction.
///
/// # Arguments
///
/// * `txn` - The transaction handle
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
/// - `txn` must be a valid transaction handle
/// - `data` must be valid for `data_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_txn_put(
    txn: *mut EntiDbTransaction,
    collection_id: EntiDbCollectionId,
    entity_id: EntiDbEntityId,
    data: *const u8,
    data_len: usize,
) -> EntiDbResult {
    clear_last_error();

    if txn.is_null() {
        set_last_error("null transaction handle");
        return EntiDbResult::NullPointer;
    }

    if data.is_null() && data_len > 0 {
        set_last_error("null data pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let ffi_txn = &mut *(txn as *mut FfiTransaction);

    if !ffi_txn.active {
        set_last_error("transaction is no longer active");
        return EntiDbResult::InvalidArgument;
    }

    let payload = if data_len > 0 {
        std::slice::from_raw_parts(data, data_len).to_vec()
    } else {
        Vec::new()
    };

    ffi_txn
        .writes
        .insert((collection_id.id, entity_id.bytes), Some(payload));

    EntiDbResult::Ok
}

/// Deletes an entity within a transaction.
///
/// # Arguments
///
/// * `txn` - The transaction handle
/// * `collection_id` - The collection ID
/// * `entity_id` - The entity ID
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// `txn` must be a valid transaction handle.
#[no_mangle]
pub unsafe extern "C" fn entidb_txn_delete(
    txn: *mut EntiDbTransaction,
    collection_id: EntiDbCollectionId,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if txn.is_null() {
        set_last_error("null transaction handle");
        return EntiDbResult::NullPointer;
    }

    let ffi_txn = &mut *(txn as *mut FfiTransaction);

    if !ffi_txn.active {
        set_last_error("transaction is no longer active");
        return EntiDbResult::InvalidArgument;
    }

    ffi_txn
        .writes
        .insert((collection_id.id, entity_id.bytes), None);

    EntiDbResult::Ok
}

/// Gets an entity within a transaction (sees uncommitted writes).
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `txn` - The transaction handle
/// * `collection_id` - The collection ID
/// * `entity_id` - The entity ID
/// * `out_buffer` - Output buffer for entity data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, `EntiDbResult::NotFound` if not found.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `txn` must be a valid transaction handle
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_txn_get(
    handle: *mut EntiDbHandle,
    txn: *mut EntiDbTransaction,
    collection_id: EntiDbCollectionId,
    entity_id: EntiDbEntityId,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || txn.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut Database);
    let ffi_txn = &*(txn as *mut FfiTransaction);

    let key = (collection_id.id, entity_id.bytes);

    // Check uncommitted writes first
    if let Some(payload) = ffi_txn.writes.get(&key) {
        match payload {
            Some(data) => {
                *out_buffer = EntiDbBuffer::from_vec(data.clone());
                return EntiDbResult::Ok;
            }
            None => {
                // Deleted in this transaction
                *out_buffer = EntiDbBuffer::empty();
                return EntiDbResult::NotFound;
            }
        }
    }

    // Not in transaction, check database
    let coll_id = CollectionId::new(collection_id.id);
    let ent_id = EntityId::from_bytes(entity_id.bytes);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::entidb_free_buffer;
    use crate::database::{
        entidb_close, entidb_collection, entidb_generate_id, entidb_open_memory,
    };

    #[test]
    fn transaction_put_commit() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            // Begin transaction
            let mut txn: *mut EntiDbTransaction = std::ptr::null_mut();
            let result = entidb_txn_begin(handle, &mut txn);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(!txn.is_null());

            // Put in transaction
            let data = b"txn data";
            let result = entidb_txn_put(txn, coll_id, entity_id, data.as_ptr(), data.len());
            assert_eq!(result, EntiDbResult::Ok);

            // Commit
            let result = entidb_txn_commit(handle, txn);
            assert_eq!(result, EntiDbResult::Ok);

            // Verify data is visible
            let mut buffer = EntiDbBuffer::empty();
            let result = crate::database::entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 8);

            entidb_free_buffer(buffer);
            entidb_close(handle);
        }
    }

    #[test]
    fn transaction_abort() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            // Begin transaction
            let mut txn: *mut EntiDbTransaction = std::ptr::null_mut();
            entidb_txn_begin(handle, &mut txn);

            // Put in transaction
            let data = b"txn data";
            entidb_txn_put(txn, coll_id, entity_id, data.as_ptr(), data.len());

            // Abort
            let result = entidb_txn_abort(txn);
            assert_eq!(result, EntiDbResult::Ok);

            // Verify data is NOT visible
            let mut buffer = EntiDbBuffer::empty();
            let result = crate::database::entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::NotFound);

            entidb_close(handle);
        }
    }

    #[test]
    fn transaction_get_sees_uncommitted() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            // Begin transaction
            let mut txn: *mut EntiDbTransaction = std::ptr::null_mut();
            entidb_txn_begin(handle, &mut txn);

            // Put in transaction
            let data = b"uncommitted";
            entidb_txn_put(txn, coll_id, entity_id, data.as_ptr(), data.len());

            // Get within transaction sees uncommitted data
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_txn_get(handle, txn, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 11);

            entidb_free_buffer(buffer);
            entidb_txn_abort(txn);
            entidb_close(handle);
        }
    }

    #[test]
    fn transaction_delete() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            // Put entity first
            let data = b"test";
            crate::database::entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Begin transaction
            let mut txn: *mut EntiDbTransaction = std::ptr::null_mut();
            entidb_txn_begin(handle, &mut txn);

            // Delete in transaction
            let result = entidb_txn_delete(txn, coll_id, entity_id);
            assert_eq!(result, EntiDbResult::Ok);

            // Get within transaction sees delete
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_txn_get(handle, txn, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::NotFound);

            // Commit
            entidb_txn_commit(handle, txn);

            // Verify delete is persisted
            let mut buffer = EntiDbBuffer::empty();
            let result = crate::database::entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::NotFound);

            entidb_close(handle);
        }
    }
}
