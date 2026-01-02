//! Transaction state.

use crate::entity::EntityId;
use crate::error::{CoreError, CoreResult};
use crate::types::{CollectionId, SequenceNumber, TransactionId};
use parking_lot::MutexGuard;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Computes a SHA-256 hash of entity content for conflict detection.
///
/// This function computes a deterministic hash over the canonical CBOR bytes
/// of an entity. The hash is used for optimistic conflict detection:
/// - When reading an entity, compute its hash
/// - When writing, store the "before" hash
/// - At commit, verify the before hash matches current state
///
/// Returns a 32-byte hash.
#[must_use]
pub fn compute_content_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

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
        /// Whether this is an update (entity existed before this transaction).
        ///
        /// - `Some(true)` = entity existed at transaction snapshot → Update
        /// - `Some(false)` = entity did not exist at transaction snapshot → Insert
        /// - `None` = not yet determined (will be resolved at commit time)
        ///
        /// This field is used by the change feed to emit the correct operation type.
        is_update: Option<bool>,
        /// Hash of the entity's "before" state for conflict detection.
        ///
        /// - `Some(hash)` = entity existed; hash is SHA-256 of previous content
        /// - `None` = entity did not exist before this write
        ///
        /// At commit time, this is verified against the current committed value.
        before_hash: Option<[u8; 32]>,
    },
    /// Delete an entity.
    Delete {
        /// Hash of the entity's "before" state for conflict detection.
        ///
        /// - `Some(hash)` = entity existed; hash is SHA-256 of previous content
        /// - `None` = entity did not exist (delete of non-existent is a no-op)
        before_hash: Option<[u8; 32]>,
    },
}

/// Callback for cleaning up a transaction from the active list.
///
/// This is called when a `WriteTransaction` is dropped without being
/// committed or aborted, ensuring proper RAII cleanup.
pub type CleanupCallback = Box<dyn FnOnce(TransactionId) + Send + Sync>;

/// A write transaction that holds an exclusive write lock.
///
/// This type ensures single-writer semantics by holding the write lock
/// for the entire duration of the transaction. Only one `WriteTransaction`
/// can exist at a time.
///
/// The lock is released when the transaction is committed, aborted, or dropped.
/// If dropped without commit/abort, the transaction is automatically removed
/// from the active transaction list via RAII cleanup.
pub struct WriteTransaction<'a> {
    /// The underlying transaction.
    inner: Transaction,
    /// The write lock guard - held for the transaction's lifetime.
    /// Using Option so we can release it on commit/abort.
    _write_guard: Option<MutexGuard<'a, ()>>,
    /// Cleanup callback for RAII - removes from active_txns on drop.
    /// None if already committed/aborted.
    cleanup: Option<CleanupCallback>,
}

impl<'a> WriteTransaction<'a> {
    /// Creates a new write transaction with the given lock guard and cleanup callback.
    pub(crate) fn new(
        inner: Transaction,
        guard: MutexGuard<'a, ()>,
        cleanup: CleanupCallback,
    ) -> Self {
        Self {
            inner,
            _write_guard: Some(guard),
            cleanup: Some(cleanup),
        }
    }

    /// Marks the transaction as finalized (committed or aborted).
    ///
    /// This prevents the cleanup callback from running on drop.
    pub(crate) fn mark_finalized(&mut self) {
        self.cleanup = None;
    }

    /// Returns a reference to the inner transaction.
    pub fn inner(&self) -> &Transaction {
        &self.inner
    }

    /// Returns a mutable reference to the inner transaction.
    pub fn inner_mut(&mut self) -> &mut Transaction {
        &mut self.inner
    }

    /// Consumes self and returns the inner transaction.
    /// This also releases the write lock and prevents cleanup callback.
    pub fn into_inner(mut self) -> Transaction {
        self._write_guard = None;
        self.cleanup = None;
        std::mem::replace(
            &mut self.inner,
            Transaction::new(TransactionId::new(0), SequenceNumber::new(0)),
        )
    }

    // Delegate common methods to inner transaction

    /// Returns the transaction ID.
    #[must_use]
    pub fn id(&self) -> TransactionId {
        self.inner.id()
    }

    /// Returns the snapshot sequence number.
    #[must_use]
    pub fn snapshot_seq(&self) -> SequenceNumber {
        self.inner.snapshot_seq()
    }

    /// Checks if the transaction is still active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Records a put operation.
    pub fn put(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        payload: Vec<u8>,
    ) -> CoreResult<()> {
        self.inner.put(collection_id, entity_id, payload)
    }

    /// Records a put operation with a known operation type.
    ///
    /// Use this when you already know whether this is an insert or update.
    pub fn put_with_op_type(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        payload: Vec<u8>,
        is_update: bool,
    ) -> CoreResult<()> {
        self.inner
            .put_with_op_type(collection_id, entity_id, payload, is_update)
    }

    /// Records a delete operation.
    pub fn delete(&mut self, collection_id: CollectionId, entity_id: EntityId) -> CoreResult<()> {
        self.inner.delete(collection_id, entity_id)
    }

    /// Gets a pending write for an entity.
    #[must_use]
    pub fn get_pending_write(
        &self,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> Option<&PendingWrite> {
        self.inner.get_pending_write(collection_id, entity_id)
    }

    /// Records a read for conflict detection.
    ///
    /// `observed_hash` is the SHA-256 hash of the entity content at read time,
    /// or `None` if the entity did not exist.
    pub fn record_read(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        observed_hash: Option<[u8; 32]>,
    ) {
        self.inner
            .record_read(collection_id, entity_id, observed_hash)
    }
}

impl Drop for WriteTransaction<'_> {
    fn drop(&mut self) {
        // If cleanup callback exists, transaction was not committed/aborted
        // Run the cleanup to remove from active_txns
        if let Some(cleanup) = self.cleanup.take() {
            cleanup(self.inner.id());
        }
    }
}

impl std::fmt::Debug for WriteTransaction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteTransaction")
            .field("id", &self.inner.id())
            .field("snapshot_seq", &self.inner.snapshot_seq())
            .field("state", &self.inner.state())
            .field("write_count", &self.inner.write_count())
            .finish()
    }
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
    /// Read set for conflict detection: (collection_id, entity_id) -> observed content hash.
    /// `Some(hash)` means entity existed with that content hash.
    /// `None` means entity did not exist at read time.
    reads: HashMap<(CollectionId, EntityId), Option<[u8; 32]>>,
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
    ///
    /// The `is_update` and `before_hash` will be determined at commit time
    /// from the read set or by checking the current entity state.
    pub fn put(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        payload: Vec<u8>,
    ) -> CoreResult<()> {
        self.ensure_active()?;
        // Get before_hash from read set if we previously read this entity
        let before_hash = self
            .reads
            .get(&(collection_id, entity_id))
            .copied()
            .flatten();
        self.writes.insert(
            (collection_id, entity_id),
            PendingWrite::Put {
                payload,
                is_update: None, // Will be resolved at commit time
                before_hash,
            },
        );
        Ok(())
    }

    /// Records a put operation with a known operation type.
    ///
    /// Use this when you already know whether this is an insert or update.
    /// This avoids the need to check entity existence at commit time.
    pub fn put_with_op_type(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        payload: Vec<u8>,
        is_update: bool,
    ) -> CoreResult<()> {
        self.ensure_active()?;
        // Get before_hash from read set if we previously read this entity
        let before_hash = self
            .reads
            .get(&(collection_id, entity_id))
            .copied()
            .flatten();
        self.writes.insert(
            (collection_id, entity_id),
            PendingWrite::Put {
                payload,
                is_update: Some(is_update),
                before_hash,
            },
        );
        Ok(())
    }

    /// Records a delete operation.
    pub fn delete(&mut self, collection_id: CollectionId, entity_id: EntityId) -> CoreResult<()> {
        self.ensure_active()?;
        // Get before_hash from read set if we previously read this entity
        let before_hash = self
            .reads
            .get(&(collection_id, entity_id))
            .copied()
            .flatten();
        self.writes.insert(
            (collection_id, entity_id),
            PendingWrite::Delete { before_hash },
        );
        Ok(())
    }

    /// Records a read for conflict detection.
    ///
    /// `observed_hash` is the SHA-256 hash of the entity content at read time,
    /// or `None` if the entity did not exist.
    pub fn record_read(
        &mut self,
        collection_id: CollectionId,
        entity_id: EntityId,
        observed_hash: Option<[u8; 32]>,
    ) {
        // Only record if not already written in this transaction
        let key = (collection_id, entity_id);
        if !self.writes.contains_key(&key) {
            self.reads.insert(key, observed_hash);
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
    ///
    /// Returns entries of (collection_id, entity_id) -> `Option<hash>`
    /// where `Some(hash)` means the entity existed with that content hash,
    /// and `None` means the entity did not exist.
    pub fn read_set(&self) -> impl Iterator<Item = (&(CollectionId, EntityId), &Option<[u8; 32]>)> {
        self.reads.iter()
    }

    /// Gets the observed hash for an entity from the read set.
    ///
    /// Returns `Some(Some(hash))` if entity was read and existed,
    /// `Some(None)` if entity was read and did not exist,
    /// `None` if entity was never read in this transaction.
    #[must_use]
    pub fn get_read_hash(
        &self,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> Option<Option<[u8; 32]>> {
        self.reads.get(&(collection_id, entity_id)).copied()
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
        assert!(matches!(write, Some(PendingWrite::Delete { .. })));
    }

    #[test]
    fn put_overwrites_previous() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.put(collection, entity, vec![1]).unwrap();
        txn.put(collection, entity, vec![2]).unwrap();

        assert_eq!(txn.write_count(), 1);
        if let Some(PendingWrite::Put { payload, .. }) = txn.get_pending_write(collection, entity) {
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
    fn record_read_tracks_observed_hash() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        let test_hash = compute_content_hash(b"test data");
        txn.record_read(collection, entity, Some(test_hash));

        let reads: Vec<_> = txn.read_set().collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].1, &Some(test_hash));
    }

    #[test]
    fn read_not_recorded_if_written() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.put(collection, entity, vec![1]).unwrap();
        let test_hash = compute_content_hash(b"test data");
        txn.record_read(collection, entity, Some(test_hash));

        // Should not be in read set since we wrote to it
        let reads: Vec<_> = txn.read_set().collect();
        assert!(reads.is_empty());
    }

    #[test]
    fn put_captures_before_hash_from_read_set() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // First record a read
        let test_hash = compute_content_hash(b"original data");
        txn.record_read(collection, entity, Some(test_hash));

        // Now write to the entity
        txn.put(collection, entity, vec![1, 2, 3]).unwrap();

        // The pending write should have captured the before_hash
        if let Some(PendingWrite::Put { before_hash, .. }) =
            txn.get_pending_write(collection, entity)
        {
            assert_eq!(*before_hash, Some(test_hash));
        } else {
            panic!("expected Put");
        }
    }

    #[test]
    fn delete_captures_before_hash_from_read_set() {
        let mut txn = create_txn();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // First record a read
        let test_hash = compute_content_hash(b"original data");
        txn.record_read(collection, entity, Some(test_hash));

        // Now delete the entity
        txn.delete(collection, entity).unwrap();

        // The pending write should have captured the before_hash
        if let Some(PendingWrite::Delete { before_hash }) =
            txn.get_pending_write(collection, entity)
        {
            assert_eq!(*before_hash, Some(test_hash));
        } else {
            panic!("expected Delete");
        }
    }
}
