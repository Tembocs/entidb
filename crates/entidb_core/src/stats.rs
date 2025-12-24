//! Database statistics and telemetry.
//!
//! Provides metrics counters and diagnostics for monitoring database performance.
//!
//! # Usage
//!
//! ```rust,ignore
//! use entidb_core::Database;
//!
//! let db = Database::open_in_memory()?;
//!
//! // Perform operations...
//! db.put(&users, id, data);
//!
//! // Get stats
//! let stats = db.stats();
//! println!("Reads: {}", stats.reads);
//! println!("Writes: {}", stats.writes);
//! println!("Transactions: {}", stats.transactions_committed);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

/// Database statistics and metrics.
///
/// All counters are atomic and can be read while operations are in progress.
/// Values are monotonically increasing (except for gauges like `active_transactions`).
#[derive(Debug, Default)]
pub struct DatabaseStats {
    // Operation counters
    /// Total number of read operations.
    reads: AtomicU64,
    /// Total number of write (put) operations.
    writes: AtomicU64,
    /// Total number of delete operations.
    deletes: AtomicU64,
    /// Total number of full collection scans.
    scans: AtomicU64,
    /// Total number of index lookups.
    index_lookups: AtomicU64,

    // Transaction counters
    /// Total number of transactions started.
    transactions_started: AtomicU64,
    /// Total number of transactions committed.
    transactions_committed: AtomicU64,
    /// Total number of transactions aborted.
    transactions_aborted: AtomicU64,

    // Entity counters
    /// Total entities in the database (approximate, updated on checkpoint).
    entity_count: AtomicU64,

    // Bytes counters
    /// Total bytes written.
    bytes_written: AtomicU64,
    /// Total bytes read.
    bytes_read: AtomicU64,

    // Checkpoint counters
    /// Total number of checkpoints performed.
    checkpoints: AtomicU64,

    // Error counters
    /// Total number of errors encountered.
    errors: AtomicU64,
}

impl DatabaseStats {
    /// Creates a new stats instance.
    pub fn new() -> Self {
        Self::default()
    }

    // === Increment methods (internal use) ===

    /// Records a read operation.
    pub(crate) fn record_read(&self, bytes: u64) {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Records a write operation.
    pub(crate) fn record_write(&self, bytes: u64) {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Records a delete operation.
    pub(crate) fn record_delete(&self) {
        self.deletes.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a full scan.
    pub(crate) fn record_scan(&self) {
        self.scans.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an index lookup.
    pub(crate) fn record_index_lookup(&self) {
        self.index_lookups.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a transaction start.
    pub(crate) fn record_transaction_start(&self) {
        self.transactions_started.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a transaction commit.
    pub(crate) fn record_transaction_commit(&self) {
        self.transactions_committed.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a transaction abort.
    pub(crate) fn record_transaction_abort(&self) {
        self.transactions_aborted.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a checkpoint.
    pub(crate) fn record_checkpoint(&self) {
        self.checkpoints.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an error.
    pub(crate) fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Updates the entity count.
    pub(crate) fn set_entity_count(&self, count: u64) {
        self.entity_count.store(count, Ordering::Relaxed);
    }

    // === Getter methods (public API) ===

    /// Returns the total number of read operations.
    pub fn reads(&self) -> u64 {
        self.reads.load(Ordering::Relaxed)
    }

    /// Returns the total number of write operations.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }

    /// Returns the total number of delete operations.
    pub fn deletes(&self) -> u64 {
        self.deletes.load(Ordering::Relaxed)
    }

    /// Returns the total number of full collection scans.
    ///
    /// High scan counts may indicate missing indexes.
    pub fn scans(&self) -> u64 {
        self.scans.load(Ordering::Relaxed)
    }

    /// Returns the total number of index lookups.
    pub fn index_lookups(&self) -> u64 {
        self.index_lookups.load(Ordering::Relaxed)
    }

    /// Returns the total number of transactions started.
    pub fn transactions_started(&self) -> u64 {
        self.transactions_started.load(Ordering::Relaxed)
    }

    /// Returns the total number of transactions committed.
    pub fn transactions_committed(&self) -> u64 {
        self.transactions_committed.load(Ordering::Relaxed)
    }

    /// Returns the total number of transactions aborted.
    pub fn transactions_aborted(&self) -> u64 {
        self.transactions_aborted.load(Ordering::Relaxed)
    }

    /// Returns the approximate entity count.
    pub fn entity_count(&self) -> u64 {
        self.entity_count.load(Ordering::Relaxed)
    }

    /// Returns the total bytes written.
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }

    /// Returns the total bytes read.
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read.load(Ordering::Relaxed)
    }

    /// Returns the total number of checkpoints.
    pub fn checkpoints(&self) -> u64 {
        self.checkpoints.load(Ordering::Relaxed)
    }

    /// Returns the total number of errors.
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    /// Returns a snapshot of all stats.
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            reads: self.reads(),
            writes: self.writes(),
            deletes: self.deletes(),
            scans: self.scans(),
            index_lookups: self.index_lookups(),
            transactions_started: self.transactions_started(),
            transactions_committed: self.transactions_committed(),
            transactions_aborted: self.transactions_aborted(),
            entity_count: self.entity_count(),
            bytes_written: self.bytes_written(),
            bytes_read: self.bytes_read(),
            checkpoints: self.checkpoints(),
            errors: self.errors(),
        }
    }
}

/// A point-in-time snapshot of database statistics.
///
/// Unlike `DatabaseStats`, this is a simple struct that can be serialized,
/// compared, or passed across threads without atomics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatsSnapshot {
    /// Total number of read operations.
    pub reads: u64,
    /// Total number of write operations.
    pub writes: u64,
    /// Total number of delete operations.
    pub deletes: u64,
    /// Total number of full collection scans.
    pub scans: u64,
    /// Total number of index lookups.
    pub index_lookups: u64,
    /// Total number of transactions started.
    pub transactions_started: u64,
    /// Total number of transactions committed.
    pub transactions_committed: u64,
    /// Total number of transactions aborted.
    pub transactions_aborted: u64,
    /// Approximate entity count.
    pub entity_count: u64,
    /// Total bytes written.
    pub bytes_written: u64,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Total number of checkpoints.
    pub checkpoints: u64,
    /// Total number of errors.
    pub errors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_stats_are_zero() {
        let stats = DatabaseStats::new();
        assert_eq!(stats.reads(), 0);
        assert_eq!(stats.writes(), 0);
        assert_eq!(stats.transactions_committed(), 0);
    }

    #[test]
    fn record_operations() {
        let stats = DatabaseStats::new();

        stats.record_read(100);
        stats.record_read(50);
        assert_eq!(stats.reads(), 2);
        assert_eq!(stats.bytes_read(), 150);

        stats.record_write(200);
        assert_eq!(stats.writes(), 1);
        assert_eq!(stats.bytes_written(), 200);
    }

    #[test]
    fn record_transactions() {
        let stats = DatabaseStats::new();

        stats.record_transaction_start();
        stats.record_transaction_start();
        stats.record_transaction_commit();
        stats.record_transaction_abort();

        assert_eq!(stats.transactions_started(), 2);
        assert_eq!(stats.transactions_committed(), 1);
        assert_eq!(stats.transactions_aborted(), 1);
    }

    #[test]
    fn snapshot() {
        let stats = DatabaseStats::new();
        stats.record_read(10);
        stats.record_write(20);
        stats.record_scan();

        let snap = stats.snapshot();
        assert_eq!(snap.reads, 1);
        assert_eq!(snap.writes, 1);
        assert_eq!(snap.scans, 1);
        assert_eq!(snap.bytes_read, 10);
        assert_eq!(snap.bytes_written, 20);
    }

    #[test]
    fn concurrent_updates() {
        use std::sync::Arc;
        use std::thread;

        let stats = Arc::new(DatabaseStats::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let s = Arc::clone(&stats);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    s.record_read(1);
                    s.record_write(1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(stats.reads(), 1000);
        assert_eq!(stats.writes(), 1000);
    }
}
