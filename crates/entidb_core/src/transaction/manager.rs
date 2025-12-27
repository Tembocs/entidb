//! Transaction manager.

use crate::entity::EntityId;
use crate::error::{CoreError, CoreResult};
use crate::segment::{SegmentManager, SegmentRecord};
use crate::transaction::state::{PendingWrite, Transaction, WriteTransaction};
use crate::types::{CollectionId, SequenceNumber, TransactionId};
use crate::wal::{WalManager, WalRecord};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Manages transactions with ACID guarantees.
///
/// The transaction manager provides:
/// - Single-writer concurrency control via `begin_write()`
/// - Snapshot isolation for readers
/// - WAL-based durability
/// - Commit ordering via sequence numbers
///
/// ## Single-Writer Guarantee
///
/// Only one write transaction can be active at a time. Use `begin_write()`
/// to start a write transaction, which acquires an exclusive lock that is
/// held for the transaction's lifetime.
pub struct TransactionManager {
    /// WAL for durability.
    wal: Arc<WalManager>,
    /// Segment storage for entities.
    segments: Arc<SegmentManager>,
    /// Next transaction ID.
    next_txid: AtomicU64,
    /// Next sequence number.
    next_seq: AtomicU64,
    /// Current committed sequence (for snapshots).
    committed_seq: AtomicU64,
    /// Write lock - only one writer at a time.
    write_lock: Mutex<()>,
    /// Active transactions.
    active_txns: RwLock<Vec<TransactionId>>,
}

impl TransactionManager {
    /// Creates a new transaction manager.
    pub fn new(wal: Arc<WalManager>, segments: Arc<SegmentManager>) -> Self {
        Self {
            wal,
            segments,
            next_txid: AtomicU64::new(1),
            next_seq: AtomicU64::new(1),
            committed_seq: AtomicU64::new(0),
            write_lock: Mutex::new(()),
            active_txns: RwLock::new(Vec::new()),
        }
    }

    /// Creates a transaction manager initialized from recovery state.
    pub fn with_state(
        wal: Arc<WalManager>,
        segments: Arc<SegmentManager>,
        next_txid: u64,
        next_seq: u64,
        committed_seq: u64,
    ) -> Self {
        Self {
            wal,
            segments,
            next_txid: AtomicU64::new(next_txid),
            next_seq: AtomicU64::new(next_seq),
            committed_seq: AtomicU64::new(committed_seq),
            write_lock: Mutex::new(()),
            active_txns: RwLock::new(Vec::new()),
        }
    }

    /// Begins a new read-only transaction.
    ///
    /// The transaction gets a snapshot of the current committed state.
    /// For write transactions, use `begin_write()` instead.
    pub fn begin(&self) -> CoreResult<Transaction> {
        let txid = TransactionId::new(self.next_txid.fetch_add(1, Ordering::SeqCst));
        let snapshot_seq = SequenceNumber::new(self.committed_seq.load(Ordering::SeqCst));

        // Write BEGIN record to WAL
        self.wal.append(&WalRecord::Begin { txid })?;

        // Track active transaction
        self.active_txns.write().push(txid);

        Ok(Transaction::new(txid, snapshot_seq))
    }

    /// Begins a new write transaction with exclusive write lock.
    ///
    /// This acquires the write lock immediately and holds it for the
    /// transaction's lifetime. Only one write transaction can exist at a time.
    ///
    /// The lock is automatically released when the transaction is committed,
    /// aborted, or dropped.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut wtx = tm.begin_write()?;
    /// wtx.put(collection, entity, payload)?;
    /// tm.commit_write(&mut wtx)?;
    /// // Lock is released after commit
    /// ```
    pub fn begin_write(&self) -> CoreResult<WriteTransaction<'_>> {
        // Acquire exclusive write lock - this blocks if another writer exists
        let guard = self.write_lock.lock();

        // Create the underlying transaction
        let txn = self.begin()?;

        Ok(WriteTransaction::new(txn, guard))
    }

    /// Commits a transaction.
    ///
    /// All pending writes are applied atomically. After commit returns,
    /// the changes are durable and visible to new transactions.
    ///
    /// Note: For write transactions created with `begin_write()`, use
    /// `commit_write()` instead, which doesn't re-acquire the lock.
    pub fn commit(&self, txn: &mut Transaction) -> CoreResult<SequenceNumber> {
        // Acquire write lock for commit phase
        let _write_guard = self.write_lock.lock();
        self.commit_inner(txn)
    }

    /// Internal commit implementation that assumes lock is already held.
    fn commit_inner(&self, txn: &mut Transaction) -> CoreResult<SequenceNumber> {
        if !txn.is_active() {
            return Err(CoreError::invalid_operation("transaction not active"));
        }

        let txid = txn.id();
        let sequence = SequenceNumber::new(self.next_seq.fetch_add(1, Ordering::SeqCst));

        // Write all operations to WAL
        for ((collection_id, entity_id), write) in txn.pending_writes() {
            let entity_bytes = *entity_id.as_bytes();
            match write {
                PendingWrite::Put { payload, .. } => {
                    self.wal.append(&WalRecord::Put {
                        txid,
                        collection_id: *collection_id,
                        entity_id: entity_bytes,
                        before_hash: None, // TODO: implement conflict detection
                        after_bytes: payload.clone(),
                    })?;
                }
                PendingWrite::Delete => {
                    self.wal.append(&WalRecord::Delete {
                        txid,
                        collection_id: *collection_id,
                        entity_id: entity_bytes,
                        before_hash: None,
                    })?;
                }
            }
        }

        // Write COMMIT record
        self.wal.append(&WalRecord::Commit { txid, sequence })?;

        // Flush WAL - critical for durability
        self.wal.flush()?;

        // Apply to segments (now that WAL is durable)
        for ((collection_id, entity_id), write) in txn.pending_writes() {
            let entity_bytes = *entity_id.as_bytes();
            match write {
                PendingWrite::Put { payload, .. } => {
                    let record =
                        SegmentRecord::put(*collection_id, entity_bytes, payload.clone(), sequence);
                    self.segments.append(&record)?;
                }
                PendingWrite::Delete => {
                    let record = SegmentRecord::tombstone(*collection_id, entity_bytes, sequence);
                    self.segments.append(&record)?;
                }
            }
        }

        // Flush segments
        self.segments.flush()?;

        // Update committed sequence
        self.committed_seq
            .store(sequence.as_u64(), Ordering::SeqCst);

        // Remove from active transactions
        self.active_txns.write().retain(|&id| id != txid);

        // Mark transaction as committed
        txn.mark_committed();

        Ok(sequence)
    }

    /// Aborts a transaction.
    ///
    /// All pending writes are discarded.
    pub fn abort(&self, txn: &mut Transaction) -> CoreResult<()> {
        if !txn.is_active() {
            return Err(CoreError::invalid_operation("transaction not active"));
        }

        let txid = txn.id();

        // Write ABORT record to WAL
        self.wal.append(&WalRecord::Abort { txid })?;

        // Remove from active transactions
        self.active_txns.write().retain(|&id| id != txid);

        // Mark transaction as aborted
        txn.mark_aborted();

        Ok(())
    }

    /// Gets an entity within a transaction's snapshot.
    ///
    /// This uses the transaction's snapshot sequence to provide
    /// snapshot isolation - the read sees data as of when the
    /// transaction started, not the current state.
    pub fn get(
        &self,
        txn: &mut Transaction,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        // First check pending writes in this transaction
        if let Some(write) = txn.get_pending_write(collection_id, entity_id) {
            return match write {
                PendingWrite::Put { payload, .. } => Ok(Some(payload.clone())),
                PendingWrite::Delete => Ok(None),
            };
        }

        // Read from segments at the transaction's snapshot point
        let snapshot_seq = txn.snapshot_seq();
        let result = self
            .segments
            .get_at_snapshot(collection_id, entity_id.as_bytes(), Some(snapshot_seq))?;

        // Record the read for conflict detection
        txn.record_read(collection_id, entity_id, Some(snapshot_seq));

        Ok(result)
    }

    /// Gets an entity within a write transaction's snapshot.
    pub fn get_write(
        &self,
        wtxn: &mut WriteTransaction<'_>,
        collection_id: CollectionId,
        entity_id: EntityId,
    ) -> CoreResult<Option<Vec<u8>>> {
        self.get(wtxn.inner_mut(), collection_id, entity_id)
    }

    /// Commits a write transaction.
    ///
    /// All pending writes are applied atomically. After commit returns,
    /// the changes are durable and visible to new transactions.
    /// The write lock is released after this call.
    pub fn commit_write(&self, wtxn: &mut WriteTransaction<'_>) -> CoreResult<SequenceNumber> {
        // WriteTransaction already holds the write lock, so we don't acquire it again
        self.commit_inner(wtxn.inner_mut())
    }

    /// Aborts a write transaction.
    ///
    /// All pending writes are discarded. The write lock is released after this call.
    pub fn abort_write(&self, wtxn: &mut WriteTransaction<'_>) -> CoreResult<()> {
        self.abort(wtxn.inner_mut())
    }

    /// Returns the current committed sequence number.
    #[must_use]
    pub fn committed_seq(&self) -> SequenceNumber {
        SequenceNumber::new(self.committed_seq.load(Ordering::SeqCst))
    }

    /// Returns the number of active transactions.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active_txns.read().len()
    }

    /// Creates a checkpoint.
    ///
    /// A checkpoint:
    /// 1. Ensures all segments are synced to durable storage
    /// 2. Writes a checkpoint record to WAL
    /// 3. Truncates the WAL (all committed data is in segments)
    ///
    /// After checkpoint, WAL space is reclaimed.
    pub fn checkpoint(&self) -> CoreResult<()> {
        // First, sync segments to ensure all committed data is durable on disk
        // Using sync() instead of flush() to guarantee data survives power loss
        self.segments.sync()?;

        let sequence = self.committed_seq();
        
        // Write checkpoint record
        self.wal.append(&WalRecord::Checkpoint { sequence })?;
        self.wal.flush()?;

        // Now we can safely truncate the WAL since all committed
        // transactions are persisted in segments
        self.wal.clear()?;

        Ok(())
    }
}

impl std::fmt::Debug for TransactionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionManager")
            .field("committed_seq", &self.committed_seq())
            .field("active_count", &self.active_count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_storage::InMemoryBackend;

    fn create_manager() -> TransactionManager {
        let wal = Arc::new(WalManager::new(Box::new(InMemoryBackend::new()), false));
        let segments = Arc::new(SegmentManager::new(
            Box::new(InMemoryBackend::new()),
            1024 * 1024,
        ));
        TransactionManager::new(wal, segments)
    }

    #[test]
    fn begin_creates_transaction() {
        let tm = create_manager();
        let txn = tm.begin().unwrap();
        assert!(txn.is_active());
        assert_eq!(tm.active_count(), 1);
    }

    #[test]
    fn commit_empty_transaction() {
        let tm = create_manager();
        let mut txn = tm.begin().unwrap();
        let seq = tm.commit(&mut txn).unwrap();
        assert_eq!(seq.as_u64(), 1);
        assert!(!txn.is_active());
        assert_eq!(tm.active_count(), 0);
    }

    #[test]
    fn abort_transaction() {
        let tm = create_manager();
        let mut txn = tm.begin().unwrap();
        tm.abort(&mut txn).unwrap();
        assert!(!txn.is_active());
        assert_eq!(tm.active_count(), 0);
    }

    #[test]
    fn put_and_get_in_transaction() {
        let tm = create_manager();
        let mut txn = tm.begin().unwrap();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();
        let payload = vec![1, 2, 3];

        txn.put(collection, entity, payload.clone()).unwrap();

        // Should see uncommitted write within transaction
        let result = tm.get(&mut txn, collection, entity).unwrap();
        assert_eq!(result, Some(payload));
    }

    #[test]
    fn delete_in_transaction() {
        let tm = create_manager();
        let mut txn = tm.begin().unwrap();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        txn.put(collection, entity, vec![1, 2, 3]).unwrap();
        txn.delete(collection, entity).unwrap();

        // Should see deletion
        let result = tm.get(&mut txn, collection, entity).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn committed_data_visible_to_new_transaction() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();
        let payload = vec![42, 43, 44];

        // First transaction: write
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, payload.clone()).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Second transaction: read
        {
            let mut txn = tm.begin().unwrap();
            let result = tm.get(&mut txn, collection, entity).unwrap();
            assert_eq!(result, Some(payload));
        }
    }

    #[test]
    fn aborted_data_not_visible() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // First transaction: write then abort
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![1, 2, 3]).unwrap();
            tm.abort(&mut txn).unwrap();
        }

        // Second transaction: should not see data
        {
            let mut txn = tm.begin().unwrap();
            let result = tm.get(&mut txn, collection, entity).unwrap();
            assert!(result.is_none());
        }
    }

    #[test]
    fn sequence_numbers_increase() {
        let tm = create_manager();

        let mut txn1 = tm.begin().unwrap();
        let seq1 = tm.commit(&mut txn1).unwrap();

        let mut txn2 = tm.begin().unwrap();
        let seq2 = tm.commit(&mut txn2).unwrap();

        assert!(seq2 > seq1);
    }

    #[test]
    fn cannot_commit_twice() {
        let tm = create_manager();
        let mut txn = tm.begin().unwrap();
        tm.commit(&mut txn).unwrap();

        let result = tm.commit(&mut txn);
        assert!(result.is_err());
    }

    #[test]
    fn cannot_abort_after_commit() {
        let tm = create_manager();
        let mut txn = tm.begin().unwrap();
        tm.commit(&mut txn).unwrap();

        let result = tm.abort(&mut txn);
        assert!(result.is_err());
    }

    #[test]
    fn checkpoint() {
        let tm = create_manager();

        let mut txn = tm.begin().unwrap();
        txn.put(CollectionId::new(1), EntityId::new(), vec![1])
            .unwrap();
        tm.commit(&mut txn).unwrap();

        // Should not error
        tm.checkpoint().unwrap();
    }

    #[test]
    fn committed_seq_updates() {
        let tm = create_manager();
        assert_eq!(tm.committed_seq().as_u64(), 0);

        let mut txn = tm.begin().unwrap();
        tm.commit(&mut txn).unwrap();

        assert_eq!(tm.committed_seq().as_u64(), 1);
    }

    // === Snapshot Isolation Tests ===

    #[test]
    fn snapshot_isolation_reader_sees_old_version() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // Write initial value
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![1, 1, 1]).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Start a read transaction (gets snapshot of seq=1)
        let mut reader = tm.begin().unwrap();
        let reader_snapshot = reader.snapshot_seq();

        // Another transaction updates the entity
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![2, 2, 2]).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Reader should still see the OLD value (snapshot isolation)
        let result = tm.get(&mut reader, collection, entity).unwrap();
        assert_eq!(
            result,
            Some(vec![1, 1, 1]),
            "Reader with snapshot {:?} should see old value",
            reader_snapshot
        );

        // A new reader should see the NEW value
        let mut new_reader = tm.begin().unwrap();
        let new_result = tm.get(&mut new_reader, collection, entity).unwrap();
        assert_eq!(new_result, Some(vec![2, 2, 2]));
    }

    #[test]
    fn snapshot_isolation_reader_does_not_see_uncommitted() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // Write initial value
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![1]).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Start a writer but don't commit
        let mut writer = tm.begin().unwrap();
        writer.put(collection, entity, vec![2]).unwrap();

        // Start a reader
        let mut reader = tm.begin().unwrap();

        // Reader should see old value (uncommitted writes are not visible)
        let result = tm.get(&mut reader, collection, entity).unwrap();
        assert_eq!(result, Some(vec![1]));

        // Abort the writer
        tm.abort(&mut writer).unwrap();
    }

    #[test]
    fn snapshot_isolation_entity_created_after_snapshot_not_visible() {
        let tm = create_manager();
        let collection = CollectionId::new(1);

        // Start a reader before any data exists
        let mut reader = tm.begin().unwrap();

        // Create entity after reader started
        let entity = EntityId::new();
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![42]).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Reader should NOT see the entity (created after snapshot)
        let result = tm.get(&mut reader, collection, entity).unwrap();
        assert!(result.is_none());

        // New reader should see it
        let mut new_reader = tm.begin().unwrap();
        let new_result = tm.get(&mut new_reader, collection, entity).unwrap();
        assert_eq!(new_result, Some(vec![42]));
    }

    #[test]
    fn snapshot_isolation_deletion_after_snapshot_not_visible() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // Write initial value
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![1]).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Start a reader
        let mut reader = tm.begin().unwrap();

        // Delete after reader started
        {
            let mut txn = tm.begin().unwrap();
            txn.delete(collection, entity).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Reader should still see the entity
        let result = tm.get(&mut reader, collection, entity).unwrap();
        assert_eq!(result, Some(vec![1]));

        // New reader should see deletion
        let mut new_reader = tm.begin().unwrap();
        let new_result = tm.get(&mut new_reader, collection, entity).unwrap();
        assert!(new_result.is_none());
    }

    // === Write Transaction Tests ===

    #[test]
    fn begin_write_creates_write_transaction() {
        let tm = create_manager();
        let wtxn = tm.begin_write().unwrap();
        assert!(wtxn.is_active());
        assert_eq!(tm.active_count(), 1);
    }

    #[test]
    fn write_transaction_commit() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        let mut wtxn = tm.begin_write().unwrap();
        wtxn.put(collection, entity, vec![1, 2, 3]).unwrap();
        let seq = tm.commit_write(&mut wtxn).unwrap();

        assert_eq!(seq.as_u64(), 1);
        assert!(!wtxn.is_active());

        // Data should be visible
        let mut reader = tm.begin().unwrap();
        let result = tm.get(&mut reader, collection, entity).unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));
    }

    #[test]
    fn write_transaction_abort() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        let mut wtxn = tm.begin_write().unwrap();
        wtxn.put(collection, entity, vec![1, 2, 3]).unwrap();
        tm.abort_write(&mut wtxn).unwrap();

        // Data should NOT be visible
        let mut reader = tm.begin().unwrap();
        let result = tm.get(&mut reader, collection, entity).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn write_transaction_has_snapshot_isolation() {
        let tm = create_manager();
        let collection = CollectionId::new(1);
        let entity = EntityId::new();

        // Write initial value
        {
            let mut txn = tm.begin().unwrap();
            txn.put(collection, entity, vec![1]).unwrap();
            tm.commit(&mut txn).unwrap();
        }

        // Start a write transaction
        let mut wtxn = tm.begin_write().unwrap();

        // Write transaction should see value from its snapshot
        let result = tm.get_write(&mut wtxn, collection, entity).unwrap();
        assert_eq!(result, Some(vec![1]));

        tm.abort_write(&mut wtxn).unwrap();
    }
}
