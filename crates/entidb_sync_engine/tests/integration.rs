//! Integration tests for sync engine and server.

use entidb_sync_engine::{
    DatabaseApplier, MemorySyncApplier, SyncConfig, SyncEngine, SyncResult, SyncTransport,
};
use entidb_sync_server::{ServerConfig, ServerOplog, SyncServer};
use entidb_sync_protocol::{
    HandshakeRequest, HandshakeResponse, OperationType, PullRequest, PullResponse, PushRequest,
    PushResponse, SyncOperation,
};
use std::sync::Arc;

/// A transport that connects to an in-memory server.
struct InMemoryTransport {
    server: Arc<SyncServer>,
}

impl InMemoryTransport {
    fn new(server: Arc<SyncServer>) -> Self {
        Self { server }
    }
}

impl SyncTransport for InMemoryTransport {
    fn handshake(
        &self,
        request: &HandshakeRequest,
    ) -> SyncResult<HandshakeResponse> {
        self.server
            .handle_handshake(request.clone())
            .map_err(|e| entidb_sync_engine::SyncError::ServerError(e))
    }

    fn pull(&self, request: &PullRequest) -> SyncResult<PullResponse> {
        self.server
            .handle_pull(request.clone())
            .map_err(|e| entidb_sync_engine::SyncError::ServerError(e))
    }

    fn push(&self, request: &PushRequest) -> SyncResult<PushResponse> {
        self.server
            .handle_push(request.clone())
            .map_err(|e| entidb_sync_engine::SyncError::ServerError(e))
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn close(&self) -> SyncResult<()> {
        Ok(())
    }
}

fn make_op(op_id: u64, entity_id: [u8; 16]) -> SyncOperation {
    SyncOperation {
        op_id,
        collection_id: 1,
        entity_id,
        op_type: OperationType::Put,
        payload: Some(vec![0x42, op_id as u8]),
        sequence: op_id,
    }
}

#[test]
fn client_server_full_sync() {
    // Create server
    let server = Arc::new(SyncServer::new(ServerConfig::default()));

    // Create client with in-memory transport to server
    let config = SyncConfig::new([1u8; 16], [2u8; 16], "memory://");
    let transport = InMemoryTransport::new(Arc::clone(&server));
    let applier = MemorySyncApplier::new();

    // Add some pending operations on the client
    applier.add_pending(make_op(0, [1u8; 16]));
    applier.add_pending(make_op(0, [2u8; 16]));

    let engine = SyncEngine::new(config, transport, applier);

    // Run sync
    let result = engine.sync().unwrap();
    assert!(result.success);
    assert_eq!(result.pushed, 2);
    assert_eq!(result.pulled, 0);

    // Server should now have 2 operations
    assert_eq!(server.operation_count(), 2);
}

#[test]
fn bidirectional_sync() {
    // Create server with some existing data
    let server_oplog = Arc::new(ServerOplog::new());
    let server = Arc::new(SyncServer::with_oplog(
        ServerConfig::default(),
        Arc::clone(&server_oplog),
    ));

    // Simulate another client pushing data to the server first
    server
        .handle_push(PushRequest::new(
            vec![make_op(0, [100u8; 16]), make_op(0, [101u8; 16])],
            1,
        ))
        .unwrap();

    // Now our client syncs
    let config = SyncConfig::new([1u8; 16], [2u8; 16], "memory://");
    let transport = InMemoryTransport::new(Arc::clone(&server));
    let applier = MemorySyncApplier::new();

    // Client has its own pending operations
    applier.add_pending(make_op(0, [1u8; 16]));

    let engine = SyncEngine::new(config, transport, applier);

    // Run sync
    let result = engine.sync().unwrap();
    assert!(result.success);

    // Client should pull the 2 operations from server, push 1
    assert_eq!(result.pulled, 2);
    assert_eq!(result.pushed, 1);

    // Server should now have 3 operations total
    assert_eq!(server.operation_count(), 3);
}

#[test]
fn empty_sync() {
    let server = Arc::new(SyncServer::new(ServerConfig::default()));
    let config = SyncConfig::new([1u8; 16], [2u8; 16], "memory://");
    let transport = InMemoryTransport::new(server);
    let applier = MemorySyncApplier::new();

    let engine = SyncEngine::new(config, transport, applier);

    // Sync with nothing to push or pull
    let result = engine.sync().unwrap();
    assert!(result.success);
    assert_eq!(result.pushed, 0);
    assert_eq!(result.pulled, 0);
}

#[test]
fn database_applier_sync() {
    // This test uses the DatabaseApplier backed by EntiDB
    // to verify the architecture requirement that sync server
    // uses the same EntiDB core as clients.

    let server = Arc::new(SyncServer::new(ServerConfig::default()));
    let config = SyncConfig::new([1u8; 16], [2u8; 16], "memory://");
    let transport = InMemoryTransport::new(Arc::clone(&server));

    // Use DatabaseApplier backed by an in-memory EntiDB database
    let db = entidb_core::Database::open_in_memory().unwrap();
    let applier = DatabaseApplier::new(Arc::new(db));

    // Add some pending operations
    applier.add_pending(SyncOperation {
        op_id: 0,
        collection_id: 100,
        entity_id: [1u8; 16],
        op_type: OperationType::Put,
        payload: Some(vec![0xCA, 0xFE]),
        sequence: 1,
    });

    let engine = SyncEngine::new(config, transport, applier);

    // Run sync
    let result = engine.sync().unwrap();
    assert!(result.success);
    assert_eq!(result.pushed, 1);
    assert_eq!(result.pulled, 0);

    // Server should have the operation
    assert_eq!(server.operation_count(), 1);
}

#[test]
fn database_applier_pull_persists() {
    // Test that pulling operations via DatabaseApplier persists them to EntiDB

    // Server has existing data
    let server_oplog = Arc::new(ServerOplog::new());
    let server = Arc::new(SyncServer::with_oplog(
        ServerConfig::default(),
        Arc::clone(&server_oplog),
    ));

    // Another client pushed some data
    server
        .handle_push(PushRequest::new(
            vec![SyncOperation {
                op_id: 0,
                collection_id: 200,
                entity_id: [42u8; 16],
                op_type: OperationType::Put,
                payload: Some(vec![1, 2, 3, 4]),
                sequence: 1,
            }],
            1,
        ))
        .unwrap();

    // Our client with DatabaseApplier
    let config = SyncConfig::new([1u8; 16], [2u8; 16], "memory://");
    let transport = InMemoryTransport::new(Arc::clone(&server));

    let db = entidb_core::Database::open_in_memory().unwrap();
    let db_arc = Arc::new(db);
    let applier = DatabaseApplier::new(Arc::clone(&db_arc));

    let engine = SyncEngine::new(config, transport, applier);

    // Run sync - should pull the remote operation
    let result = engine.sync().unwrap();
    assert!(result.success);
    assert_eq!(result.pulled, 1);

    // Verify the entity was persisted to the database
    let collection_id = entidb_core::CollectionId::new(200);
    let entity_id = entidb_core::EntityId::from_bytes([42u8; 16]);
    let data = db_arc.get(collection_id, entity_id).unwrap();
    assert_eq!(data, Some(vec![1, 2, 3, 4]));
}
