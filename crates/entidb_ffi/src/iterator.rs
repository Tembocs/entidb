//! Iterator FFI functions.

use crate::buffer::EntiDbBuffer;
use crate::error::{clear_last_error, set_last_error, EntiDbResult};
use crate::types::{EntiDbCollectionId, EntiDbEntityId, EntiDbHandle};
use entidb_core::{CollectionId, Database, EntityId};

/// An opaque iterator handle.
#[repr(C)]
pub struct EntiDbIterator {
    _private: [u8; 0],
}

/// Internal iterator state.
pub struct FfiIterator {
    /// Collected entities to iterate over.
    entities: Vec<(EntityId, Vec<u8>)>,
    /// Current position.
    position: usize,
}

impl FfiIterator {
    /// Creates a new iterator from entities.
    pub fn new(entities: Vec<(EntityId, Vec<u8>)>) -> Self {
        Self {
            entities,
            position: 0,
        }
    }

    /// Returns true if there are more items.
    pub fn has_next(&self) -> bool {
        self.position < self.entities.len()
    }

    /// Gets the next item.
    pub fn next(&mut self) -> Option<&(EntityId, Vec<u8>)> {
        if self.position < self.entities.len() {
            let item = &self.entities[self.position];
            self.position += 1;
            Some(item)
        } else {
            None
        }
    }

    /// Returns the number of remaining items.
    pub fn remaining(&self) -> usize {
        self.entities.len().saturating_sub(self.position)
    }
}

/// Creates an iterator over all entities in a collection.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `out_iter` - Output pointer for the iterator handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_iter` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_iter_create(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    out_iter: *mut *mut EntiDbIterator,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_iter.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut Database);
    let coll_id = CollectionId::new(collection_id.id);

    // List all entities in the collection
    match db.list(coll_id) {
        Ok(entities) => {
            let iter = Box::new(FfiIterator::new(entities));
            *out_iter = Box::into_raw(iter) as *mut EntiDbIterator;
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Checks if the iterator has more items.
///
/// # Arguments
///
/// * `iter` - The iterator handle
/// * `out_has_next` - Output for whether there are more items
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `iter` must be a valid iterator handle
/// - `out_has_next` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_iter_has_next(
    iter: *mut EntiDbIterator,
    out_has_next: *mut bool,
) -> EntiDbResult {
    if iter.is_null() || out_has_next.is_null() {
        return EntiDbResult::NullPointer;
    }

    let ffi_iter = &*(iter as *mut FfiIterator);
    *out_has_next = ffi_iter.has_next();
    EntiDbResult::Ok
}

/// Gets the next item from the iterator.
///
/// # Arguments
///
/// * `iter` - The iterator handle
/// * `out_entity_id` - Output for the entity ID
/// * `out_buffer` - Output buffer for entity data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, `EntiDbResult::NotFound` if no more items.
///
/// # Safety
///
/// - `iter` must be a valid iterator handle
/// - `out_entity_id` must be a valid pointer
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_iter_next(
    iter: *mut EntiDbIterator,
    out_entity_id: *mut EntiDbEntityId,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if iter.is_null() || out_entity_id.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let ffi_iter = &mut *(iter as *mut FfiIterator);

    match ffi_iter.next() {
        Some((id, data)) => {
            *out_entity_id = EntiDbEntityId::from_bytes(*id.as_bytes());
            *out_buffer = EntiDbBuffer::from_vec(data.clone());
            EntiDbResult::Ok
        }
        None => {
            *out_entity_id = EntiDbEntityId::zero();
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::NotFound
        }
    }
}

/// Returns the number of remaining items in the iterator.
///
/// # Arguments
///
/// * `iter` - The iterator handle
/// * `out_count` - Output for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `iter` must be a valid iterator handle
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_iter_remaining(
    iter: *mut EntiDbIterator,
    out_count: *mut usize,
) -> EntiDbResult {
    if iter.is_null() || out_count.is_null() {
        return EntiDbResult::NullPointer;
    }

    let ffi_iter = &*(iter as *mut FfiIterator);
    *out_count = ffi_iter.remaining();
    EntiDbResult::Ok
}

/// Frees an iterator.
///
/// # Arguments
///
/// * `iter` - The iterator handle (consumed)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success.
///
/// # Safety
///
/// `iter` must be a valid iterator handle returned by `entidb_iter_create`.
#[no_mangle]
pub unsafe extern "C" fn entidb_iter_free(iter: *mut EntiDbIterator) -> EntiDbResult {
    if iter.is_null() {
        return EntiDbResult::NullPointer;
    }

    let _ = Box::from_raw(iter as *mut FfiIterator);
    EntiDbResult::Ok
}

/// Gets the count of entities in a collection.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `out_count` - Output for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_count(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut Database);
    let coll_id = CollectionId::new(collection_id.id);

    // Use list to count entities in the collection
    match db.list(coll_id) {
        Ok(entities) => {
            *out_count = entities.len();
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::entidb_free_buffer;
    use crate::database::{
        entidb_close, entidb_collection, entidb_generate_id, entidb_open_memory, entidb_put,
    };

    #[test]
    fn iterator_empty() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Create iterator on empty collection
            let mut iter: *mut EntiDbIterator = std::ptr::null_mut();
            let result = entidb_iter_create(handle, coll_id, &mut iter);
            assert_eq!(result, EntiDbResult::Ok);

            // Check has_next
            let mut has_next = true;
            entidb_iter_has_next(iter, &mut has_next);
            assert!(!has_next);

            // Check remaining
            let mut remaining = 999;
            entidb_iter_remaining(iter, &mut remaining);
            assert_eq!(remaining, 0);

            entidb_iter_free(iter);
            entidb_close(handle);
        }
    }

    #[test]
    fn iterator_with_data() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Add some entities
            for i in 0..3 {
                let mut entity_id = EntiDbEntityId::zero();
                entidb_generate_id(&mut entity_id);

                let data = format!("data-{i}");
                entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());
            }

            // Create iterator
            let mut iter: *mut EntiDbIterator = std::ptr::null_mut();
            let result = entidb_iter_create(handle, coll_id, &mut iter);
            assert_eq!(result, EntiDbResult::Ok);

            // Check remaining
            let mut remaining = 0;
            entidb_iter_remaining(iter, &mut remaining);
            assert_eq!(remaining, 3);

            // Iterate
            let mut count = 0;
            loop {
                let mut has_next = false;
                entidb_iter_has_next(iter, &mut has_next);
                if !has_next {
                    break;
                }

                let mut entity_id = EntiDbEntityId::zero();
                let mut buffer = EntiDbBuffer::empty();
                let result = entidb_iter_next(iter, &mut entity_id, &mut buffer);
                assert_eq!(result, EntiDbResult::Ok);
                assert!(!buffer.is_null());

                entidb_free_buffer(buffer);
                count += 1;
            }

            assert_eq!(count, 3);

            entidb_iter_free(iter);
            entidb_close(handle);
        }
    }

    #[test]
    fn count_entities() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Initially empty
            let mut count = 999;
            let result = entidb_count(handle, coll_id, &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 0);

            // Add entities
            for _ in 0..5 {
                let mut entity_id = EntiDbEntityId::zero();
                entidb_generate_id(&mut entity_id);

                let data = b"test";
                entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());
            }

            // Count should be 5
            let result = entidb_count(handle, coll_id, &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 5);

            entidb_close(handle);
        }
    }
}
