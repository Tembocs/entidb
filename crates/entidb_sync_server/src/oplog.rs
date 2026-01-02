//! Server-side oplog management.

use crate::error::{ServerError, ServerResult};
use entidb_sync_protocol::{Conflict, ConflictPolicy, SyncOperation};
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Server-side operation log.
///
/// The server oplog maintains:
/// - All operations in commit order
/// - Current cursor (highest operation ID)
/// - Entity states for conflict detection
///
/// # Durability
///
/// **WARNING:** This is currently an in-memory implementation suitable for
/// development and testing. For production use, the sync server should be
/// backed by `entidb_core` to provide durable persistence of the oplog and
/// entity state. See `docs/sync_protocol.md` for the recommended production
/// architecture where the sync server uses the same EntiDB core as clients.
///
/// # Conflict Detection
///
/// Uses SHA-256 hashing over canonical CBOR bytes for cryptographically
/// secure conflict detection.
pub struct ServerOplog {
    /// Operations in commit order.
    operations: RwLock<Vec<SyncOperation>>,
    /// Current cursor (next operation ID).
    next_cursor: RwLock<u64>,
    /// Latest version of each entity (collection_id, entity_id) -> (sequence, payload_hash).
    entity_versions: RwLock<HashMap<(u32, [u8; 16]), (u64, Option<[u8; 32]>)>>,
    /// Conflict policy.
    conflict_policy: ConflictPolicy,
}

impl ServerOplog {
    /// Creates a new empty oplog.
    pub fn new() -> Self {
        Self {
            operations: RwLock::new(Vec::new()),
            next_cursor: RwLock::new(1),
            entity_versions: RwLock::new(HashMap::new()),
            conflict_policy: ConflictPolicy::ServerWins,
        }
    }

    /// Creates an oplog with initial cursor.
    pub fn with_cursor(cursor: u64) -> Self {
        Self {
            operations: RwLock::new(Vec::new()),
            next_cursor: RwLock::new(cursor),
            entity_versions: RwLock::new(HashMap::new()),
            conflict_policy: ConflictPolicy::ServerWins,
        }
    }

    /// Returns the current cursor.
    pub fn cursor(&self) -> u64 {
        *self.next_cursor.read()
    }

    /// Returns operations since a given cursor.
    pub fn operations_since(&self, cursor: u64, limit: u32) -> Vec<SyncOperation> {
        let ops = self.operations.read();
        ops.iter()
            .filter(|op| op.sequence > cursor)
            .take(limit as usize)
            .cloned()
            .collect()
    }

    /// Returns true if there are more operations after the given cursor + limit.
    pub fn has_more_after(&self, cursor: u64, limit: u32) -> bool {
        let ops = self.operations.read();
        let count = ops.iter().filter(|op| op.sequence > cursor).count();
        count > limit as usize
    }

    /// Appends operations from a client.
    ///
    /// Returns conflicts if any operations conflict with server state.
    pub fn append(
        &self,
        operations: Vec<SyncOperation>,
        expected_cursor: u64,
    ) -> ServerResult<(u64, Vec<Conflict>)> {
        let current_cursor = self.cursor();

        // Check for cursor conflict
        if expected_cursor != current_cursor {
            return Err(ServerError::CursorConflict {
                expected: current_cursor,
                actual: expected_cursor,
            });
        }

        let mut conflicts = Vec::new();
        let mut accepted = Vec::new();
        let mut versions = self.entity_versions.write();
        let mut next = self.next_cursor.write();

        for mut op in operations {
            let key = (op.collection_id, op.entity_id);

            // Check for conflicts
            if let Some((server_seq, _server_hash)) = versions.get(&key) {
                // Entity exists on server
                if op.sequence <= *server_seq {
                    // Client has older version - conflict
                    let conflict = self.create_conflict(&op, &key, &versions);
                    conflicts.push(conflict);
                    continue;
                }
            }

            // Accept operation
            op.sequence = *next;
            *next += 1;

            // Update version tracking
            let payload_hash = op.payload.as_ref().map(|p| Self::compute_hash(p));
            versions.insert(key, (op.sequence, payload_hash));

            accepted.push(op);
        }

        // Append accepted operations
        let mut ops = self.operations.write();
        ops.extend(accepted);

        Ok((*next, conflicts))
    }

    /// Creates a conflict from an operation.
    fn create_conflict(
        &self,
        client_op: &SyncOperation,
        key: &(u32, [u8; 16]),
        _versions: &HashMap<(u32, [u8; 16]), (u64, Option<[u8; 32]>)>,
    ) -> Conflict {
        // Find server's current payload for this entity
        let ops = self.operations.read();
        let server_payload = ops
            .iter()
            .rev()
            .find(|op| op.collection_id == key.0 && op.entity_id == key.1)
            .and_then(|op| op.payload.clone());

        Conflict::new(
            key.0,
            key.1,
            client_op.payload.as_ref().map(|p| Self::compute_hash(p)),
            server_payload.as_ref().map(|p| Self::compute_hash(p)),
            client_op.payload.clone(),
            server_payload,
        )
    }

    /// Resolves conflicts using the server's policy.
    pub fn resolve_conflicts(&self, conflicts: &mut [Conflict]) {
        for conflict in conflicts {
            self.conflict_policy.resolve(conflict);
        }
    }

    /// Computes a cryptographic SHA-256 hash for conflict detection.
    ///
    /// Uses SHA-256 over canonical CBOR bytes to ensure proper conflict
    /// detection that is resistant to collisions.
    fn compute_hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Returns the number of operations.
    pub fn len(&self) -> usize {
        self.operations.read().len()
    }

    /// Returns true if the oplog is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.read().is_empty()
    }

    /// Clears all operations (for testing).
    #[cfg(test)]
    pub fn clear(&self) {
        self.operations.write().clear();
        self.entity_versions.write().clear();
        *self.next_cursor.write() = 1;
    }
}

impl Default for ServerOplog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_sync_protocol::OperationType;

    fn make_op(collection_id: u32, entity_id: [u8; 16], sequence: u64) -> SyncOperation {
        SyncOperation {
            op_id: sequence,
            collection_id,
            entity_id,
            op_type: OperationType::Put,
            payload: Some(vec![0x42]),
            sequence,
        }
    }

    #[test]
    fn empty_oplog() {
        let oplog = ServerOplog::new();
        assert_eq!(oplog.cursor(), 1);
        assert!(oplog.is_empty());
        assert_eq!(oplog.operations_since(0, 10).len(), 0);
    }

    #[test]
    fn append_operations() {
        let oplog = ServerOplog::new();
        let ops = vec![make_op(1, [1u8; 16], 0), make_op(1, [2u8; 16], 0)];

        let (new_cursor, conflicts) = oplog.append(ops, 1).unwrap();
        assert_eq!(new_cursor, 3);
        assert!(conflicts.is_empty());
        assert_eq!(oplog.len(), 2);
    }

    #[test]
    fn cursor_conflict() {
        let oplog = ServerOplog::new();
        let ops = vec![make_op(1, [1u8; 16], 0)];

        // Append with wrong expected cursor
        let result = oplog.append(ops, 5);
        assert!(result.is_err());
    }

    #[test]
    fn operations_since_cursor() {
        let oplog = ServerOplog::new();
        let ops = vec![
            make_op(1, [1u8; 16], 0),
            make_op(1, [2u8; 16], 0),
            make_op(1, [3u8; 16], 0),
        ];

        oplog.append(ops, 1).unwrap();

        // Get all since 0
        let all = oplog.operations_since(0, 10);
        assert_eq!(all.len(), 3);

        // Get since cursor 2
        let some = oplog.operations_since(2, 10);
        assert_eq!(some.len(), 1);
    }

    #[test]
    fn has_more() {
        let oplog = ServerOplog::new();
        let ops = vec![
            make_op(1, [1u8; 16], 0),
            make_op(1, [2u8; 16], 0),
            make_op(1, [3u8; 16], 0),
        ];

        oplog.append(ops, 1).unwrap();

        assert!(oplog.has_more_after(0, 2));
        assert!(!oplog.has_more_after(0, 10));
    }
}
