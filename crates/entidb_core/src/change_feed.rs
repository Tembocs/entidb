//! Change feed for observing committed operations.
//!
//! The change feed emits events for all committed database operations,
//! enabling:
//! - Sync layer integration
//! - Reactive UI updates
//! - Audit logging
//!
//! # Usage
//!
//! ```rust,ignore
//! use entidb_core::Database;
//!
//! let db = Database::open_in_memory()?;
//! let users = db.collection("users");
//!
//! // Subscribe to changes
//! let receiver = db.subscribe();
//!
//! // In another thread, listen for changes
//! std::thread::spawn(move || {
//!     while let Ok(event) = receiver.recv() {
//!         println!("Change: {:?}", event);
//!     }
//! });
//!
//! // Changes are emitted after commit
//! db.put(&users, EntityId::new(), vec![1, 2, 3]);
//! ```

use parking_lot::RwLock;
use std::sync::mpsc::{self, Receiver, Sender};

/// Type of change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// Entity was inserted (no previous version existed).
    Insert,
    /// Entity was updated (previous version existed).
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
    /// Entity ID (16 bytes).
    pub entity_id: [u8; 16],
    /// Type of change.
    pub change_type: ChangeType,
    /// New payload (for Insert/Update). None for Delete.
    pub payload: Option<Vec<u8>>,
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
            payload: Some(payload),
        }
    }

    /// Creates an update event.
    pub fn update(
        sequence: u64,
        collection_id: u32,
        entity_id: [u8; 16],
        payload: Vec<u8>,
    ) -> Self {
        Self {
            sequence,
            collection_id,
            entity_id,
            change_type: ChangeType::Update,
            payload: Some(payload),
        }
    }

    /// Creates a delete event.
    pub fn delete(sequence: u64, collection_id: u32, entity_id: [u8; 16]) -> Self {
        Self {
            sequence,
            collection_id,
            entity_id,
            change_type: ChangeType::Delete,
            payload: None,
        }
    }
}

/// A change feed that distributes committed operations to subscribers.
///
/// The change feed:
/// - Emits only committed operations
/// - Preserves commit order
/// - Supports multiple subscribers
/// - Is thread-safe
pub struct ChangeFeed {
    /// Subscribers (senders).
    subscribers: RwLock<Vec<Sender<ChangeEvent>>>,
    /// History of recent events for polling.
    history: RwLock<Vec<ChangeEvent>>,
    /// Maximum history size.
    max_history: usize,
}

impl ChangeFeed {
    /// Creates a new change feed.
    pub fn new() -> Self {
        Self::with_max_history(10000)
    }

    /// Creates a change feed with a specific history limit.
    pub fn with_max_history(max_history: usize) -> Self {
        Self {
            subscribers: RwLock::new(Vec::new()),
            history: RwLock::new(Vec::new()),
            max_history,
        }
    }

    /// Subscribes to the change feed.
    ///
    /// Returns a receiver that will receive all future change events.
    /// The receiver should be polled regularly to avoid unbounded memory growth.
    pub fn subscribe(&self) -> Receiver<ChangeEvent> {
        let (tx, rx) = mpsc::channel();
        self.subscribers.write().push(tx);
        rx
    }

    /// Emits a change event to all subscribers.
    ///
    /// This is called by the transaction manager after commit.
    /// Events are cloned to each active subscriber.
    pub fn emit(&self, event: ChangeEvent) {
        // Add to history
        {
            let mut history = self.history.write();
            history.push(event.clone());
            // Trim history if needed
            if history.len() > self.max_history {
                let to_remove = history.len() - self.max_history;
                history.drain(0..to_remove);
            }
        }

        // Send to subscribers (remove disconnected ones)
        let mut subscribers = self.subscribers.write();
        subscribers.retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Emits multiple events from a single commit.
    pub fn emit_batch(&self, events: Vec<ChangeEvent>) {
        for event in events {
            self.emit(event);
        }
    }

    /// Polls events from a sequence cursor.
    ///
    /// Returns events with sequence > cursor, up to limit.
    /// This is useful for catch-up scenarios.
    pub fn poll(&self, cursor: u64, limit: usize) -> Vec<ChangeEvent> {
        let history = self.history.read();
        history
            .iter()
            .filter(|e| e.sequence > cursor)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Returns the latest sequence number in history.
    pub fn latest_sequence(&self) -> u64 {
        self.history.read().last().map(|e| e.sequence).unwrap_or(0)
    }

    /// Returns the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.read().len()
    }

    /// Returns the number of events in history.
    pub fn history_len(&self) -> usize {
        self.history.read().len()
    }

    /// Clears history older than the given sequence.
    pub fn truncate_history(&self, min_sequence: u64) {
        let mut history = self.history.write();
        history.retain(|e| e.sequence >= min_sequence);
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
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn emit_and_receive() {
        let feed = ChangeFeed::new();
        let rx = feed.subscribe();

        let event = ChangeEvent::insert(1, 1, [0u8; 16], vec![1, 2, 3]);
        feed.emit(event.clone());

        let received = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(received, event);
    }

    #[test]
    fn multiple_subscribers() {
        let feed = ChangeFeed::new();
        let rx1 = feed.subscribe();
        let rx2 = feed.subscribe();

        let event = ChangeEvent::insert(1, 1, [0u8; 16], vec![1]);
        feed.emit(event.clone());

        assert_eq!(rx1.recv().unwrap(), event);
        assert_eq!(rx2.recv().unwrap(), event);
    }

    #[test]
    fn subscriber_cleanup() {
        let feed = ChangeFeed::new();
        assert_eq!(feed.subscriber_count(), 0);

        let rx = feed.subscribe();
        assert_eq!(feed.subscriber_count(), 1);

        // Drop receiver
        drop(rx);

        // Emit - should clean up disconnected subscriber
        feed.emit(ChangeEvent::insert(1, 1, [0u8; 16], vec![]));
        assert_eq!(feed.subscriber_count(), 0);
    }

    #[test]
    fn poll_from_cursor() {
        let feed = ChangeFeed::new();

        for i in 1..=5 {
            feed.emit(ChangeEvent::insert(i, 1, [i as u8; 16], vec![i as u8]));
        }

        // Poll from cursor 2 (get events 3, 4, 5)
        let events = feed.poll(2, 10);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 3);
        assert_eq!(events[1].sequence, 4);
        assert_eq!(events[2].sequence, 5);
    }

    #[test]
    fn poll_with_limit() {
        let feed = ChangeFeed::new();

        for i in 1..=10 {
            feed.emit(ChangeEvent::insert(i, 1, [i as u8; 16], vec![]));
        }

        let events = feed.poll(0, 3);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn history_truncation() {
        let feed = ChangeFeed::with_max_history(5);

        for i in 1..=10 {
            feed.emit(ChangeEvent::insert(i, 1, [i as u8; 16], vec![]));
        }

        assert_eq!(feed.history_len(), 5);
        // Only events 6-10 should remain
        let events = feed.poll(0, 100);
        assert_eq!(events[0].sequence, 6);
    }

    #[test]
    fn latest_sequence() {
        let feed = ChangeFeed::new();
        assert_eq!(feed.latest_sequence(), 0);

        feed.emit(ChangeEvent::insert(5, 1, [0u8; 16], vec![]));
        assert_eq!(feed.latest_sequence(), 5);

        feed.emit(ChangeEvent::insert(10, 1, [1u8; 16], vec![]));
        assert_eq!(feed.latest_sequence(), 10);
    }

    #[test]
    fn threaded_subscribe() {
        let feed = Arc::new(ChangeFeed::new());
        let rx = feed.subscribe();

        let feed_clone = Arc::clone(&feed);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            feed_clone.emit(ChangeEvent::insert(1, 1, [0u8; 16], vec![42]));
        });

        let received = rx.recv_timeout(Duration::from_millis(500)).unwrap();
        assert_eq!(received.payload, Some(vec![42]));

        handle.join().unwrap();
    }
}
