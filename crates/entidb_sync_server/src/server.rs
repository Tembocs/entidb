//! Main sync server.

use crate::config::ServerConfig;
use crate::handler::{HandlerContext, RequestHandler};
use crate::oplog::ServerOplog;
use entidb_sync_protocol::{
    HandshakeRequest, HandshakeResponse, PullRequest, PullResponse, PushRequest, PushResponse,
    SyncMessage,
};
use std::sync::Arc;

/// The sync server.
///
/// This server handles synchronization requests from clients using
/// the EntiDB sync protocol. It maintains a server-side oplog and
/// processes handshake, pull, and push requests.
///
/// # Example
///
/// ```
/// use entidb_sync_server::{SyncServer, ServerConfig};
///
/// let config = ServerConfig::default();
/// let server = SyncServer::new(config);
///
/// // In a real application, you would expose HTTP endpoints
/// // that call server.handle_handshake(), handle_pull(), handle_push()
/// ```
pub struct SyncServer {
    handler: RequestHandler,
    context: Arc<HandlerContext>,
}

impl SyncServer {
    /// Creates a new sync server.
    pub fn new(config: ServerConfig) -> Self {
        let oplog = Arc::new(ServerOplog::new());
        let context = Arc::new(HandlerContext::new(config, oplog));
        let handler = RequestHandler::new(Arc::clone(&context));

        Self { handler, context }
    }

    /// Creates a sync server with an existing oplog.
    pub fn with_oplog(config: ServerConfig, oplog: Arc<ServerOplog>) -> Self {
        let context = Arc::new(HandlerContext::new(config, oplog));
        let handler = RequestHandler::new(Arc::clone(&context));

        Self { handler, context }
    }

    /// Handles a handshake request.
    pub fn handle_handshake(&self, request: HandshakeRequest) -> Result<HandshakeResponse, String> {
        self.handler
            .handle_handshake(request)
            .map_err(|e| e.to_string())
    }

    /// Handles a pull request.
    pub fn handle_pull(&self, request: PullRequest) -> Result<PullResponse, String> {
        self.handler.handle_pull(request).map_err(|e| e.to_string())
    }

    /// Handles a push request.
    pub fn handle_push(&self, request: PushRequest) -> Result<PushResponse, String> {
        self.handler.handle_push(request).map_err(|e| e.to_string())
    }

    /// Handles a sync message (dispatches to appropriate handler).
    pub fn handle_message(&self, message: SyncMessage) -> Result<SyncMessage, String> {
        match message {
            SyncMessage::HandshakeRequest(req) => self
                .handle_handshake(req)
                .map(SyncMessage::HandshakeResponse),
            SyncMessage::PullRequest(req) => self.handle_pull(req).map(SyncMessage::PullResponse),
            SyncMessage::PushRequest(req) => self.handle_push(req).map(SyncMessage::PushResponse),
            _ => Err("Unexpected message type".into()),
        }
    }

    /// Returns the current server cursor.
    pub fn cursor(&self) -> u64 {
        self.context.oplog.cursor()
    }

    /// Returns the number of operations in the oplog.
    pub fn operation_count(&self) -> usize {
        self.context.oplog.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_sync_protocol::{OperationType, SyncOperation};

    fn make_op(entity_id: [u8; 16]) -> SyncOperation {
        SyncOperation {
            op_id: 0,
            collection_id: 1,
            entity_id,
            op_type: OperationType::Put,
            payload: Some(vec![0x42]),
            sequence: 0,
        }
    }

    #[test]
    fn server_lifecycle() {
        let server = SyncServer::new(ServerConfig::default());
        assert_eq!(server.cursor(), 1);
        assert_eq!(server.operation_count(), 0);
    }

    #[test]
    fn full_sync_flow() {
        let server = SyncServer::new(ServerConfig::default());

        // 1. Handshake
        let handshake = HandshakeRequest::new([1u8; 16], [2u8; 16], 0);
        let response = server.handle_handshake(handshake).unwrap();
        assert!(response.success);
        let server_cursor = response.server_cursor;

        // 2. Pull (should be empty initially)
        let pull = PullRequest::new(0, 10);
        let response = server.handle_pull(pull).unwrap();
        assert!(response.operations.is_empty());

        // 3. Push some operations
        let push = PushRequest::new(vec![make_op([1u8; 16]), make_op([2u8; 16])], server_cursor);
        let response = server.handle_push(push).unwrap();
        assert!(response.success);
        assert_eq!(response.new_cursor, 3);

        // 4. Pull again (should get the pushed operations)
        let pull = PullRequest::new(0, 10);
        let response = server.handle_pull(pull).unwrap();
        assert_eq!(response.operations.len(), 2);
    }

    #[test]
    fn message_dispatch() {
        let server = SyncServer::new(ServerConfig::default());

        let message = SyncMessage::HandshakeRequest(HandshakeRequest::new([1u8; 16], [2u8; 16], 0));

        let response = server.handle_message(message).unwrap();
        assert!(matches!(response, SyncMessage::HandshakeResponse(_)));
    }

    #[test]
    fn shared_oplog() {
        let oplog = Arc::new(ServerOplog::new());
        let server = SyncServer::with_oplog(ServerConfig::default(), Arc::clone(&oplog));

        // Push via server
        let push = PushRequest::new(vec![make_op([1u8; 16])], 1);
        server.handle_push(push).unwrap();

        // Check oplog directly
        assert_eq!(oplog.len(), 1);
    }
}
