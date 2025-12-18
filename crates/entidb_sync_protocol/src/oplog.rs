//! Logical oplog for replication.

use crate::operation::SyncOperation;
use std::collections::VecDeque;

/// An entry in the logical oplog.
#[derive(Debug, Clone, PartialEq)]
pub struct OplogEntry {
    /// The operation.
    pub operation: SyncOperation,
    /// Whether this entry has been acknowledged by server.
    pub acknowledged: bool,
}

impl OplogEntry {
    /// Creates a new unacknowledged entry.
    pub fn new(operation: SyncOperation) -> Self {
        Self {
            operation,
            acknowledged: false,
        }
    }

    /// Marks this entry as acknowledged.
    pub fn acknowledge(&mut self) {
        self.acknowledged = true;
    }
}

/// A logical oplog for tracking operations to be synced.
///
/// The oplog maintains:
/// - Pending operations to push to server
/// - Last pushed operation ID
/// - Server cursor (last received from server)
///
/// # Invariants
///
/// - Operations are in commit order
/// - Only committed operations are added
/// - Acknowledged operations can be compacted
pub struct LogicalOplog {
    /// Pending entries.
    entries: VecDeque<OplogEntry>,
    /// Next operation ID.
    next_op_id: u64,
    /// Last acknowledged operation ID.
    last_acked_op_id: u64,
    /// Server cursor (last received from server).
    server_cursor: u64,
}

impl LogicalOplog {
    /// Creates a new empty oplog.
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            next_op_id: 1,
            last_acked_op_id: 0,
            server_cursor: 0,
        }
    }

    /// Creates an oplog from persisted state.
    pub fn from_state(next_op_id: u64, last_acked_op_id: u64, server_cursor: u64) -> Self {
        Self {
            entries: VecDeque::new(),
            next_op_id,
            last_acked_op_id,
            server_cursor,
        }
    }

    /// Appends an operation to the oplog.
    ///
    /// Returns the assigned operation ID.
    pub fn append(&mut self, mut operation: SyncOperation) -> u64 {
        let op_id = self.next_op_id;
        self.next_op_id += 1;
        operation.op_id = op_id;

        self.entries.push_back(OplogEntry::new(operation));
        op_id
    }

    /// Returns pending (unacknowledged) operations.
    pub fn pending(&self) -> impl Iterator<Item = &SyncOperation> {
        self.entries
            .iter()
            .filter(|e| !e.acknowledged)
            .map(|e| &e.operation)
    }

    /// Returns pending operations up to a limit.
    pub fn pending_batch(&self, limit: usize) -> Vec<&SyncOperation> {
        self.pending().take(limit).collect()
    }

    /// Returns the number of pending operations.
    pub fn pending_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.acknowledged).count()
    }

    /// Acknowledges operations up to the given ID.
    pub fn acknowledge_up_to(&mut self, op_id: u64) {
        for entry in &mut self.entries {
            if entry.operation.op_id <= op_id {
                entry.acknowledged = true;
            }
        }
        self.last_acked_op_id = self.last_acked_op_id.max(op_id);
    }

    /// Updates the server cursor.
    pub fn set_server_cursor(&mut self, cursor: u64) {
        self.server_cursor = cursor;
    }

    /// Returns the server cursor.
    pub fn server_cursor(&self) -> u64 {
        self.server_cursor
    }

    /// Returns the last acknowledged operation ID.
    pub fn last_acked_op_id(&self) -> u64 {
        self.last_acked_op_id
    }

    /// Returns the next operation ID.
    pub fn next_op_id(&self) -> u64 {
        self.next_op_id
    }

    /// Compacts acknowledged entries.
    ///
    /// Removes entries that have been acknowledged and are
    /// before the given op_id.
    pub fn compact(&mut self) {
        while let Some(entry) = self.entries.front() {
            if entry.acknowledged {
                self.entries.pop_front();
            } else {
                break;
            }
        }
    }

    /// Returns the total number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the oplog is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for LogicalOplog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationType;

    fn make_op(collection_id: u32, entity_id: u8) -> SyncOperation {
        SyncOperation {
            op_id: 0, // Will be assigned
            collection_id,
            entity_id: [entity_id; 16],
            op_type: OperationType::Put,
            payload: Some(vec![1, 2, 3]),
            sequence: 1,
        }
    }

    #[test]
    fn append_assigns_op_id() {
        let mut oplog = LogicalOplog::new();

        let id1 = oplog.append(make_op(1, 1));
        let id2 = oplog.append(make_op(1, 2));
        let id3 = oplog.append(make_op(1, 3));

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[test]
    fn pending_operations() {
        let mut oplog = LogicalOplog::new();

        oplog.append(make_op(1, 1));
        oplog.append(make_op(1, 2));
        oplog.append(make_op(1, 3));

        let pending: Vec<_> = oplog.pending().collect();
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn acknowledge_up_to() {
        let mut oplog = LogicalOplog::new();

        oplog.append(make_op(1, 1));
        oplog.append(make_op(1, 2));
        oplog.append(make_op(1, 3));

        oplog.acknowledge_up_to(2);

        assert_eq!(oplog.pending_count(), 1);
        assert_eq!(oplog.last_acked_op_id(), 2);
    }

    #[test]
    fn compact() {
        let mut oplog = LogicalOplog::new();

        oplog.append(make_op(1, 1));
        oplog.append(make_op(1, 2));
        oplog.append(make_op(1, 3));

        assert_eq!(oplog.len(), 3);

        oplog.acknowledge_up_to(2);
        oplog.compact();

        assert_eq!(oplog.len(), 1); // Only op 3 remains
    }

    #[test]
    fn pending_batch() {
        let mut oplog = LogicalOplog::new();

        for i in 0..10 {
            oplog.append(make_op(1, i));
        }

        let batch = oplog.pending_batch(5);
        assert_eq!(batch.len(), 5);
        assert_eq!(batch[0].op_id, 1);
        assert_eq!(batch[4].op_id, 5);
    }

    #[test]
    fn server_cursor() {
        let mut oplog = LogicalOplog::new();

        assert_eq!(oplog.server_cursor(), 0);

        oplog.set_server_cursor(100);
        assert_eq!(oplog.server_cursor(), 100);
    }

    #[test]
    fn from_state() {
        let oplog = LogicalOplog::from_state(50, 40, 1000);

        assert_eq!(oplog.next_op_id(), 50);
        assert_eq!(oplog.last_acked_op_id(), 40);
        assert_eq!(oplog.server_cursor(), 1000);
    }
}
