//! Error types for EntiDB core.

use std::io;
use thiserror::Error;

/// Result type for core operations.
pub type CoreResult<T> = Result<T, CoreError>;

/// Errors that can occur in EntiDB core operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Storage backend error.
    #[error("storage error: {0}")]
    Storage(#[from] entidb_storage::StorageError),

    /// CBOR codec error.
    #[error("codec error: {0}")]
    Codec(#[from] entidb_codec::CodecError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// WAL is corrupted or invalid.
    #[error("WAL corruption: {message}")]
    WalCorruption {
        /// Description of the corruption.
        message: String,
    },

    /// Segment is corrupted or invalid.
    #[error("segment corruption: {message}")]
    SegmentCorruption {
        /// Description of the corruption.
        message: String,
    },

    /// Transaction was aborted.
    #[error("transaction aborted: {reason}")]
    TransactionAborted {
        /// Reason for abort.
        reason: String,
    },

    /// Transaction conflict detected.
    #[error("transaction conflict on entity {entity_id:?} in collection {collection_id}")]
    TransactionConflict {
        /// The collection where conflict occurred.
        collection_id: u32,
        /// The entity that conflicted.
        entity_id: [u8; 16],
    },

    /// Entity not found.
    #[error("entity not found: {entity_id:?} in collection {collection_id}")]
    EntityNotFound {
        /// The collection searched.
        collection_id: u32,
        /// The entity ID that was not found.
        entity_id: [u8; 16],
    },

    /// Collection not found.
    #[error("collection not found: {name}")]
    CollectionNotFound {
        /// Name of the collection.
        name: String,
    },

    /// Database is already open or locked.
    #[error("database locked: another process has exclusive access")]
    DatabaseLocked,

    /// Invalid database format or version.
    #[error("invalid database format: {message}")]
    InvalidFormat {
        /// Description of the format issue.
        message: String,
    },

    /// Checksum mismatch detected.
    #[error("checksum mismatch: expected {expected:08x}, got {actual:08x}")]
    ChecksumMismatch {
        /// Expected checksum.
        expected: u32,
        /// Actual checksum.
        actual: u32,
    },

    /// Operation not permitted in current state.
    #[error("invalid operation: {message}")]
    InvalidOperation {
        /// Description of why operation is invalid.
        message: String,
    },

    /// Database is closed.
    #[error("database is closed")]
    DatabaseClosed,

    /// Encryption is not enabled.
    #[error("encryption feature not enabled")]
    EncryptionNotEnabled,

    /// Encryption failed.
    #[error("encryption failed: {message}")]
    EncryptionFailed {
        /// Description of the failure.
        message: String,
    },

    /// Decryption failed.
    #[error("decryption failed: {message}")]
    DecryptionFailed {
        /// Description of the failure.
        message: String,
    },

    /// Invalid key size.
    #[error("invalid key size: expected {expected} bytes, got {actual}")]
    InvalidKeySize {
        /// Expected size in bytes.
        expected: usize,
        /// Actual size in bytes.
        actual: usize,
    },

    /// Key derivation failed.
    #[error("key derivation failed: {message}")]
    KeyDerivationFailed {
        /// Description of the failure.
        message: String,
    },

    /// Migration failed.
    #[error("migration failed: {message}")]
    MigrationFailed {
        /// Description of the failure.
        message: String,
    },

    /// Manifest persistence failed.
    ///
    /// This error occurs when the database fails to persist metadata changes
    /// (such as new collections or indexes) to the manifest file on disk.
    /// The in-memory state is rolled back to ensure consistency.
    #[error("manifest persist failed: {message}")]
    ManifestPersistFailed {
        /// Description of the failure.
        message: String,
    },

    /// Commit succeeded in WAL but segment apply failed.
    ///
    /// This error indicates that the transaction is durably committed in the WAL
    /// but the segment write failed. The database requires recovery to complete
    /// the commit. The transaction WILL be applied on next database open.
    ///
    /// **Important:** The caller should NOT retry the transaction - it is already
    /// committed and will be recovered. The database should be reopened to
    /// trigger recovery.
    #[error("commit accepted but segment apply failed (recovery required): {message}")]
    CommitPendingRecovery {
        /// The committed sequence number.
        sequence: u64,
        /// Description of the segment failure.
        message: String,
    },

    /// Segment file creation failed.
    ///
    /// This error occurs when a segment file cannot be created on disk.
    /// Unlike silent fallback to in-memory, this is a hard error that
    /// prevents the database from opening in an unreliable state.
    #[error("segment file creation failed: {path}")]
    SegmentFileCreationFailed {
        /// Path that failed to create.
        path: String,
        /// Underlying error message.
        source_message: String,
    },

    /// Invalid argument provided.
    ///
    /// This error occurs when an invalid argument is passed to an API.
    #[error("invalid argument: {message}")]
    InvalidArgument {
        /// Description of the argument issue.
        message: String,
    },
}

impl CoreError {
    /// Creates a WAL corruption error.
    pub fn wal_corruption(message: impl Into<String>) -> Self {
        Self::WalCorruption {
            message: message.into(),
        }
    }

    /// Creates a segment corruption error.
    pub fn segment_corruption(message: impl Into<String>) -> Self {
        Self::SegmentCorruption {
            message: message.into(),
        }
    }

    /// Creates a transaction aborted error.
    pub fn transaction_aborted(reason: impl Into<String>) -> Self {
        Self::TransactionAborted {
            reason: reason.into(),
        }
    }

    /// Creates an invalid format error.
    pub fn invalid_format(message: impl Into<String>) -> Self {
        Self::InvalidFormat {
            message: message.into(),
        }
    }

    /// Creates an invalid operation error.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation {
            message: message.into(),
        }
    }

    /// Creates an encryption not enabled error.
    pub fn encryption_not_enabled() -> Self {
        Self::EncryptionNotEnabled
    }

    /// Creates an encryption failed error.
    pub fn encryption_failed(message: impl Into<String>) -> Self {
        Self::EncryptionFailed {
            message: message.into(),
        }
    }

    /// Creates a decryption failed error.
    pub fn decryption_failed(message: impl Into<String>) -> Self {
        Self::DecryptionFailed {
            message: message.into(),
        }
    }

    /// Creates an invalid key size error.
    pub fn invalid_key_size(actual: usize, expected: usize) -> Self {
        Self::InvalidKeySize { expected, actual }
    }

    /// Creates a key derivation failed error.
    pub fn key_derivation_failed(message: impl Into<String>) -> Self {
        Self::KeyDerivationFailed {
            message: message.into(),
        }
    }

    /// Creates a migration failed error.
    pub fn migration_failed(message: impl Into<String>) -> Self {
        Self::MigrationFailed {
            message: message.into(),
        }
    }

    /// Creates a manifest persist failed error.
    pub fn manifest_persist_failed(message: impl Into<String>) -> Self {
        Self::ManifestPersistFailed {
            message: message.into(),
        }
    }

    /// Creates a commit pending recovery error.
    pub fn commit_pending_recovery(sequence: u64, message: impl Into<String>) -> Self {
        Self::CommitPendingRecovery {
            sequence,
            message: message.into(),
        }
    }

    /// Creates a segment file creation failed error.
    pub fn segment_file_creation_failed(path: impl Into<String>, source: impl Into<String>) -> Self {
        Self::SegmentFileCreationFailed {
            path: path.into(),
            source_message: source.into(),
        }
    }

    /// Creates an invalid argument error.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            message: message.into(),
        }
    }
}
