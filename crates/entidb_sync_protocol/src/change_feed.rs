//! Change feed for emitting committed operations.

use crate::operation::SyncOperation;

/// Type of change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// Entity was inserted.
    Insert,
    /// Entity was updated.
    Update,
    /// Entity was deleted.
    Delete,
}

/// A single change event from the change feed.
///
/// Change events are emitted only after a transaction commits.
/// They represent the logical changes made to the database.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeEvent {
    /// Sequence number of the commit.
    pub sequence: u64,
    /// Collection ID.
    pub collection_id: u32,
    /// Entity ID.
    pub entity_id: [u8; 16],
    /// Type of change.
    pub change_type: ChangeType,
    /// New payload (for Insert/Update).
    pub after_bytes: Option<Vec<u8>>,
    /// Previous payload hash (for Update/Delete).
    pub before_hash: Option<[u8; 32]>,
}

impl ChangeEvent {
    /// Creates an insert event.
    pub fn insert(
        sequence: u64,
        collection_id: u32,
        entity_id: [u8; 16],
        payload: Vec<u8>,
    ) -> Self {
        Self {
            sequence,
            collection_id,
            entity_id,
            change_type: ChangeType::Insert,
            after_bytes: Some(payload),
            before_hash: None,
        }
    }

    /// Creates an update event.
    pub fn update(
        sequence: u64,
        collection_id: u32,
        entity_id: [u8; 16],
        payload: Vec<u8>,
        before_hash: [u8; 32],
    ) -> Self {
        Self {
            sequence,
            collection_id,
            entity_id,
            change_type: ChangeType::Update,
            after_bytes: Some(payload),
            before_hash: Some(before_hash),
        }
    }

    /// Creates a delete event.
    pub fn delete(
        sequence: u64,
        collection_id: u32,
        entity_id: [u8; 16],
        before_hash: [u8; 32],
    ) -> Self {
        Self {
            sequence,
            collection_id,
            entity_id,
            change_type: ChangeType::Delete,
            after_bytes: None,
            before_hash: Some(before_hash),
        }
    }

    /// Converts to a sync operation.
    pub fn to_sync_operation(&self, op_id: u64) -> SyncOperation {
        match self.change_type {
            ChangeType::Insert | ChangeType::Update => SyncOperation::put(
                op_id,
                self.collection_id,
                self.entity_id,
                self.after_bytes.clone().unwrap_or_default(),
                self.sequence,
            ),
            ChangeType::Delete => {
                SyncOperation::delete(op_id, self.collection_id, self.entity_id, self.sequence)
            }
        }
    }
}

/// A change feed that emits committed operations.
///
/// The change feed:
/// - Emits only committed operations
/// - Preserves commit order
/// - Can be polled from a cursor position
///
/// # Example
///
/// ```rust,ignore
/// let feed = ChangeFeed::new();
///
/// // After a commit
/// feed.emit(ChangeEvent::insert(...));
///
/// // Consumer polls from cursor
/// let events = feed.poll(last_cursor, 100);
/// ```
pub struct ChangeFeed {
    /// Events in commit order.
    events: Vec<ChangeEvent>,
    /// Next sequence number.
    next_sequence: u64,
}

impl ChangeFeed {
    /// Creates a new empty change feed.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_sequence: 1,
        }
    }

    /// Creates a change feed starting from a sequence number.
    pub fn from_sequence(sequence: u64) -> Self {
        Self {
            events: Vec::new(),
            next_sequence: sequence,
        }
    }

    /// Emits a change event.
    ///
    /// Events are assigned monotonically increasing sequence numbers.
    pub fn emit(&mut self, mut event: ChangeEvent) {
        event.sequence = self.next_sequence;
        self.next_sequence += 1;
        self.events.push(event);
    }

    /// Emits multiple events from a single commit.
    ///
    /// All events in a commit get the same sequence number.
    pub fn emit_batch(&mut self, events: Vec<ChangeEvent>) {
        let seq = self.next_sequence;
        self.next_sequence += 1;

        for mut event in events {
            event.sequence = seq;
            self.events.push(event);
        }
    }

    /// Polls events from a cursor position.
    ///
    /// Returns events with sequence > cursor, up to limit.
    pub fn poll(&self, cursor: u64, limit: usize) -> Vec<&ChangeEvent> {
        self.events
            .iter()
            .filter(|e| e.sequence > cursor)
            .take(limit)
            .collect()
    }

    /// Returns the latest sequence number.
    pub fn latest_sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    /// Returns the total number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if the feed is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Truncates events older than the given sequence.
    ///
    /// This is used for compaction when events have been replicated.
    pub fn truncate_before(&mut self, sequence: u64) {
        self.events.retain(|e| e.sequence >= sequence);
    }

    /// Clears all events.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for ChangeFeed {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationType;

    #[test]
    fn emit_and_poll() {
        let mut feed = ChangeFeed::new();

        feed.emit(ChangeEvent::insert(0, 1, [1u8; 16], vec![1, 2, 3]));
        feed.emit(ChangeEvent::insert(0, 1, [2u8; 16], vec![4, 5, 6]));

        let events = feed.poll(0, 10);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[1].sequence, 2);
    }

    #[test]
    fn poll_from_cursor() {
        let mut feed = ChangeFeed::new();

        for i in 0..5 {
            feed.emit(ChangeEvent::insert(0, 1, [i; 16], vec![i]));
        }

        // Poll from cursor 2 (get events 3, 4, 5)
        let events = feed.poll(2, 10);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 3);
    }

    #[test]
    fn poll_with_limit() {
        let mut feed = ChangeFeed::new();

        for i in 0..10 {
            feed.emit(ChangeEvent::insert(0, 1, [i; 16], vec![i]));
        }

        let events = feed.poll(0, 3);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn latest_sequence() {
        let mut feed = ChangeFeed::new();
        assert_eq!(feed.latest_sequence(), 0);

        feed.emit(ChangeEvent::insert(0, 1, [0u8; 16], vec![]));
        assert_eq!(feed.latest_sequence(), 1);

        feed.emit(ChangeEvent::insert(0, 1, [1u8; 16], vec![]));
        assert_eq!(feed.latest_sequence(), 2);
    }

    #[test]
    fn emit_batch_same_sequence() {
        let mut feed = ChangeFeed::new();

        let events = vec![
            ChangeEvent::insert(0, 1, [0u8; 16], vec![1]),
            ChangeEvent::insert(0, 1, [1u8; 16], vec![2]),
            ChangeEvent::insert(0, 1, [2u8; 16], vec![3]),
        ];

        feed.emit_batch(events);

        // All events should have the same sequence
        let polled = feed.poll(0, 10);
        assert_eq!(polled.len(), 3);
        assert!(polled.iter().all(|e| e.sequence == 1));
    }

    #[test]
    fn truncate_before() {
        let mut feed = ChangeFeed::new();

        for i in 0..5 {
            feed.emit(ChangeEvent::insert(0, 1, [i; 16], vec![i]));
        }

        assert_eq!(feed.len(), 5);

        feed.truncate_before(3);
        assert_eq!(feed.len(), 3); // sequences 3, 4, 5
    }

    #[test]
    fn change_event_to_sync_operation() {
        let event = ChangeEvent::insert(5, 100, [7u8; 16], vec![1, 2, 3]);
        let op = event.to_sync_operation(42);

        assert_eq!(op.op_id, 42);
        assert_eq!(op.collection_id, 100);
        assert_eq!(op.entity_id, [7u8; 16]);
        assert_eq!(op.op_type, OperationType::Put);
        assert_eq!(op.payload, Some(vec![1, 2, 3]));
        assert_eq!(op.sequence, 5);
    }

    #[test]
    fn delete_event() {
        let hash = [0xABu8; 32];
        let event = ChangeEvent::delete(10, 50, [3u8; 16], hash);

        assert_eq!(event.change_type, ChangeType::Delete);
        assert_eq!(event.before_hash, Some(hash));
        assert!(event.after_bytes.is_none());
    }
}
