//! Web storage backend implementations.
//!
//! This module provides storage backends for web environments:
//! - OPFS (Origin Private File System) for modern browsers
//! - IndexedDB as a fallback for older browsers
//!
//! Both backends implement the same byte-store abstraction that EntiDB
//! uses for native file storage.

mod indexeddb;
mod memory;
mod opfs;
mod persistent;

#[allow(unused_imports)]
pub use indexeddb::IndexedDbBackend;
#[allow(unused_imports)]
pub use memory::WasmMemoryBackend;
#[allow(unused_imports)]
pub use opfs::OpfsBackend;
pub use persistent::{PersistentBackend, StorageType};

/// Check if OPFS is available in the current browser.
#[allow(dead_code)]
pub fn is_opfs_available() -> bool {
    OpfsBackend::is_available()
}

/// Check if IndexedDB is available.
#[allow(dead_code)]
pub fn is_indexeddb_available() -> bool {
    IndexedDbBackend::is_available()
}
