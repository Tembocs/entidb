//! Request handlers for sync endpoints.

use crate::config::ServerConfig;
use crate::error::{ServerError, ServerResult};
use crate::oplog::ServerOplog;
use entidb_sync_protocol::{
    HandshakeRequest, HandshakeResponse, PullRequest, PullResponse, PushRequest, PushResponse,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Context for request handling.
pub struct HandlerContext {
    /// Server configuration.
    pub config: ServerConfig,
    /// Server oplog (shared across all handlers).
    pub oplog: Arc<ServerOplog>,
    /// Device sessions (device_id -> session info).
    sessions: RwLock<HashMap<[u8; 16], DeviceSession>>,
}

/// Information about a connected device.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used in session management
struct DeviceSession {
    /// Database ID.
    db_id: [u8; 16],
    /// Last known cursor.
    last_cursor: u64,
    /// Whether authenticated.
    authenticated: bool,
}

#[allow(dead_code)] // Session management methods for future use
impl HandlerContext {
    /// Creates a new handler context.
    pub fn new(config: ServerConfig, oplog: Arc<ServerOplog>) -> Self {
        Self {
            config,
            oplog,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a device session.
    fn register_session(&self, device_id: [u8; 16], db_id: [u8; 16], cursor: u64) {
        let session = DeviceSession {
            db_id,
            last_cursor: cursor,
            authenticated: true,
        };
        self.sessions.write().insert(device_id, session);
    }

    /// Gets a device session.
    fn get_session(&self, device_id: &[u8; 16]) -> Option<DeviceSession> {
        self.sessions.read().get(device_id).cloned()
    }

    /// Updates session cursor.
    fn update_cursor(&self, device_id: &[u8; 16], cursor: u64) {
        if let Some(session) = self.sessions.write().get_mut(device_id) {
            session.last_cursor = cursor;
        }
    }
}

/// Handler for sync requests.
pub struct RequestHandler {
    context: Arc<HandlerContext>,
}

impl RequestHandler {
    /// Creates a new request handler.
    pub fn new(context: Arc<HandlerContext>) -> Self {
        Self { context }
    }

    /// Handles a handshake request.
    pub fn handle_handshake(&self, request: HandshakeRequest) -> ServerResult<HandshakeResponse> {
        // Validate protocol version
        if request.protocol_version != 1 {
            return Ok(HandshakeResponse::error(format!(
                "Unsupported protocol version: {}",
                request.protocol_version
            )));
        }

        // Register the device session
        self.context
            .register_session(request.device_id, request.db_id, request.last_cursor);

        let server_cursor = self.context.oplog.cursor();
        Ok(HandshakeResponse::success(server_cursor))
    }

    /// Handles a pull request.
    pub fn handle_pull(&self, request: PullRequest) -> ServerResult<PullResponse> {
        let limit = request.limit.min(self.context.config.max_pull_batch);

        let operations = self.context.oplog.operations_since(request.cursor, limit);
        let has_more = self.context.oplog.has_more_after(request.cursor, limit);
        let new_cursor = operations
            .last()
            .map(|op| op.sequence)
            .unwrap_or(request.cursor);

        Ok(PullResponse::new(operations, new_cursor, has_more))
    }

    /// Handles a push request.
    pub fn handle_push(&self, request: PushRequest) -> ServerResult<PushResponse> {
        if request.operations.len() > self.context.config.max_push_batch as usize {
            return Err(ServerError::InvalidRequest(format!(
                "Too many operations: {} > {}",
                request.operations.len(),
                self.context.config.max_push_batch
            )));
        }

        match self
            .context
            .oplog
            .append(request.operations, request.expected_cursor)
        {
            Ok((new_cursor, conflicts)) => {
                if conflicts.is_empty() {
                    Ok(PushResponse::success(new_cursor))
                } else {
                    Ok(PushResponse::with_conflicts(new_cursor, conflicts))
                }
            }
            Err(ServerError::CursorConflict { expected, actual }) => {
                // Return a response indicating the cursor conflict
                Ok(PushResponse {
                    success: false,
                    new_cursor: expected,
                    conflicts: vec![],
                    error: Some(format!(
                        "Cursor conflict: expected {}, got {}",
                        expected, actual
                    )),
                })
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_sync_protocol::{OperationType, SyncOperation};

    fn make_op(collection_id: u32, entity_id: [u8; 16]) -> SyncOperation {
        SyncOperation {
            op_id: 0,
            collection_id,
            entity_id,
            op_type: OperationType::Put,
            payload: Some(vec![0x42]),
            sequence: 0,
        }
    }

    fn create_handler() -> RequestHandler {
        let config = ServerConfig::default();
        let oplog = Arc::new(ServerOplog::new());
        let context = Arc::new(HandlerContext::new(config, oplog));
        RequestHandler::new(context)
    }

    #[test]
    fn handshake_success() {
        let handler = create_handler();
        let request = HandshakeRequest::new([1u8; 16], [2u8; 16], 0);

        let response = handler.handle_handshake(request).unwrap();
        assert!(response.success);
        assert_eq!(response.server_cursor, 1);
    }

    #[test]
    fn handshake_bad_version() {
        let handler = create_handler();
        let request = HandshakeRequest {
            db_id: [1u8; 16],
            device_id: [2u8; 16],
            protocol_version: 99,
            last_cursor: 0,
        };

        let response = handler.handle_handshake(request).unwrap();
        assert!(!response.success);
        assert!(response.error.is_some());
    }

    #[test]
    fn pull_empty() {
        let handler = create_handler();
        let request = PullRequest::new(0, 10);

        let response = handler.handle_pull(request).unwrap();
        assert!(response.operations.is_empty());
        assert!(!response.has_more);
    }

    #[test]
    fn push_and_pull() {
        let handler = create_handler();

        // Push some operations
        let push_request = PushRequest::new(
            vec![make_op(1, [1u8; 16]), make_op(1, [2u8; 16])],
            1, // expected cursor
        );

        let push_response = handler.handle_push(push_request).unwrap();
        assert!(push_response.success);

        // Pull them back
        let pull_request = PullRequest::new(0, 10);
        let pull_response = handler.handle_pull(pull_request).unwrap();
        assert_eq!(pull_response.operations.len(), 2);
    }

    #[test]
    fn push_cursor_conflict() {
        let handler = create_handler();

        // Push with wrong cursor
        let request = PushRequest::new(vec![make_op(1, [1u8; 16])], 5);

        let response = handler.handle_push(request).unwrap();
        assert!(!response.success);
        assert!(response.error.is_some());
    }

    #[test]
    fn pull_pagination() {
        let handler = create_handler();

        // Push 5 operations
        let ops: Vec<_> = (0..5)
            .map(|i| make_op(1, [i as u8; 16]))
            .collect();
        handler
            .handle_push(PushRequest::new(ops, 1))
            .unwrap();

        // Pull with limit 2
        let response = handler.handle_pull(PullRequest::new(0, 2)).unwrap();
        assert_eq!(response.operations.len(), 2);
        assert!(response.has_more);

        // Pull next batch
        let response = handler
            .handle_pull(PullRequest::new(response.new_cursor, 2))
            .unwrap();
        assert_eq!(response.operations.len(), 2);
        assert!(response.has_more);

        // Pull final
        let response = handler
            .handle_pull(PullRequest::new(response.new_cursor, 2))
            .unwrap();
        assert_eq!(response.operations.len(), 1);
        assert!(!response.has_more);
    }
}
