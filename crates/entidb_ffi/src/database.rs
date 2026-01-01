//! Database FFI functions.

use crate::buffer::EntiDbBuffer;
use crate::error::{clear_last_error, set_last_error, EntiDbResult};
use crate::types::{EntiDbCollectionId, EntiDbConfig, EntiDbEntityId, EntiDbHandle};
use std::ffi::CStr;
use std::path::Path;

/// Opens a database.
///
/// This function opens a database using the same directory-based layout as the Rust core:
/// - LOCK file for single-writer guarantee
/// - MANIFEST for metadata persistence
/// - WAL/ directory for write-ahead log
/// - SEGMENTS/ directory for segment files with proper rotation
///
/// This ensures binding parity: Dart, Python, and Rust all observe identical
/// durability, locking, and metadata behavior.
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

        // Build core config matching the FFI config
        let core_config = entidb_core::Config::default()
            .create_if_missing(config.create_if_missing)
            .sync_on_commit(config.sync_on_commit)
            .max_segment_size(if config.max_segment_size > 0 {
                config.max_segment_size
            } else {
                256 * 1024 * 1024 // Default 256MB
            });

        // Open database using the same directory-based path as Rust core
        // This ensures: LOCK file, MANIFEST, SEGMENTS/ layout, proper segment rotation
        match entidb_core::Database::open_with_config(path, core_config) {
            Ok(db) => {
                let boxed = Box::new(db);
                *out_handle = Box::into_raw(boxed) as *mut EntiDbHandle;
                EntiDbResult::Ok
            }
            Err(e) => {
                set_last_error(e.to_string());
                // Map specific errors to appropriate result codes
                if e.to_string().contains("locked") {
                    return EntiDbResult::Locked;
                }
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

    match db.create_collection(name_str) {
        Ok(id) => {
            *out_collection_id = EntiDbCollectionId::new(id.as_u32());
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
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
    static VERSION: &[u8] = b"2.0.0-alpha.1\0";
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

/// Compaction statistics returned by `entidb_compact`.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct EntiDbCompactionStats {
    /// Number of records in the input.
    pub input_records: u64,
    /// Number of records in the output.
    pub output_records: u64,
    /// Number of tombstones removed.
    pub tombstones_removed: u64,
    /// Number of obsolete versions removed.
    pub obsolete_versions_removed: u64,
    /// Bytes saved (estimated).
    pub bytes_saved: u64,
}

/// Compacts the database, removing obsolete versions and optionally tombstones.
///
/// Compaction merges segment records to:
/// - Remove obsolete entity versions (keeping only the latest)
/// - Optionally remove tombstones (deleted entities)
/// - Reclaim storage space
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `remove_tombstones` - If true, tombstones are removed; if false, they are preserved
/// * `out_stats` - Output pointer for compaction statistics (optional, may be null)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_stats` may be null or a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_compact(
    handle: *mut EntiDbHandle,
    remove_tombstones: bool,
    out_stats: *mut EntiDbCompactionStats,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() {
        set_last_error("null database handle");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    match db.compact(remove_tombstones) {
        Ok(stats) => {
            if !out_stats.is_null() {
                *out_stats = EntiDbCompactionStats {
                    input_records: stats.input_records as u64,
                    output_records: stats.output_records as u64,
                    tombstones_removed: stats.tombstones_removed as u64,
                    obsolete_versions_removed: stats.obsolete_versions_removed as u64,
                    bytes_saved: stats.bytes_saved as u64,
                };
            }
            EntiDbResult::Ok
        }
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
/// - `field` must be a valid null-terminated UTF-8 string specifying the field to index
///
/// # Note
///
/// Per `docs/access_paths.md`, users specify the field to index, not an arbitrary
/// index name. The engine manages index names internally.
#[no_mangle]
pub unsafe extern "C" fn entidb_create_hash_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    unique: bool,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    match db.create_hash_index(coll_id, field_str, unique) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Creates a BTree index on a field.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name to index
/// * `unique` - Whether the index enforces unique constraint
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string specifying the field to index
///
/// # Note
///
/// Per `docs/access_paths.md`, users specify the field to index, not an arbitrary
/// index name. The engine manages index names internally.
#[no_mangle]
pub unsafe extern "C" fn entidb_create_btree_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    unique: bool,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    match db.create_btree_index(coll_id, field_str, unique) {
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
/// * `field` - Null-terminated field name (must match field used in create_hash_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_insert(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
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

    match db.hash_index_insert(coll_id, field_str, key_bytes, ent_id) {
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
/// * `field` - Null-terminated field name (must match field used in create_btree_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_insert(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
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

    match db.btree_index_insert(coll_id, field_str, key_bytes, ent_id) {
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
/// * `field` - Null-terminated field name (must match field used in create_hash_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_remove(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
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

    match db.hash_index_remove(coll_id, field_str, key_bytes, ent_id) {
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
/// * `field` - Null-terminated field name (must match field used in create_btree_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_remove(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
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

    match db.btree_index_remove(coll_id, field_str, key_bytes, ent_id) {
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
/// * `field` - Null-terminated field name (must match field used in create_hash_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_lookup(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len)
    } else {
        &[]
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.hash_index_lookup(coll_id, field_str, key_bytes) {
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
/// * `field` - Null-terminated field name (must match field used in create_btree_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `key` must be valid for `key_len` bytes
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_lookup(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    key: *const u8,
    key_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    if key.is_null() && key_len > 0 {
        set_last_error("null key pointer with non-zero length");
        return EntiDbResult::InvalidArgument;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let key_bytes = if key_len > 0 {
        std::slice::from_raw_parts(key, key_len)
    } else {
        &[]
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.btree_index_lookup(coll_id, field_str, key_bytes) {
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
/// * `field` - Null-terminated field name (must match field used in create_btree_index)
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
/// - `field` must be a valid null-terminated UTF-8 string
/// - `min_key` must be valid for `min_key_len` bytes if non-null
/// - `max_key` must be valid for `max_key_len` bytes if non-null
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_range(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    min_key: *const u8,
    min_key_len: usize,
    max_key: *const u8,
    max_key_len: usize,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
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

    match db.btree_index_range(coll_id, field_str, min_key_bytes, max_key_bytes) {
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
/// * `field` - Null-terminated field name
/// * `out_count` - Output pointer for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_hash_index_len(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.hash_index_len(coll_id, field_str) {
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
/// * `field` - Null-terminated field name
/// * `out_count` - Output pointer for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_btree_index_len(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.btree_index_len(coll_id, field_str) {
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
/// The engine manages index names internally. Users specify the field that was indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name that was indexed
///
/// # Returns
///
/// `EntiDbResult::Ok` on success (returns Ok even if index didn't exist).
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_drop_hash_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.drop_hash_index(coll_id, field_str) {
        Ok(_) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Drops a BTree index.
///
/// The engine manages index names internally. Users specify the field that was indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name that was indexed
///
/// # Returns
///
/// `EntiDbResult::Ok` on success (returns Ok even if index didn't exist).
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_drop_btree_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.drop_btree_index(coll_id, field_str) {
        Ok(_) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

// ==================== FTS Index FFI Functions ====================

/// Creates a Full-Text Search (FTS) index with default configuration.
///
/// The engine manages index names internally. Users specify the field to index.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name to index
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_create_fts_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    match db.create_fts_index(coll_id, field_str) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Creates an FTS index with custom configuration.
///
/// The engine manages index names internally. Users specify the field to index.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name to index
/// * `min_token_length` - Minimum token length (tokens shorter are ignored)
/// * `max_token_length` - Maximum token length (tokens longer are truncated)
/// * `case_sensitive` - If true, searches are case-sensitive
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_create_fts_index_with_config(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    min_token_length: usize,
    max_token_length: usize,
    case_sensitive: bool,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    match db.create_fts_index_with_config(
        coll_id,
        field_str,
        min_token_length,
        max_token_length,
        case_sensitive,
    ) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Indexes text content for an entity in an FTS index.
///
/// The engine manages index names internally. Users specify the field being indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being indexed
/// * `entity_id` - The entity ID to associate with the text
/// * `text` - Null-terminated text content to index
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `text` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_index_text(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    entity_id: EntiDbEntityId,
    text: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || text.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let text_cstr = CStr::from_ptr(text);
    let text_str = match text_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in text");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.fts_index_text(coll_id, field_str, ent_id, text_str) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Removes an entity from an FTS index.
///
/// The engine manages index names internally. Users specify the field being indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being indexed
/// * `entity_id` - The entity ID to remove
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_remove_entity(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    entity_id: EntiDbEntityId,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);
    let ent_id = entidb_core::EntityId::from_bytes(entity_id.bytes);

    match db.fts_remove_entity(coll_id, field_str, ent_id) {
        Ok(_) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Searches an FTS index using AND semantics (all terms must match).
///
/// The engine manages index names internally. Users specify the field being searched.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being searched
/// * `query` - Null-terminated search query (space-separated terms)
/// * `out_buffer` - Output buffer for entity IDs (16 bytes each)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `query` must be a valid null-terminated UTF-8 string
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_search(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    query: *const std::ffi::c_char,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || query.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let query_cstr = CStr::from_ptr(query);
    let query_str = match query_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in query");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.fts_search(coll_id, field_str, query_str) {
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

/// Searches an FTS index using OR semantics (any term may match).
///
/// The engine manages index names internally. Users specify the field being searched.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being searched
/// * `query` - Null-terminated search query (space-separated terms)
/// * `out_buffer` - Output buffer for entity IDs (16 bytes each)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `query` must be a valid null-terminated UTF-8 string
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_search_any(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    query: *const std::ffi::c_char,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || query.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let query_cstr = CStr::from_ptr(query);
    let query_str = match query_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in query");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.fts_search_any(coll_id, field_str, query_str) {
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

/// Searches an FTS index using prefix matching.
///
/// The engine manages index names internally. Users specify the field being searched.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being searched
/// * `prefix` - Null-terminated prefix to search for
/// * `out_buffer` - Output buffer for entity IDs (16 bytes each)
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `prefix` must be a valid null-terminated UTF-8 string
/// - `out_buffer` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_search_prefix(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    prefix: *const std::ffi::c_char,
    out_buffer: *mut EntiDbBuffer,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || prefix.is_null() || out_buffer.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let prefix_cstr = CStr::from_ptr(prefix);
    let prefix_str = match prefix_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in prefix");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.fts_search_prefix(coll_id, field_str, prefix_str) {
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

/// Gets the number of entities in an FTS index.
///
/// The engine manages index names internally. Users specify the field being indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being indexed
/// * `out_count` - Output pointer for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_index_len(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.fts_index_len(coll_id, field_str) {
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

/// Gets the number of unique tokens in an FTS index.
///
/// The engine manages index names internally. Users specify the field being indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being indexed
/// * `out_count` - Output pointer for the count
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
/// - `out_count` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_unique_token_count(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
    out_count: *mut usize,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() || out_count.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.fts_unique_token_count(coll_id, field_str) {
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

/// Clears all entries from an FTS index.
///
/// The engine manages index names internally. Users specify the field being indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name being indexed
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_fts_clear(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.fts_clear(coll_id, field_str) {
        Ok(()) => EntiDbResult::Ok,
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Drops an FTS index.
///
/// The engine manages index names internally. Users specify the field that was indexed.
/// See `docs/access_paths.md` for engine-controlled access path semantics.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `collection_id` - The collection ID
/// * `field` - Null-terminated field name that was indexed
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `field` must be a valid null-terminated UTF-8 string
#[no_mangle]
pub unsafe extern "C" fn entidb_drop_fts_index(
    handle: *mut EntiDbHandle,
    collection_id: EntiDbCollectionId,
    field: *const std::ffi::c_char,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || field.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let field_cstr = CStr::from_ptr(field);
    let field_str = match field_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("invalid UTF-8 in field name");
            return EntiDbResult::InvalidArgument;
        }
    };

    let coll_id = entidb_core::CollectionId::new(collection_id.id);

    match db.drop_fts_index(coll_id, field_str) {
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
        assert_eq!(s.to_str().unwrap(), "2.0.0-alpha.1");
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

// ============================================================================
// Change Feed
// ============================================================================

/// Polls the change feed for events since a given sequence number.
///
/// Returns up to `limit` events with sequence > `cursor`.
/// This is the recommended way for bindings to access the change feed.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `cursor` - Return events after this sequence number (0 = from beginning)
/// * `limit` - Maximum number of events to return
/// * `out_events` - Output pointer for the event list
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
///
/// # Safety
///
/// - `handle` must be a valid database handle
/// - `out_events` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn entidb_poll_changes(
    handle: *mut EntiDbHandle,
    cursor: u64,
    limit: usize,
    out_events: *mut crate::types::EntiDbChangeEventList,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_events.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    let events = db.change_feed().poll(cursor, limit);

    if events.is_empty() {
        *out_events = crate::types::EntiDbChangeEventList::empty();
        return EntiDbResult::Ok;
    }

    // Allocate event array
    let mut ffi_events: Vec<crate::types::EntiDbChangeEvent> = Vec::with_capacity(events.len());

    for event in &events {
        // Allocate payload if present
        let (payload_ptr, payload_len) = if let Some(ref data) = event.payload {
            let boxed_data = data.clone().into_boxed_slice();
            let len = boxed_data.len();
            let ptr = Box::into_raw(boxed_data) as *const u8;
            (ptr, len)
        } else {
            (std::ptr::null(), 0)
        };

        ffi_events.push(crate::types::EntiDbChangeEvent {
            sequence: event.sequence,
            collection_id: event.collection_id,
            entity_id: event.entity_id,
            change_type: event.change_type.into(),
            payload: payload_ptr,
            payload_len,
        });
    }

    let count = ffi_events.len();
    let capacity = ffi_events.capacity();
    let events_ptr = ffi_events.as_mut_ptr();
    std::mem::forget(ffi_events);

    *out_events = crate::types::EntiDbChangeEventList {
        events: events_ptr,
        count,
        capacity,
    };

    EntiDbResult::Ok
}

/// Returns the latest sequence number in the change feed history.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_sequence` - Output pointer for the sequence number
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
#[no_mangle]
pub unsafe extern "C" fn entidb_latest_sequence(
    handle: *mut EntiDbHandle,
    out_sequence: *mut u64,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_sequence.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);
    *out_sequence = db.change_feed().latest_sequence();
    EntiDbResult::Ok
}

/// Frees a change event list returned by `entidb_poll_changes`.
///
/// # Safety
///
/// The event list must have been returned by `entidb_poll_changes`.
#[no_mangle]
pub unsafe extern "C" fn entidb_free_change_events(events: crate::types::EntiDbChangeEventList) {
    if events.events.is_null() {
        return;
    }

    // Free each event's payload
    let slice = std::slice::from_raw_parts_mut(events.events, events.count);
    for event in slice.iter() {
        if !event.payload.is_null() && event.payload_len > 0 {
            let _ = Box::from_raw(std::slice::from_raw_parts_mut(
                event.payload as *mut u8,
                event.payload_len,
            ));
        }
    }

    // Free the events array
    let _ = Vec::from_raw_parts(events.events, events.count, events.capacity);
}

// ============================================================================
// Schema Version / Migration State
// ============================================================================

/// Gets the current schema version.
///
/// Schema version is stored in database metadata and can be used
/// by bindings to track migration state.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `out_version` - Output pointer for the version number
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
#[no_mangle]
pub unsafe extern "C" fn entidb_get_schema_version(
    handle: *mut EntiDbHandle,
    out_version: *mut u64,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() || out_version.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);

    // Use a special metadata collection for schema version
    let meta_collection = db.collection_unchecked("__entidb_meta__");
    let version_key = entidb_core::EntityId::from_bytes([
        0x5c, 0x68, 0x65, 0x6d, 0x61, 0x5f, 0x76, 0x65, // "schema_ve"
        0x72, 0x73, 0x69, 0x6f, 0x6e, 0x00, 0x00, 0x00, // "rsion\0\0\0"
    ]);

    match db.get(meta_collection, version_key) {
        Ok(Some(data)) if data.len() >= 8 => {
            *out_version = u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]);
            EntiDbResult::Ok
        }
        Ok(_) => {
            *out_version = 0; // No version set
            EntiDbResult::Ok
        }
        Err(e) => {
            set_last_error(e.to_string());
            EntiDbResult::Error
        }
    }
}

/// Sets the schema version.
///
/// This should be called after running migrations to record the new version.
///
/// # Arguments
///
/// * `handle` - The database handle
/// * `version` - The new schema version
///
/// # Returns
///
/// `EntiDbResult::Ok` on success, error code otherwise.
#[no_mangle]
pub unsafe extern "C" fn entidb_set_schema_version(
    handle: *mut EntiDbHandle,
    version: u64,
) -> EntiDbResult {
    clear_last_error();

    if handle.is_null() {
        set_last_error("null pointer argument");
        return EntiDbResult::NullPointer;
    }

    let db = &*(handle as *mut entidb_core::Database);

    // Use a special metadata collection for schema version
    let meta_collection = db.collection_unchecked("__entidb_meta__");
    let version_key = entidb_core::EntityId::from_bytes([
        0x5c, 0x68, 0x65, 0x6d, 0x61, 0x5f, 0x76, 0x65, // "schema_ve"
        0x72, 0x73, 0x69, 0x6f, 0x6e, 0x00, 0x00, 0x00, // "rsion\0\0\0"
    ]);

    let version_bytes = version.to_le_bytes().to_vec();

    let result = db.transaction(|txn| {
        txn.put(meta_collection, version_key, version_bytes)?;
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

#[cfg(test)]
mod change_feed_tests {
    use super::*;
    use crate::buffer::entidb_free_buffer;

    #[test]
    fn test_poll_changes() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            let result = entidb_open_memory(&mut handle);
            assert_eq!(result, EntiDbResult::Ok);

            // Initial poll should return empty
            let mut events = crate::types::EntiDbChangeEventList::empty();
            let result = entidb_poll_changes(handle, 0, 100, &mut events);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(events.count, 0);

            // Perform a write
            let coll_name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, coll_name.as_ptr(), &mut coll_id);

            let entity_id = EntiDbEntityId::from_bytes([1u8; 16]);
            let payload = vec![10u8, 20, 30];
            entidb_put(handle, coll_id, entity_id, payload.as_ptr(), payload.len());

            // Poll should now return the change
            let result = entidb_poll_changes(handle, 0, 100, &mut events);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(events.count >= 1);

            // Free events
            entidb_free_change_events(events);

            entidb_close(handle);
        }
    }

    #[test]
    fn test_latest_sequence() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            let mut seq: u64 = 0;
            let result = entidb_latest_sequence(handle, &mut seq);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(seq, 0);

            // Add some data
            let coll_name = std::ffi::CString::new("test").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, coll_name.as_ptr(), &mut coll_id);

            let entity_id = EntiDbEntityId::from_bytes([1u8; 16]);
            let payload = vec![1u8];
            entidb_put(handle, coll_id, entity_id, payload.as_ptr(), payload.len());

            let result = entidb_latest_sequence(handle, &mut seq);
            assert_eq!(result, EntiDbResult::Ok);
            assert!(seq >= 1);

            entidb_close(handle);
        }
    }

    #[test]
    fn test_schema_version() {
        unsafe {
            let mut handle: *mut EntiDbHandle = std::ptr::null_mut();
            entidb_open_memory(&mut handle);

            // Initial version should be 0
            let mut version: u64 = 999;
            let result = entidb_get_schema_version(handle, &mut version);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(version, 0);

            // Set version
            let result = entidb_set_schema_version(handle, 5);
            assert_eq!(result, EntiDbResult::Ok);

            // Get version
            let result = entidb_get_schema_version(handle, &mut version);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(version, 5);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_index_operations() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            // Get collection
            let name = std::ffi::CString::new("documents").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Create FTS index
            let index_name = std::ffi::CString::new("content").unwrap();
            let result = entidb_create_fts_index(handle, coll_id, index_name.as_ptr());
            assert_eq!(result, EntiDbResult::Ok);

            // Generate entity IDs
            let mut entity1 = EntiDbEntityId::zero();
            let mut entity2 = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity1);
            entidb_generate_id(&mut entity2);

            // Index text
            let text1 = std::ffi::CString::new("Hello world from Rust").unwrap();
            let result = entidb_fts_index_text(
                handle,
                coll_id,
                index_name.as_ptr(),
                entity1,
                text1.as_ptr(),
            );
            assert_eq!(result, EntiDbResult::Ok);

            let text2 = std::ffi::CString::new("Hello Python programming").unwrap();
            let result = entidb_fts_index_text(
                handle,
                coll_id,
                index_name.as_ptr(),
                entity2,
                text2.as_ptr(),
            );
            assert_eq!(result, EntiDbResult::Ok);

            // Check index length
            let mut count: usize = 0;
            let result = entidb_fts_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 2);

            // Search for "hello" - should find both
            let query = std::ffi::CString::new("hello").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            let result =
                entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 32); // Two entity IDs (16 bytes each)
            entidb_free_buffer(buffer);

            // Search for "rust" - should find only one
            let query = std::ffi::CString::new("rust").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            let result =
                entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 16); // One entity ID
            entidb_free_buffer(buffer);

            // Drop index
            let result = entidb_drop_fts_index(handle, coll_id, index_name.as_ptr());
            assert_eq!(result, EntiDbResult::Ok);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_search_and_or_semantics() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let index_name = std::ffi::CString::new("content").unwrap();
            entidb_create_fts_index(handle, coll_id, index_name.as_ptr());

            // Index documents
            let mut entity1 = EntiDbEntityId::zero();
            let mut entity2 = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity1);
            entidb_generate_id(&mut entity2);

            let text1 = std::ffi::CString::new("apple orange").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity1, text1.as_ptr());

            let text2 = std::ffi::CString::new("banana orange").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity2, text2.as_ptr());

            // AND search: "apple orange" - only entity1 has both
            let query = std::ffi::CString::new("apple orange").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 16); // One match
            entidb_free_buffer(buffer);

            // OR search: "apple banana" - both match
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search_any(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            // "apple orange" with OR - entity1 has apple and orange, entity2 has orange
            // Actually this query is "apple orange" so OR should match both (both have "orange")
            assert!(buffer.len >= 16);
            entidb_free_buffer(buffer);

            // Search for "apple banana" with OR - should find both
            let query2 = std::ffi::CString::new("apple banana").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search_any(handle, coll_id, index_name.as_ptr(), query2.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 32); // Both match (entity1 has apple, entity2 has banana)
            entidb_free_buffer(buffer);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_prefix_search() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let index_name = std::ffi::CString::new("content").unwrap();
            entidb_create_fts_index(handle, coll_id, index_name.as_ptr());

            let mut entity1 = EntiDbEntityId::zero();
            let mut entity2 = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity1);
            entidb_generate_id(&mut entity2);

            let text1 = std::ffi::CString::new("programming in Rust").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity1, text1.as_ptr());

            let text2 = std::ffi::CString::new("program management").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity2, text2.as_ptr());

            // Prefix search for "prog" - should find both
            let prefix = std::ffi::CString::new("prog").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_fts_search_prefix(
                handle,
                coll_id,
                index_name.as_ptr(),
                prefix.as_ptr(),
                &mut buffer,
            );
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(buffer.len, 32); // Both entities
            entidb_free_buffer(buffer);

            // Prefix search for "rust" - should find one
            let prefix = std::ffi::CString::new("rust").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search_prefix(
                handle,
                coll_id,
                index_name.as_ptr(),
                prefix.as_ptr(),
                &mut buffer,
            );
            assert_eq!(buffer.len, 16); // One entity
            entidb_free_buffer(buffer);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_with_custom_config() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            // Create with custom config: min token length 3, case-sensitive
            let index_name = std::ffi::CString::new("content").unwrap();
            let result = entidb_create_fts_index_with_config(
                handle,
                coll_id,
                index_name.as_ptr(),
                3,   // min token length
                256, // max token length
                true, // case sensitive
            );
            assert_eq!(result, EntiDbResult::Ok);

            let mut entity = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity);

            let text = std::ffi::CString::new("I am a Rust Developer").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity, text.as_ptr());

            // Short tokens ("I", "am", "a") should be ignored
            let query = std::ffi::CString::new("am").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 0); // No match - "am" too short
            entidb_free_buffer(buffer);

            // "Rust" should match (case-sensitive)
            let query = std::ffi::CString::new("Rust").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 16);
            entidb_free_buffer(buffer);

            // "rust" (lowercase) should NOT match in case-sensitive mode
            let query = std::ffi::CString::new("rust").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 0);
            entidb_free_buffer(buffer);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_remove_entity() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let index_name = std::ffi::CString::new("content").unwrap();
            entidb_create_fts_index(handle, coll_id, index_name.as_ptr());

            let mut entity1 = EntiDbEntityId::zero();
            let mut entity2 = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity1);
            entidb_generate_id(&mut entity2);

            let text1 = std::ffi::CString::new("hello world").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity1, text1.as_ptr());

            let text2 = std::ffi::CString::new("hello rust").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity2, text2.as_ptr());

            // Both match "hello"
            let query = std::ffi::CString::new("hello").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 32);
            entidb_free_buffer(buffer);

            // Remove entity1
            let result = entidb_fts_remove_entity(handle, coll_id, index_name.as_ptr(), entity1);
            assert_eq!(result, EntiDbResult::Ok);

            // Now only entity2 matches "hello"
            let mut buffer = EntiDbBuffer::empty();
            entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(buffer.len, 16);
            entidb_free_buffer(buffer);

            // Check index length
            let mut count: usize = 0;
            entidb_fts_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(count, 1);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_clear_index() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let index_name = std::ffi::CString::new("content").unwrap();
            entidb_create_fts_index(handle, coll_id, index_name.as_ptr());

            // Add some entities
            for i in 0..5 {
                let mut entity = EntiDbEntityId::zero();
                entidb_generate_id(&mut entity);
                let text = std::ffi::CString::new(format!("document number {}", i)).unwrap();
                entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity, text.as_ptr());
            }

            // Verify count
            let mut count: usize = 0;
            entidb_fts_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(count, 5);

            // Clear index
            let result = entidb_fts_clear(handle, coll_id, index_name.as_ptr());
            assert_eq!(result, EntiDbResult::Ok);

            // Verify count is 0
            entidb_fts_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(count, 0);

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_unique_token_count() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let index_name = std::ffi::CString::new("content").unwrap();
            entidb_create_fts_index(handle, coll_id, index_name.as_ptr());

            let mut entity1 = EntiDbEntityId::zero();
            let mut entity2 = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity1);
            entidb_generate_id(&mut entity2);

            // "hello world hello" - unique tokens: hello, world
            let text1 = std::ffi::CString::new("hello world hello").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity1, text1.as_ptr());

            // "hello rust" - unique tokens overall: hello, world, rust
            let text2 = std::ffi::CString::new("hello rust").unwrap();
            entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity2, text2.as_ptr());

            let mut count: usize = 0;
            let result = entidb_fts_unique_token_count(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(result, EntiDbResult::Ok);
            assert_eq!(count, 3); // hello, world, rust

            entidb_close(handle);
        }
    }

    #[test]
    fn fts_nonexistent_index_errors() {
        let mut handle: *mut EntiDbHandle = std::ptr::null_mut();

        unsafe {
            entidb_open_memory(&mut handle);

            let name = std::ffi::CString::new("docs").unwrap();
            let mut coll_id = EntiDbCollectionId::new(0);
            entidb_collection(handle, name.as_ptr(), &mut coll_id);

            let index_name = std::ffi::CString::new("nonexistent").unwrap();

            // Operations on non-existent index should fail
            let mut entity = EntiDbEntityId::zero();
            entidb_generate_id(&mut entity);

            let text = std::ffi::CString::new("test").unwrap();
            let result = entidb_fts_index_text(handle, coll_id, index_name.as_ptr(), entity, text.as_ptr());
            assert_eq!(result, EntiDbResult::Error);

            let query = std::ffi::CString::new("test").unwrap();
            let mut buffer = EntiDbBuffer::empty();
            let result = entidb_fts_search(handle, coll_id, index_name.as_ptr(), query.as_ptr(), &mut buffer);
            assert_eq!(result, EntiDbResult::Error);

            let mut count: usize = 0;
            let result = entidb_fts_index_len(handle, coll_id, index_name.as_ptr(), &mut count);
            assert_eq!(result, EntiDbResult::Error);

            entidb_close(handle);
        }
    }
}
