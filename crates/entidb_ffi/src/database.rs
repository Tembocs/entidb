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

// ============================================================================
// Checkpoint, Backup, and Restore
// ============================================================================

/// Creates a checkpoint.
///
/// A checkpoint persists all committed data and truncates the WAL.
///
/// # Arguments
///
/// * `handle` - The database handle
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// `handle` must be a valid database handle.
#[no_mangle]
pub unsafe extern "C" fn entidb_checkpoint(handle: *mut EntiDbHandle) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() {
        set_last_error("null database handle");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    match db.checkpoint() {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Creates a backup of the database.
///
/// Returns the backup data as a buffer that must be freed with `entidb_free_buffer`.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_buffer` - Output buffer for the backup data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_backup(
    handle: *mut EntiDbHandle,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    match db.backup() {
        Ok(data) => {
            *out_buffer = EntiDbBuffer::from_vec(data);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::Error
        }
    }
}

/// Creates a backup with custom options.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `include_tombstones` - Whether to include deleted entities
/// * `out_buffer` - Output buffer for the backup data
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_backup_with_options(
    handle: *mut EntiDbHandle,
    include_tombstones: bool,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    match db.backup_with_options(include_tombstones) {
        Ok(data) => {
            *out_buffer = EntiDbBuffer::from_vec(data);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::Error
        }
    }
}

/// Restore statistics returned by restore operations.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EntiDbRestoreStats {
    /// Number of entities restored.
    pub entities_restored: u64,
    /// Number of tombstones (deletions) applied.
    pub tombstones_applied: u64,
    /// Timestamp when the backup was created (Unix millis).
    pub backup_timestamp: u64,
    /// Sequence number at the time of backup.
    pub backup_sequence: u64,
}

impl EntiDbRestoreStats {
    /// Creates an empty stats struct.
    pub fn empty() -> Self {
        Self {
            entities_restored: 0,
            tombstones_applied: 0,
            backup_timestamp: 0,
            backup_sequence: 0,
        }
    }
}

/// Restores entities from a backup into the database.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `data` - Pointer to backup data
/// * `data_len` - Length of backup data
/// * `out_stats` - Output pointer for restore statistics
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `data` must be valid for `data_len` bytes
/// - `out_stats` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_restore(
    handle: *mut EntiDbHandle,
    data: *const u8,
    data_len: usize,
    out_stats: *mut EntiDbRestoreStats,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_stats.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if data.is_null() && data_len > 0 {
        set_last_error("null data pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let backup_data = if data_len > 0 {
        std::slice::from_raw_parts(data, data_len)
    } else {
        &[]
    };

    match db.restore(backup_data) {
        Ok(stats) => {
            *out_stats = EntiDbRestoreStats {
                entities_restored: stats.entities_restored,
                tombstones_applied: stats.tombstones_applied,
                backup_timestamp: stats.backup_timestamp,
                backup_sequence: stats.backup_sequence,
            };
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_stats = EntiDbRestoreStats::empty();
            EntiDbResult::Error
        }
    }
}

/// Backup information returned by validate_backup.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EntiDbBackupInfo {
    /// Whether the backup checksum is valid.
    pub valid: bool,
    /// Timestamp when the backup was created (Unix millis).
    pub timestamp: u64,
    /// Sequence number at the time of backup.
    pub sequence: u64,
    /// Number of records in the backup.
    pub record_count: u32,
    /// Size of the backup in bytes.
    pub size: usize,
}

impl EntiDbBackupInfo {
    /// Creates an empty info struct.
    pub fn empty() -> Self {
        Self {
            valid: false,
            timestamp: 0,
            sequence: 0,
            record_count: 0,
            size: 0,
        }
    }
}

/// Validates a backup without restoring it.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `data` - Pointer to backup data
/// * `data_len` - Length of backup data
/// * `out_info` - Output pointer for backup information
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `data` must be valid for `data_len` bytes
/// - `out_info` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_validate_backup(
    handle: *mut EntiDbHandle,
    data: *const u8,
    data_len: usize,
    out_info: *mut EntiDbBackupInfo,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_info.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if data.is_null() && data_len > 0 {
        set_last_error("null data pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let backup_data = if data_len > 0 {
        std::slice::from_raw_parts(data, data_len)
    } else {
        &[]
    };

    match db.validate_backup(backup_data) {
        Ok(info) => {
            *out_info = EntiDbBackupInfo {
                valid: info.valid,
                timestamp: info.timestamp,
                sequence: info.sequence,
                record_count: info.record_count,
                size: info.size,
            };
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_info = EntiDbBackupInfo::empty();
            EntiDbResult::Error
        }
    }
}

/// Returns the current committed sequence number.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_seq` - Output pointer for the sequence number
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_seq` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_committed_seq(
    handle: *mut EntiDbHandle,
    out_seq: *mut u64,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_seq.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    *out_seq = db.committed_seq().as_u64();
    EntiDbResult::Ok
}

/// Returns the total entity count.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_count` - Output pointer for the count
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
pub unsafe extern "C" fn entidb_entity_count(
    handle: *mut EntiDbHandle,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    *out_count = db.entity_count();
    EntiDbResult::Ok
}

// ============================================================================
// Index Management
// ============================================================================

/// Index type enumeration for FFI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntiDbIndexType {
    /// Hash index for O(1) equality lookups.
    Hash = 0,
    /// BTree index for ordered and range lookups.
    BTree = 1,
}

/// Creates a hash index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `unique` - Whether the index enforces unique constraint
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_create_hash_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    unique: bool,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    match db.create_hash_index(coll_id, name_str, unique) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Creates a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `unique` - Whether the index enforces unique constraint
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_create_btree_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    unique: bool,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    match db.create_btree_index(coll_id, name_str, unique) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Inserts a key-entity pair into a hash index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `key` - Pointer to key bytes
/// * `key_len` - Length of key bytes
/// * `entity_id` - The entity ID to associate with the key
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_insert(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len).to_vec()
    } else {
        Vec::new()
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.hash_index_insert(coll_id, name_str, key_bytes, ent_id) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Inserts a key-entity pair into a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `key` - Pointer to key bytes
/// * `key_len` - Length of key bytes
/// * `entity_id` - The entity ID to associate with the key
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_insert(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len).to_vec()
    } else {
        Vec::new()
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.btree_index_insert(coll_id, name_str, key_bytes, ent_id) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Removes a key-entity pair from a hash index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `key` - Pointer to key bytes
/// * `key_len` - Length of key bytes
/// * `entity_id` - The entity ID to remove
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_remove(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len)
    } else {
        &[]
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.hash_index_remove(coll_id, name_str, key_bytes, ent_id) {
        Ok(_) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Removes a key-entity pair from a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `key` - Pointer to key bytes
/// * `key_len` - Length of key bytes
/// * `entity_id` - The entity ID to remove
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_remove(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len)
    } else {
        &[]
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.btree_index_remove(coll_id, name_str, key_bytes, ent_id) {
        Ok(_) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Looks up entities by key in a hash index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `key` - Pointer to key bytes
/// * `key_len` - Length of key bytes
/// * `out_buffer` - Output buffer for entity IDs (16 bytes each)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_lookup(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len)
    } else {
        &[]
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.hash_index_lookup(coll_id, name_str, key_bytes) {
        Ok(entity_ids) => {
            // Serialize entity IDs as contiguous 16-byte blocks
            let mut result = Vec::with_capacity(entity_ids.len() * 16);
            for id in entity_ids {
                result.extend_from_slice(id.as_bytes());
            }
            *out_buffer = EntiDbBuffer::from_vec(result);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::Error
        }
    }
}

/// Looks up entities by key in a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `key` - Pointer to key bytes
/// * `key_len` - Length of key bytes
/// * `out_buffer` - Output buffer for entity IDs (16 bytes each)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_lookup(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len)
    } else {
        &[]
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.btree_index_lookup(coll_id, name_str, key_bytes) {
        Ok(entity_ids) => {
            // Serialize entity IDs as contiguous 16-byte blocks
            let mut result = Vec::with_capacity(entity_ids.len() * 16);
            for id in entity_ids {
                result.extend_from_slice(id.as_bytes());
            }
            *out_buffer = EntiDbBuffer::from_vec(result);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::Error
        }
    }
}

/// Performs a range query on a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `min_key` - Pointer to minimum key bytes (or null for unbounded)
/// * `min_key_len` - Length of minimum key bytes
/// * `max_key` - Pointer to maximum key bytes (or null for unbounded)
/// * `max_key_len` - Length of maximum key bytes
/// * `out_buffer` - Output buffer for entity IDs (16 bytes each)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `min_key` must be valid for `min_key_len` bytes if non-null
/// - `max_key` must be valid for `max_key_len` bytes if non-null
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_range(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    min_key: *const u8,
    min_key_len: usize,
    max_key: *const u8,
    max_key_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let min_key_bytes = if min_key.is_null() {
        None
    } else if min_key_len > 0 {
        Some(std::slice::from_raw_parts(min_key, min_key_len))
    } else {
        Some(&[] as &[u8])
    };

    let max_key_bytes = if max_key.is_null() {
        None
    } else if max_key_len > 0 {
        Some(std::slice::from_raw_parts(max_key, max_key_len))
    } else {
        Some(&[] as &[u8])
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.btree_index_range(coll_id, name_str, min_key_bytes, max_key_bytes) {
        Ok(entity_ids) => {
            // Serialize entity IDs as contiguous 16-byte blocks
            let mut result = Vec::with_capacity(entity_ids.len() * 16);
            for id in entity_ids {
                result.extend_from_slice(id.as_bytes());
            }
            *out_buffer = EntiDbBuffer::from_vec(result);
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            *out_buffer = EntiDbBuffer::empty();
            EntiDbResult::Error
        }
    }
}

/// Returns the number of entries in a hash index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `out_count` - Output pointer for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_len(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.hash_index_len(coll_id, name_str) {
        Ok(count) => {
            *out_count = count;
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Returns the number of entries in a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
/// * `out_count` - Output pointer for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_len(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.btree_index_len(coll_id, name_str) {
        Ok(count) => {
            *out_count = count;
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Drops a hash index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
///
/// # Returns
///
/// `EntiDbResult::Ok` on success (returns Ok even if index didn't exist).
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_drop_hash_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.drop_hash_index(coll_id, name_str) {
        Ok(_) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Drops a BTree index.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `name` - Null-terminated index name
///
/// # Returns
///
/// `EntiDbResult::Ok` on success (returns Ok even if index didn't exist).
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `name` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_drop_btree_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    name: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || name.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let name_cstr = CStr::from_ptr(name);
    let name_str = match name_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in index name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.drop_btree_index(coll_id, name_str) {
        Ok(_) => EntiDbResult::Ok,
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

    #[test]
    fn checkpoint() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            // Add some data
            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            let data = b"checkpoint test";
            entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Checkpoint
            let result = entidb_checkpoint(handle);
            assert_eq!(result, EntiDbResult::Ok);

            // Data should still be accessible
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            entidb_free_buffer(buffer);

            entidb_close(handle);
        }
    }

    #[test]
    fn backup_and_restore() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            // Add some data
            let name = std::ffi::CString::new("users").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            let data = b"backup test data";
            entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Create backup
            let mut backup_buffer = EntiDbBuffer::empty();
            let result = entidb_backup(handle, &mut backup_buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(!backup_buffer.is_null());
            assert!(backup_buffer.len > 0);

            // Create a new database and restore
            let mut handle2: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle2);

            // Ensure the collection exists in the new database
            let mut coll_id2 = EntiDbCollectionId::new(0);
            entidb_collection(handle2, name.as_ptr(), &mut coll_id2);

            let mut stats = EntiDbRestoreStats::empty();
            let result = entidb_restore(
                handle2,
                backup_buffer.data,
                backup_buffer.len,
                &mut stats,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(stats.entities_restored, 1);

            // Verify data exists in new database
            let mut get_buffer = EntiDbBuffer::empty();
            let result = entidb_get(handle2, coll_id2, entity_id, &mut get_buffer);
            assert_eq!(result, EntiDbResult::Ok);

            let slice = std::slice::from_raw_parts(get_buffer.data, get_buffer.len);
            assert_eq!(slice, b"backup test data");

            entidb_free_buffer(get_buffer);
            entidb_free_buffer(backup_buffer);
            entidb_close(handle);
            entidb_close(handle2);
        }
    }

    #[test]
    fn backup_with_options() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            let data = b"test data";
            entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Backup without tombstones
            let mut buffer1 = EntiDbBuffer::empty();
            let result = entidb_backup_with_options(handle, false, &mut buffer1);
            assert_eq!(result, EntiDbResult::Ok);

            // Backup with tombstones
            let mut buffer2 = EntiDbBuffer::empty();
            let result = entidb_backup_with_options(handle, true, &mut buffer2);
            assert_eq!(result, EntiDbResult::Ok);

            entidb_free_buffer(buffer1);
            entidb_free_buffer(buffer2);
            entidb_close(handle);
        }
    }

    #[test]
    fn validate_backup() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            let data = b"validation test";
            entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Create backup
            let mut backup_buffer = EntiDbBuffer::empty();
            entidb_backup(handle, &mut backup_buffer);

            // Validate
            let mut info = EntiDbBackupInfo::empty();
            let result = entidb_validate_backup(
                handle,
                backup_buffer.data,
                backup_buffer.len,
                &mut info,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert!(info.valid);
            assert!(info.record_count > 0);
            assert!(info.size > 0);

            entidb_free_buffer(backup_buffer);
            entidb_close(handle);
        }
    }

    #[test]
    fn committed_seq_and_entity_count() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            // Initial state
            let mut seq: u64 = 0;
            let result = entidb_committed_seq(handle, &mut seq);
            assert_eq!(result, EntiDbResult::Ok);

            let mut count: usize = 0;
            let result = entidb_entity_count(handle, &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 0);

            // Add data
            let name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            let data = b"test";
            entidb_put(handle, coll_id, entity_id, data.as_ptr(), data.len());

            // Verify count increased
            let result = entidb_entity_count(handle, &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 1);

            // Verify sequence increased
            let mut new_seq: u64 = 0;
            let result = entidb_committed_seq(handle, &mut new_seq);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(new_seq > seq);

            entidb_close(handle);
        }
    }

    #[test]
    fn hash_index_operations() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            // Get collection
            let name = std::ffi::CString::new("users").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Create hash index
            let index_name = std::ffi::CString::new("email").unwrap();
            let result = entidb_create_hash_index(handle, coll_id, index_name.as_ptr(), true);
            assert_eq!(result, EntiDbResult::Ok);

            // Generate entity ID
            let mut entity_id = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity_id);

            // Insert into index
            let key = b"alice@example.com";
            let result =
                entidb_hash_index_insert(handle, coll_id, index_name.as_ptr(), key.as_ptr(), key.len(), entity_id);
            assert_eq!(result, EntiDbResult::Ok);

            // Lookup
            let mut buffer = EntiDbBuffer::empty();
            let result =
                entidb_hash_index_lookup(handle, coll_id, index_name.as_ptr(), key.as_ptr(), key.len(), &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 16); // One entity ID

            // Check length
            let mut count: usize = 0;
            let result = entidb_hash_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 1);

            // Remove
            let result =
                entidb_hash_index_remove(handle, coll_id, index_name.as_ptr(), key.as_ptr(), key.len(), entity_id);
            assert_eq!(result, EntiDbResult::Ok);

            // Check length is 0
            let result = entidb_hash_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 0);

            // Drop index
            let result = entidb_drop_hash_index(handle, coll_id, index_name.as_ptr());
            assert_eq!(result, EntiDbResult::Ok);

            entidb_free_buffer(buffer);
            entidb_close(handle);
        }
    }

    #[test]
    fn btree_index_operations() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            // Get collection
            let name = std::ffi::CString::new("users").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Create btree index
            let index_name = std::ffi::CString::new("age").unwrap();
            let result = entidb_create_btree_index(handle, coll_id, index_name.as_ptr(), false);
            assert_eq!(result, EntiDbResult::Ok);

            // Generate entity IDs
            let mut e1 = EntiDbEntityId::zero();
            let mut e2 = EntiDbEntityId::zero();
            let mut e3 = EntiDbEntityId::zero();
            entidb_generate_id(&mut e1);
            entidb_generate_id(&mut e2);
            entidb_generate_id(&mut e3);

            // Insert into index (big-endian for proper ordering)
            let key1 = 25i64.to_be_bytes();
            let key2 = 30i64.to_be_bytes();
            let key3 = 35i64.to_be_bytes();

            entidb_btree_index_insert(handle, coll_id, index_name.as_ptr(), key1.as_ptr(), key1.len(), e1);
            entidb_btree_index_insert(handle, coll_id, index_name.as_ptr(), key2.as_ptr(), key2.len(), e2);
            entidb_btree_index_insert(handle, coll_id, index_name.as_ptr(), key3.as_ptr(), key3.len(), e3);

            // Lookup exact
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_btree_index_lookup(
                handle,
                coll_id,
                index_name.as_ptr(),
                key2.as_ptr(),
                key2.len(),
                &mut buffer,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 16); // One entity
            entidb_free_buffer(buffer);

            // Range query: 25 <= age <= 30
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_btree_index_range(
                handle,
                coll_id,
                index_name.as_ptr(),
                key1.as_ptr(),
                key1.len(),
                key2.as_ptr(),
                key2.len(),
                &mut buffer,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 32); // Two entities
            entidb_free_buffer(buffer);

            // Range query: unbounded (all)
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_btree_index_range(
                handle,
                coll_id,
                index_name.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                &mut buffer,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 48); // Three entities
            entidb_free_buffer(buffer);

            // Drop index
            let result = entidb_drop_btree_index(handle, coll_id, index_name.as_ptr());
            assert_eq!(result, EntiDbResult::Ok);

            entidb_close(handle);
        }
    }
}

// ============================================================================
// Observability (Stats)
// ============================================================================

/// Gets the current database statistics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_stats` - Output pointer for the stats structure
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_stats` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_stats(
    handle: *mut EntiDbHandle,
    out_stats: *mut crate::types::EntiDbStats,
) -> EntiDbResult {
    if handle.is_null() {
        return EntiDbResult::NullPointer;
    }
    if out_stats.is_null() {
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let stats = db.stats();
    *out_stats = crate::types::EntiDbStats::from(stats);
    EntiDbResult::Ok
}

#[cfg(test)]
mod stats_tests {
    use super::*;
    use crate::buffer::entidb_free_buffer;

    #[test]
    fn test_stats() {
        unsafe {
            // Open database
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            let result = entidb_open_memory(&mut handle);
            assert_eq!(result, EntiDbResult::Ok);

            // Get initial stats
            let mut stats = crate::types::EntiDbStats::default();
            let result = entidb_stats(handle, &mut stats);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(stats.reads, 0);
            assert_eq!(stats.writes, 0);

            // Perform a write
            let coll_name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            let result = entidb_collection(handle, coll_name.as_ptr(), &mut coll_id);
            assert_eq!(result, EntiDbResult::Ok);

            let entity_id = EntiDbEntityId::from_bytes([1u8; 16]);
            let payload = vec![10u8, 20, 30];
            let result = entidb_put(handle, coll_id, entity_id, payload.as_ptr(), payload.len());
            assert_eq!(result, EntiDbResult::Ok);

            // Stats should reflect the write
            let result = entidb_stats(handle, &mut stats);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(stats.transactions_committed, 1);
            assert_eq!(stats.writes, 1);
            assert_eq!(stats.bytes_written, 3);

            // Perform a read
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_get(handle, coll_id, entity_id, &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            entidb_free_buffer(buffer);

            // Stats should reflect the read
            let result = entidb_stats(handle, &mut stats);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(stats.reads, 1);
            assert_eq!(stats.bytes_read, 3);

            entidb_close(handle);
        }
    }
}
