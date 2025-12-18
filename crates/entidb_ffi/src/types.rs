//! Type definitions for FFI.

/// An opaque database handle.
///
/// This is a pointer to the internal database structure.
/// Never dereference or modify directly.
#[repr(C)]
pub struct EntiDbHandle {
    _private: [u8; 0],
}

/// An opaque transaction handle.
#[repr(C)]
pub struct EntiDbTransaction {
    _private: [u8; 0],
}

/// Entity ID as a 16-byte array.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntiDbEntityId {
    /// The 16-byte entity ID.
    pub bytes: [u8; 16],
}

impl EntiDbEntityId {
    /// Creates a new entity ID from bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }

    /// Creates a zero ID.
    pub fn zero() -> Self {
        Self { bytes: [0; 16] }
    }
}

/// Collection ID.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntiDbCollectionId {
    /// The collection ID.
    pub id: u32,
}

impl EntiDbCollectionId {
    /// Creates a new collection ID.
    pub fn new(id: u32) -> Self {
        Self { id }
    }
}

/// Configuration for opening a database.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct EntiDbConfig {
    /// Path to database directory (null-terminated UTF-8).
    pub path: *const std::ffi::c_char,
    /// Maximum segment size in bytes.
    pub max_segment_size: u64,
    /// Whether to sync on commit.
    pub sync_on_commit: bool,
    /// Whether to create if not exists.
    pub create_if_missing: bool,
}

impl Default for EntiDbConfig {
    fn default() -> Self {
        Self {
            path: std::ptr::null(),
            max_segment_size: 64 * 1024 * 1024, // 64MB
            sync_on_commit: true,
            create_if_missing: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id() {
        let id = EntiDbEntityId::from_bytes([1u8; 16]);
        assert_eq!(id.bytes, [1u8; 16]);

        let zero = EntiDbEntityId::zero();
        assert_eq!(zero.bytes, [0u8; 16]);
    }

    #[test]
    fn collection_id() {
        let cid = EntiDbCollectionId::new(42);
        assert_eq!(cid.id, 42);
    }

    #[test]
    fn config_default() {
        let config = EntiDbConfig::default();
        assert!(config.path.is_null());
        assert_eq!(config.max_segment_size, 64 * 1024 * 1024);
        assert!(config.sync_on_commit);
        assert!(config.create_if_missing);
    }
}
