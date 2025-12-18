//! Transaction state.

use crate::entity::EntityId;
use crate::error::{CoreError, CoreResult};
use crate::types::{CollectionId, SequenceNumber, TransactionId};
use std::collections::HashMap;

/// State of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// Transaction is active and can perform operations.
    Active,
    /// Transaction has been committed.
    Committed,
    /// Transaction has been aborted.
    Aborted,
}

/// Represents a pending write in a transaction.
#[derive(Debug, Clone)]
pub enum PendingWrite {
    /// Insert or update an entity.
    Put {
        /// Entity payload (canonical CBOR bytes).
        payload: Vec<u8>,
    },
    /// Delete an entity.
    Delete,
}

/// An active transaction.
///
/// Transactions provide atomicity and isolation. Changes made within a
/// transaction are not visible to other readers until commit.
#[derive(Debug)]
pub struct Transaction {
    /// Transaction ID.
    id: TransactionId,
    /// Snapshot sequence number (reads see this point in time).
    snapshot_seq: SequenceNumber,
    /// Current state.
    state: TransactionState,
    /// Pending writes: (collection_id, entity_id) -> write operation.
    writes: HashMap<(CollectionId, EntityId), PendingWrite>,
    /// Read set for conflict detection: (collection_id, entity_id) -> observed sequence.
    reads: HashMap<(CollectionId, EntityId), Option<SequenceNumber>>,
}

impl Transaction {
    /// Creates a new transaction.
    pub(crate) fn new(id: TransactionId, snapshot_seq: SequenceNumber) -> Self {
        Self {
            id,
            snapshot_seq,
            state: TransactionState::Active,
            writes: HashMap::new(),
            reads: HashMap::new(),
        }
    }

    /// Returns the transaction ID.
    #[must_use]
    pub fn id(&self) -> TransactionId {
        self.id
    }

    /// Returns the snapshot sequence number.
    #[must_use]
    pub fn snapshot_seq(&self) -> SequenceNumber {
        self.snapshot_seq
    }

    /// Returns the current state.
    #[must_use]
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Checks if the transaction is still active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active
    }

    /// Records a put operation.
    pub fn put(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        payload: Vec<u8>,
    ) -> CoreResult<()> {
        self.ensure_active()?;
        self.writes
            .insert((collection_id, entity_id), PendingWrite::Put { payload });
        Ok(())
    }

    /// Records a delete operation.
    pub fn delete(&mut self, collection_id: CollectionId, entity_id: EntityId) -> CoreResult<()> {
        self.ensure_active()?;
        self.writes
            .insert((collection_id, entity_id), PendingWrite::Delete);
        Ok(())
    }

    /// Records a read for conflict detection.
    pub fn record_read(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        observed_seq: Option<SequenceNumber>,
    ) {
        // Only record if not already written in this transaction
        let key = (collection_id, entity_id);
        if !self.writes.contains_key(&key) {
            self.reads.insert(key, observed_seq);
        }
    }

    /// Gets a pending write for an entity.
    #[must_use]
    pub fn get_pending_write(
        &self,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> Option<&PendingWrite> {
        self.writes.get(&(collection_id, entity_id))
    }

    /// Returns all pending writes.
    pub fn pending_writes(
        &self,
    ) -> impl Iterator<Item = (&(CollectionId, EntityId), &PendingWrite)> {
        self.writes.iter()
    }

    /// Returns the number of pending writes.
    #[must_use]
    pub fn write_count(&self) -> usize {
        self.writes.len()
    }

    /// Returns the read set for conflict detection.
    pub fn read_set(
        &self,
    ) -> impl Iterator<Item = (&(CollectionId, EntityId), &Option<SequenceNumber>)> {
        self.reads.iter()
    }

    /// Marks the transaction as committed.
    pub(crate) fn mark_committed(&mut self) {
        self.state = TransactionState::Committed;
    }

    /// Marks the transaction as aborted.
    pub(crate) fn mark_aborted(&mut self) {
        self.state = TransactionState::Aborted;
    }

    /// Ensures the transaction is active.
    fn ensure_active(&self) -> CoreResult<()> {
        match self.state {
            TransactionState::Active => Ok(()),
            TransactionState::Committed => Err(CoreError::invalid_operation(
                "transaction already committed",
            )),
            TransactionState::Aborted => {
                Err(CoreError::invalid_operation("transaction already aborted"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_txn() -> Transaction {
        Transaction::new(TransactionId::new(1), SequenceNumber::new(0))
    }

    #[test]
    fn new_transaction_is_active() {
        let txn = create_txn();
        assert!(txn.is_active());
        assert_eq!(txn.state(), TransactionState::Active);
    }

    #[test]
    fn put_records_write() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.put(collection, entity, vec![1, 2, 3]).unwrap();

        assert_eq!(txn.write_count(), 1);
        assert!(txn.get_pending_write(collection, entity).is_some());
    }

    #[test]
    fn delete_records_write() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.delete(collection, entity).unwrap();

        let write = txn.get_pending_write(collection, entity);
        assert!(matches!(write, Some(PendingWrite::Delete)));
    }

    #[test]
    fn put_overwrites_previous() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.put(collection, entity, vec![1]).unwrap();
        txn.put(collection, entity, vec![2]).unwrap();

        assert_eq!(txn.write_count(), 1);
        if let Some(PendingWrite::Put { payload }) = txn.get_pending_write(collection, entity) {
            assert_eq!(payload, &vec![2]);
        } else {
            panic!("expected Put");
        }
    }

    #[test]
    fn cannot_write_after_commit() {
        let mut txn = create_txn();
        txn.mark_committed();

        let result = txn.put(CollectionId::new(1), EntityId::new(), vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn cannot_write_after_abort() {
        let mut txn = create_txn();
        txn.mark_aborted();

        let result = txn.delete(CollectionId::new(1), EntityId::new());
        assert!(result.is_err());
    }

    #[test]
    fn record_read_tracks_observed_sequence() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.record_read(collection, entity, Some(SequenceNumber::new(5)));

        let reads: Vec<_> = txn.read_set().collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].1, &Some(SequenceNumber::new(5)));
    }

    #[test]
    fn read_not_recorded_if_written() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.put(collection, entity, vec![1]).unwrap();
        txn.record_read(collection, entity, Some(SequenceNumber::new(5)));

        // Should not be in read set since we wrote to it
        let reads: Vec<_> = txn.read_set().collect();
        assert!(reads.is_empty());
    }
}
