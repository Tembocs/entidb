//! Error types for storage operations.

use std::io;
use thiserror::Error;

/// Result type for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Errors that can occur during storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Attempted to read beyond the end of storage.
    #[error("read beyond end of storage: offset {offset}, len {len}, size {size}")]
    ReadPastEnd {
        /// The requested read offset.
        offset: u64,
        /// The requested read length.
        len: usize,
        /// The current storage size.
        size: u64,
    },

    /// The storage file is corrupted.
    #[error("storage corrupted: {0}")]
    Corrupted(String),

    /// The storage is closed.
    #[error("storage is closed")]
    Closed,

    /// Encryption or decryption failed.
    #[error("encryption error: {0}")]
    Encryption(String),
}
