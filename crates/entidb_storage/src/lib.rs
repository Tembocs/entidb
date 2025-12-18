//! # EntiDB Storage
//!
//! Storage backend trait and implementations for EntiDB.
//!
//! This crate provides the lowest-level storage abstraction for EntiDB.
//! Storage backends are **opaque byte stores** - they do not interpret
//! the data they store.
//!
//! ## Design Principles
//!
//! - Backends are simple byte stores (read, append, flush)
//! - No knowledge of EntiDB file formats, WAL, or segments
//! - Must be `Send + Sync` for concurrent access
//! - EntiDB owns all file format interpretation
//!
//! ## Available Backends
//!
//! - [`InMemoryBackend`] - For testing and ephemeral storage
//! - [`FileBackend`] - For persistent storage using OS file APIs
//! - [`EncryptedBackend`] - Wrapper that adds AES-256-GCM encryption
//!
//! ## Example
//!
//! ```rust
//! use entidb_storage::{StorageBackend, InMemoryBackend};
//!
//! let mut backend = InMemoryBackend::new();
//! let offset = backend.append(b"hello world").unwrap();
//! let data = backend.read_at(offset, 11).unwrap();
//! assert_eq!(&data, b"hello world");
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod backend;
mod encrypted;
mod error;
mod file;
mod memory;

pub use backend::StorageBackend;
pub use encrypted::{EncryptedBackend, EncryptionKey, KEY_SIZE, NONCE_SIZE, TAG_SIZE};
pub use error::{StorageError, StorageResult};
pub use file::FileBackend;
pub use memory::InMemoryBackend;
