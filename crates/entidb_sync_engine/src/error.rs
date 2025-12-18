//! Error types for the sync engine.

use thiserror::Error;

/// Result type for sync operations.
pub type SyncResult<T> = Result<T, SyncError>;

/// Errors that can occur during sync operations.
#[derive(Error, Debug)]
pub enum SyncError {
    /// Network or transport error.
    #[error("transport error: {message}")]
    Transport {
        /// Error message.
        message: String,
        /// Whether the operation can be retried.
        retryable: bool,
    },

    /// Protocol error (invalid message format).
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Authentication failed.
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Server rejected the request.
    #[error("server error: {0}")]
    ServerError(String),

    /// Database error during sync.
    #[error("database error: {0}")]
    Database(#[from] entidb_core::CoreError),

    /// Conflict that requires manual resolution.
    #[error("unresolved conflict for entity {entity_id:?} in collection {collection_id}")]
    UnresolvedConflict {
        /// Collection ID.
        collection_id: u32,
        /// Entity ID.
        entity_id: [u8; 16],
    },

    /// Sync was cancelled.
    #[error("sync cancelled")]
    Cancelled,

    /// Invalid state transition.
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        /// Current state.
        from: String,
        /// Attempted target state.
        to: String,
    },

    /// Codec error.
    #[error("codec error: {0}")]
    Codec(String),

    /// Timeout.
    #[error("operation timed out")]
    Timeout,

    /// Not connected.
    #[error("not connected to server")]
    NotConnected,

    /// Version mismatch.
    #[error("protocol version mismatch: local={local}, remote={remote}")]
    VersionMismatch {
        /// Local protocol version.
        local: u16,
        /// Remote protocol version.
        remote: u16,
    },
}

impl SyncError {
    /// Creates a retryable transport error.
    pub fn transport_retryable(message: impl Into<String>) -> Self {
        Self::Transport {
            message: message.into(),
            retryable: true,
        }
    }

    /// Creates a non-retryable transport error.
    pub fn transport_fatal(message: impl Into<String>) -> Self {
        Self::Transport {
            message: message.into(),
            retryable: false,
        }
    }

    /// Returns true if this error can be retried.
    pub fn is_retryable(&self) -> bool {
        match self {
            SyncError::Transport { retryable, .. } => *retryable,
            SyncError::Timeout => true,
            SyncError::ServerError(_) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_errors() {
        assert!(SyncError::transport_retryable("connection lost").is_retryable());
        assert!(!SyncError::transport_fatal("invalid certificate").is_retryable());
        assert!(SyncError::Timeout.is_retryable());
        assert!(SyncError::ServerError("internal error".into()).is_retryable());
        assert!(!SyncError::Cancelled.is_retryable());
    }

    #[test]
    fn error_display() {
        let err = SyncError::NotConnected;
        assert_eq!(err.to_string(), "not connected to server");

        let err = SyncError::VersionMismatch {
            local: 1,
            remote: 2,
        };
        assert!(err.to_string().contains("1"));
        assert!(err.to_string().contains("2"));
    }
}
