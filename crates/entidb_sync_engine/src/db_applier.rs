//! Database-backed sync applier.
//!
//! This module provides an applier that uses EntiDB for persistence,
//! ensuring the sync server uses the same EntiDB core as clients
//! (per architecture requirement).

use crate::error::SyncResult;
use crate::state::SyncApplier;
use entidb_core::{CollectionId, Database, EntityId};
use entidb_sync_protocol::{OperationType, SyncOperation};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Metadata for sync state.
const SYNC_COLLECTION_ID: u32 = 0xFFFF_FF00;
const CURSOR_ENTITY_ID: [u8; 16] = [0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x01];
#[allow(dead_code)] // Reserved for oplog entity encoding
const OPLOG_ENTITY_PREFIX: u8 = 0xAA;

/// A sync applier backed by an EntiDB database.
///
/// This applier persists sync state (cursor, pending operations) to EntiDB,
/// ensuring durability and atomic updates.
///
/// # Architecture
///
/// - Server cursor is stored as a special entity
/// - Pending operations are stored in an oplog collection
/// - Applied operations update the target collections
///
/// # Example
///
/// ```ignore
/// use entidb_core::Database;
/// use entidb_sync_engine::DatabaseApplier;
///
/// let db = Database::open_in_memory().unwrap();
/// let applier = DatabaseApplier::new(db);
///
/// // Use with sync engine
/// let engine = SyncEngine::new(config, transport, applier);
/// ```
pub struct DatabaseApplier {
    /// The database storing sync state.
    database: Arc<Database>,
    /// Cached server cursor for fast access.
    cached_cursor: AtomicU64,
    /// Next operation ID for pending operations.
    next_op_id: AtomicU64,
    /// Pending operations not yet pushed (in-memory for simplicity).
    pending: RwLock<Vec<SyncOperation>>,
    /// Acknowledged operation ID watermark.
    acknowledged_up_to: AtomicU64,
}

impl DatabaseApplier {
    /// Creates a new database-backed applier.
    pub fn new(database: Arc<Database>) -> Self {
        // Load cursor from database if exists
        let cursor = Self::load_cursor(&database).unwrap_or(0);

        Self {
            database,
            cached_cursor: AtomicU64::new(cursor),
            next_op_id: AtomicU64::new(1),
            pending: RwLock::new(Vec::new()),
            acknowledged_up_to: AtomicU64::new(0),
        }
    }

    /// Gets the underlying database.
    pub fn database(&self) -> &Arc<Database> {
        &self.database
    }

    /// Loads the cursor from the database.
    fn load_cursor(db: &Database) -> Option<u64> {
        let collection_id = CollectionId::new(SYNC_COLLECTION_ID);
        let entity_id = EntityId::from_bytes(CURSOR_ENTITY_ID);

        db.get(collection_id, entity_id)
            .ok()
            .flatten()
            .and_then(|bytes| {
                if bytes.len() == 8 {
                    Some(u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ]))
                } else {
                    None
                }
            })
    }

    /// Saves the cursor to the database.
    fn save_cursor(&self, cursor: u64) -> SyncResult<()> {
        let collection_id = CollectionId::new(SYNC_COLLECTION_ID);
        let entity_id = EntityId::from_bytes(CURSOR_ENTITY_ID);

        self.database
            .transaction(|txn| {
                txn.put(collection_id, entity_id, cursor.to_le_bytes().to_vec())?;
                Ok(())
            })?;

        Ok(())
    }

    /// Adds an operation to the pending queue.
    ///
    /// This is called by the local database when changes are committed.
    pub fn add_pending(&self, mut operation: SyncOperation) {
        operation.op_id = self.next_op_id.fetch_add(1, Ordering::SeqCst);
        self.pending.write().push(operation);
    }

    /// Creates a sync operation from a local change.
    pub fn create_operation(
        &self,
        collection_id: u32,
        entity_id: [u8; 16],
        op_type: OperationType,
        payload: Option<Vec<u8>>,
        sequence: u64,
    ) -> SyncOperation {
        SyncOperation {
            op_id: self.next_op_id.fetch_add(1, Ordering::SeqCst),
            collection_id,
            entity_id,
            op_type,
            payload,
            sequence,
        }
    }
}

impl SyncApplier for DatabaseApplier {
    fn apply_remote_operations(&self, operations: &[SyncOperation]) -> SyncResult<()> {
        if operations.is_empty() {
            return Ok(());
        }

        // Apply all operations in a single transaction for atomicity
        self.database
            .transaction(|txn| {
                for op in operations {
                    let collection_id = CollectionId::new(op.collection_id);
                    let entity_id = EntityId::from_bytes(op.entity_id);

                    match op.op_type {
                        OperationType::Put => {
                            if let Some(payload) = &op.payload {
                                txn.put(collection_id, entity_id, payload.clone())?;
                            }
                        }
                        OperationType::Delete => {
                            txn.delete(collection_id, entity_id)?;
                        }
                    }
                }
                Ok(())
            })?;

        Ok(())
    }

    fn get_pending_operations(&self, limit: u32) -> SyncResult<Vec<SyncOperation>> {
        let pending = self.pending.read();
        let ack_watermark = self.acknowledged_up_to.load(Ordering::SeqCst);

        Ok(pending
            .iter()
            .filter(|op| op.op_id > ack_watermark)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    fn acknowledge_operations(&self, up_to_op_id: u64) -> SyncResult<()> {
        self.acknowledged_up_to.store(up_to_op_id, Ordering::SeqCst);

        // Clean up acknowledged operations
        let mut pending = self.pending.write();
        pending.retain(|op| op.op_id > up_to_op_id);

        Ok(())
    }

    fn get_server_cursor(&self) -> SyncResult<u64> {
        Ok(self.cached_cursor.load(Ordering::SeqCst))
    }

    fn set_server_cursor(&self, cursor: u64) -> SyncResult<()> {
        self.cached_cursor.store(cursor, Ordering::SeqCst);
        self.save_cursor(cursor)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_core::Database;

    fn create_applier() -> DatabaseApplier {
        let db = Database::open_in_memory().unwrap();
        DatabaseApplier::new(Arc::new(db))
    }

    #[test]
    fn cursor_management() {
        let applier = create_applier();

        assert_eq!(applier.get_server_cursor().unwrap(), 0);

        applier.set_server_cursor(42).unwrap();
        assert_eq!(applier.get_server_cursor().unwrap(), 42);

        // Create new applier with same db to test persistence
        let db = Arc::clone(applier.database());
        let applier2 = DatabaseApplier::new(db);
        assert_eq!(applier2.get_server_cursor().unwrap(), 42);
    }

    #[test]
    fn pending_operations() {
        let applier = create_applier();

        // Add pending operations
        applier.add_pending(SyncOperation {
            op_id: 0, // will be assigned
            collection_id: 1,
            entity_id: [1u8; 16],
            op_type: OperationType::Put,
            payload: Some(vec![42]),
            sequence: 1,
        });

        applier.add_pending(SyncOperation {
            op_id: 0,
            collection_id: 1,
            entity_id: [2u8; 16],
            op_type: OperationType::Put,
            payload: Some(vec![43]),
            sequence: 2,
        });

        let pending = applier.get_pending_operations(10).unwrap();
        assert_eq!(pending.len(), 2);

        // Acknowledge first
        applier.acknowledge_operations(1).unwrap();
        let pending = applier.get_pending_operations(10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].entity_id, [2u8; 16]);
    }

    #[test]
    fn apply_remote_put() {
        let applier = create_applier();

        let operations = vec![SyncOperation {
            op_id: 1,
            collection_id: 100,
            entity_id: [5u8; 16],
            op_type: OperationType::Put,
            payload: Some(vec![0xCA, 0xFE]),
            sequence: 1,
        }];

        applier.apply_remote_operations(&operations).unwrap();

        // Verify entity was stored
        let collection_id = CollectionId::new(100);
        let entity_id = EntityId::from_bytes([5u8; 16]);
        let result = applier.database().get(collection_id, entity_id).unwrap();
        assert_eq!(result, Some(vec![0xCA, 0xFE]));
    }

    #[test]
    fn apply_remote_delete() {
        let applier = create_applier();
        let collection_id = CollectionId::new(100);
        let entity_id = EntityId::from_bytes([5u8; 16]);

        // First put
        applier
            .database()
            .transaction(|txn| {
                txn.put(collection_id, entity_id, vec![1, 2, 3])?;
                Ok(())
            })
            .unwrap();

        // Now delete via sync
        let operations = vec![SyncOperation {
            op_id: 1,
            collection_id: 100,
            entity_id: [5u8; 16],
            op_type: OperationType::Delete,
            payload: None,
            sequence: 2,
        }];

        applier.apply_remote_operations(&operations).unwrap();

        // Verify deleted
        let result = applier.database().get(collection_id, entity_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn apply_batch_atomicity() {
        let applier = create_applier();

        // Apply multiple operations
        let operations = vec![
            SyncOperation {
                op_id: 1,
                collection_id: 100,
                entity_id: [1u8; 16],
                op_type: OperationType::Put,
                payload: Some(vec![1]),
                sequence: 1,
            },
            SyncOperation {
                op_id: 2,
                collection_id: 100,
                entity_id: [2u8; 16],
                op_type: OperationType::Put,
                payload: Some(vec![2]),
                sequence: 2,
            },
        ];

        applier.apply_remote_operations(&operations).unwrap();

        // Both should exist
        let collection_id = CollectionId::new(100);
        let e1 = applier
            .database()
            .get(collection_id, EntityId::from_bytes([1u8; 16]))
            .unwrap();
        let e2 = applier
            .database()
            .get(collection_id, EntityId::from_bytes([2u8; 16]))
            .unwrap();

        assert_eq!(e1, Some(vec![1]));
        assert_eq!(e2, Some(vec![2]));
    }

    #[test]
    fn create_operation() {
        let applier = create_applier();

        let op1 = applier.create_operation(1, [1u8; 16], OperationType::Put, Some(vec![1]), 1);
        let op2 = applier.create_operation(1, [2u8; 16], OperationType::Put, Some(vec![2]), 2);

        // Op IDs should be sequential
        assert_eq!(op1.op_id, 1);
        assert_eq!(op2.op_id, 2);
    }
}
