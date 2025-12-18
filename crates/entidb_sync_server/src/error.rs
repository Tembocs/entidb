//! Error types for the sync server.

use thiserror::Error;

/// Result type for server operations.
pub type ServerResult<T> = Result<T, ServerError>;

/// Errors that can occur in the sync server.
#[derive(Error, Debug)]
pub enum ServerError {
    /// Invalid request format.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Authentication failed.
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Authorization failed.
    #[error("not authorized: {0}")]
    NotAuthorized(String),

    /// Database error.
    #[error("database error: {0}")]
    Database(String),

    /// Conflict during push.
    #[error("conflict: cursor mismatch, expected {expected}, got {actual}")]
    CursorConflict {
        /// Expected cursor.
        expected: u64,
        /// Actual cursor from client.
        actual: u64,
    },

    /// Invalid database ID.
    #[error("unknown database: {0:?}")]
    UnknownDatabase([u8; 16]),

    /// Protocol version mismatch.
    #[error("protocol version mismatch: {0}")]
    ProtocolMismatch(String),

    /// Internal server error.
    #[error("internal error: {0}")]
    Internal(String),

    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl ServerError {
    /// Returns true if this is a client error (4xx).
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            ServerError::InvalidRequest(_)
                | ServerError::AuthenticationFailed(_)
                | ServerError::NotAuthorized(_)
                | ServerError::CursorConflict { .. }
                | ServerError::UnknownDatabase(_)
                | ServerError::ProtocolMismatch(_)
        )
    }

    /// Returns true if this is a server error (5xx).
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            ServerError::Database(_) | ServerError::Internal(_) | ServerError::Io(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_classification() {
        assert!(ServerError::InvalidRequest("bad".into()).is_client_error());
        assert!(ServerError::Internal("oops".into()).is_server_error());
        assert!(!ServerError::InvalidRequest("bad".into()).is_server_error());
    }

    #[test]
    fn error_display() {
        let err = ServerError::CursorConflict {
            expected: 10,
            actual: 5,
        };
        let msg = err.to_string();
        assert!(msg.contains("10"));
        assert!(msg.contains("5"));
    }
}
