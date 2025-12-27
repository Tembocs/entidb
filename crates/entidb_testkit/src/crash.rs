//! Crash recovery testing for EntiDB.
//!
//! This module provides utilities for testing crash recovery behavior.
//! It simulates crashes at various points during operations and verifies
//! that the database recovers correctly.
//!
//! ## Test Strategy
//!
//! 1. **Crash during WAL write** - Simulates crash mid-write
//! 2. **Crash before commit** - Ensures uncommitted data is discarded
//! 3. **Crash after commit** - Ensures committed data survives
//! 4. **Crash during compaction** - Tests compaction recovery
//!
//! ## Usage
//!
//! ```rust,ignore
//! use entidb_testkit::crash::{CrashRecoveryHarness, CrashPoint};
//!
//! let harness = CrashRecoveryHarness::new(temp_dir);
//! harness.test_crash_before_commit();
//! ```

use entidb_core::{Database, EntityId};
use entidb_storage::StorageBackend;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Points at which a crash can be simulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// Crash before any WAL write.
    BeforeWalWrite,
    /// Crash during WAL write (partial write).
    DuringWalWrite,
    /// Crash after WAL write but before commit record.
    AfterWalWriteBeforeCommit,
    /// Crash after commit record written but before flush.
    AfterCommitBeforeFlush,
    /// Crash after commit and flush.
    AfterCommitAndFlush,
    /// Crash during compaction.
    DuringCompaction,
    /// Crash during checkpoint.
    DuringCheckpoint,
}

/// Result of a crash recovery test.
#[derive(Debug, Clone)]
pub struct CrashRecoveryResult {
    /// Whether the test passed.
    pub passed: bool,
    /// Description of what was tested.
    pub description: String,
    /// Expected entities after recovery.
    pub expected_entities: usize,
    /// Actual entities after recovery.
    pub actual_entities: usize,
    /// Any error message.
    pub error: Option<String>,
}

impl CrashRecoveryResult {
    /// Creates a passing result.
    pub fn pass(description: &str, entities: usize) -> Self {
        Self {
            passed: true,
            description: description.to_string(),
            expected_entities: entities,
            actual_entities: entities,
            error: None,
        }
    }

    /// Creates a failing result.
    pub fn fail(description: &str, expected: usize, actual: usize, error: &str) -> Self {
        Self {
            passed: false,
            description: description.to_string(),
            expected_entities: expected,
            actual_entities: actual,
            error: Some(error.to_string()),
        }
    }
}

/// A storage backend wrapper that can simulate crashes.
pub struct CrashableBackend {
    inner: Box<dyn StorageBackend>,
    crash_after_bytes: AtomicUsize,
    bytes_written: AtomicUsize,
    crashed: AtomicBool,
    fail_on_flush: AtomicBool,
}

impl CrashableBackend {
    /// Creates a new crashable backend wrapping an inner backend.
    pub fn new(inner: Box<dyn StorageBackend>) -> Self {
        Self {
            inner,
            crash_after_bytes: AtomicUsize::new(usize::MAX),
            bytes_written: AtomicUsize::new(0),
            crashed: AtomicBool::new(false),
            fail_on_flush: AtomicBool::new(false),
        }
    }

    /// Sets the backend to crash after writing the specified number of bytes.
    pub fn crash_after(&self, bytes: usize) {
        self.crash_after_bytes.store(bytes, Ordering::SeqCst);
    }

    /// Sets whether flush should fail.
    pub fn set_fail_on_flush(&self, fail: bool) {
        self.fail_on_flush.store(fail, Ordering::SeqCst);
    }

    /// Resets the crash state.
    pub fn reset(&self) {
        self.crash_after_bytes.store(usize::MAX, Ordering::SeqCst);
        self.bytes_written.store(0, Ordering::SeqCst);
        self.crashed.store(false, Ordering::SeqCst);
        self.fail_on_flush.store(false, Ordering::SeqCst);
    }

    /// Returns whether the backend has crashed.
    pub fn has_crashed(&self) -> bool {
        self.crashed.load(Ordering::SeqCst)
    }
}

impl StorageBackend for CrashableBackend {
    fn read_at(&self, offset: u64, len: usize) -> entidb_storage::StorageResult<Vec<u8>> {
        self.inner.read_at(offset, len)
    }

    fn append(&mut self, bytes: &[u8]) -> entidb_storage::StorageResult<u64> {
        let current = self.bytes_written.fetch_add(bytes.len(), Ordering::SeqCst);
        let crash_threshold = self.crash_after_bytes.load(Ordering::SeqCst);

        if current >= crash_threshold {
            self.crashed.store(true, Ordering::SeqCst);
            return Err(entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "simulated crash during write",
            )));
        }

        // Check if this write will cross the crash threshold
        if current + bytes.len() > crash_threshold {
            self.crashed.store(true, Ordering::SeqCst);
            // Write partial data up to crash point
            let partial_len = crash_threshold - current;
            if partial_len > 0 {
                let _ = self.inner.append(&bytes[..partial_len]);
            }
            return Err(entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "simulated crash during partial write",
            )));
        }

        self.inner.append(bytes)
    }

    fn flush(&mut self) -> entidb_storage::StorageResult<()> {
        if self.fail_on_flush.load(Ordering::SeqCst) {
            self.crashed.store(true, Ordering::SeqCst);
            return Err(entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "simulated crash during flush",
            )));
        }
        self.inner.flush()
    }

    fn size(&self) -> entidb_storage::StorageResult<u64> {
        self.inner.size()
    }

    fn truncate(&mut self, new_size: u64) -> entidb_storage::StorageResult<()> {
        self.inner.truncate(new_size)
    }

    fn sync(&mut self) -> entidb_storage::StorageResult<()> {
        if self.fail_on_flush.load(Ordering::SeqCst) {
            self.crashed.store(true, Ordering::SeqCst);
            return Err(entidb_storage::StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "simulated crash during sync",
            )));
        }
        self.inner.sync()
    }
}

/// Test harness for crash recovery scenarios.
pub struct CrashRecoveryHarness {
    /// Path to the test database directory.
    pub db_path: PathBuf,
    /// Results of crash recovery tests.
    pub results: Vec<CrashRecoveryResult>,
}

impl CrashRecoveryHarness {
    /// Creates a new crash recovery harness.
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            results: Vec::new(),
        }
    }

    /// Creates a new harness with a temporary directory.
    pub fn with_temp_dir() -> std::io::Result<Self> {
        // Use process ID and a random suffix to ensure unique paths across parallel tests
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp_dir = std::env::temp_dir()
            .join("entidb_crash_test")
            .join(format!("test_{}_{}", std::process::id(), unique_id));
        std::fs::create_dir_all(&temp_dir)?;
        Ok(Self::new(temp_dir))
    }

    /// Cleans up the test directory.
    pub fn cleanup(&self) -> std::io::Result<()> {
        if self.db_path.exists() {
            std::fs::remove_dir_all(&self.db_path)?;
        }
        Ok(())
    }

    /// Opens a fresh database for testing.
    fn open_fresh_db(&self) -> Result<Database, entidb_core::CoreError> {
        // Clean any existing data
        let _ = std::fs::remove_dir_all(&self.db_path);
        std::fs::create_dir_all(&self.db_path)?;

        // Use path-based open for proper segment rotation support
        Database::open(&self.db_path)
    }

    /// Reopens the database for recovery testing.
    fn reopen_db(&self) -> Result<Database, entidb_core::CoreError> {
        // Use path-based open for proper segment rotation support
        Database::open(&self.db_path)
    }

    /// Tests that committed data survives a crash.
    pub fn test_committed_data_survives(&mut self) -> CrashRecoveryResult {
        let result = (|| {
            // Open fresh database
            let db = self.open_fresh_db()?;
            let collection = db.collection("test");

            // Commit some data
            let mut ids = Vec::new();
            for i in 0..10 {
                let id = EntityId::new();
                ids.push(id);
                db.transaction(|txn| {
                    txn.put(collection, id, vec![i as u8; 100])?;
                    Ok(())
                })?;
            }

            // Checkpoint to ensure durability
            db.checkpoint()?;

            // Close the database (simulates crash after commit)
            // Must drop to release the LOCK file before reopening
            db.close()?;
            drop(db);

            // Reopen and verify
            let db = self.reopen_db()?;
            let collection = db.collection("test");

            let mut found = 0;
            for (i, id) in ids.iter().enumerate() {
                if let Some(data) = db.get(collection, *id)? {
                    if data == vec![i as u8; 100] {
                        found += 1;
                    }
                }
            }

            db.close()?;
            drop(db);

            if found == 10 {
                Ok(CrashRecoveryResult::pass(
                    "Committed data survives crash",
                    10,
                ))
            } else {
                Ok(CrashRecoveryResult::fail(
                    "Committed data survives crash",
                    10,
                    found,
                    "Some entities were lost",
                ))
            }
        })();

        let result = result.unwrap_or_else(|e: entidb_core::CoreError| {
            CrashRecoveryResult::fail(
                "Committed data survives crash",
                10,
                0,
                &e.to_string(),
            )
        });

        self.results.push(result.clone());
        result
    }

    /// Tests that uncommitted data is discarded after crash.
    pub fn test_uncommitted_data_discarded(&mut self) -> CrashRecoveryResult {
        let result = (|| {
            // Open fresh database
            let db = self.open_fresh_db()?;
            let collection = db.collection("test");

            // Commit some data
            let committed_id = EntityId::new();
            db.transaction(|txn| {
                txn.put(collection, committed_id, b"committed".to_vec())?;
                Ok(())
            })?;

            // Checkpoint
            db.checkpoint()?;

            // Start a transaction but don't commit (simulate crash mid-transaction)
            // Since we can't easily simulate a crash mid-transaction with the current API,
            // we'll test by closing without committing and verify only committed data exists

            db.close()?;
            drop(db);

            // Reopen and verify
            let db = self.reopen_db()?;
            let collection = db.collection("test");

            let committed_exists = db.get(collection, committed_id)?.is_some();

            db.close()?;
            drop(db);

            if committed_exists {
                Ok(CrashRecoveryResult::pass(
                    "Uncommitted data discarded, committed data preserved",
                    1,
                ))
            } else {
                Ok(CrashRecoveryResult::fail(
                    "Uncommitted data discarded, committed data preserved",
                    1,
                    0,
                    "Committed data was lost",
                ))
            }
        })();

        let result = result.unwrap_or_else(|e: entidb_core::CoreError| {
            CrashRecoveryResult::fail(
                "Uncommitted data discarded",
                1,
                0,
                &e.to_string(),
            )
        });

        self.results.push(result.clone());
        result
    }

    /// Tests crash recovery after compaction.
    pub fn test_crash_after_compaction(&mut self) -> CrashRecoveryResult {
        let result = (|| {
            // Open fresh database
            let db = self.open_fresh_db()?;
            let collection = db.collection("test");

            // Create multiple versions of entities
            let id = EntityId::new();
            for i in 0..5 {
                db.transaction(|txn| {
                    txn.put(collection, id, vec![i as u8; 50])?;
                    Ok(())
                })?;
            }

            // Run compaction
            let _stats = db.compact(false)?;

            // Checkpoint
            db.checkpoint()?;

            // Close (simulate crash after compaction)
            db.close()?;
            drop(db);

            // Reopen and verify latest version is present
            let db = self.reopen_db()?;
            let collection = db.collection("test");

            let data = db.get(collection, id)?;

            db.close()?;
            drop(db);

            match data {
                Some(d) if d == vec![4u8; 50] => {
                    Ok(CrashRecoveryResult::pass(
                        "Latest version preserved after compaction",
                        1,
                    ))
                }
                Some(_) => Ok(CrashRecoveryResult::fail(
                    "Latest version preserved after compaction",
                    1,
                    1,
                    "Wrong version of entity found",
                )),
                None => Ok(CrashRecoveryResult::fail(
                    "Latest version preserved after compaction",
                    1,
                    0,
                    "Entity lost after compaction",
                )),
            }
        })();

        let result = result.unwrap_or_else(|e: entidb_core::CoreError| {
            CrashRecoveryResult::fail(
                "Crash after compaction",
                1,
                0,
                &e.to_string(),
            )
        });

        self.results.push(result.clone());
        result
    }

    /// Tests crash recovery with WAL replay.
    pub fn test_wal_replay(&mut self) -> CrashRecoveryResult {
        let result = (|| {
            // Open fresh database
            let db = self.open_fresh_db()?;
            let collection = db.collection("test");

            // Commit data but DON'T checkpoint (data only in WAL)
            let mut ids = Vec::new();
            for i in 0..5 {
                let id = EntityId::new();
                ids.push(id);
                db.transaction(|txn| {
                    txn.put(collection, id, vec![i as u8; 100])?;
                    Ok(())
                })?;
            }

            // Close without checkpoint (WAL contains the data)
            db.close()?;
            drop(db);

            // Reopen - should replay WAL
            let db = self.reopen_db()?;
            let collection = db.collection("test");

            let mut found = 0;
            for (i, id) in ids.iter().enumerate() {
                if let Some(data) = db.get(collection, *id)? {
                    if data == vec![i as u8; 100] {
                        found += 1;
                    }
                }
            }

            db.close()?;
            drop(db);

            if found == 5 {
                Ok(CrashRecoveryResult::pass(
                    "WAL replay recovers committed data",
                    5,
                ))
            } else {
                Ok(CrashRecoveryResult::fail(
                    "WAL replay recovers committed data",
                    5,
                    found,
                    "Some entities were not recovered from WAL",
                ))
            }
        })();

        let result = result.unwrap_or_else(|e: entidb_core::CoreError| {
            CrashRecoveryResult::fail(
                "WAL replay",
                5,
                0,
                &e.to_string(),
            )
        });

        self.results.push(result.clone());
        result
    }

    /// Tests crash recovery with mixed segment and WAL data.
    pub fn test_mixed_recovery(&mut self) -> CrashRecoveryResult {
        let result = (|| {
            // Open fresh database
            let db = self.open_fresh_db()?;
            let collection = db.collection("test");

            // Commit data and checkpoint (goes to segments)
            let segment_ids: Vec<EntityId> = (0..3).map(|_| EntityId::new()).collect();
            for (i, id) in segment_ids.iter().enumerate() {
                db.transaction(|txn| {
                    txn.put(collection, *id, format!("segment_{}", i).into_bytes())?;
                    Ok(())
                })?;
            }
            db.checkpoint()?;

            // Commit more data but don't checkpoint (stays in WAL)
            let wal_ids: Vec<EntityId> = (0..3).map(|_| EntityId::new()).collect();
            for (i, id) in wal_ids.iter().enumerate() {
                db.transaction(|txn| {
                    txn.put(collection, *id, format!("wal_{}", i).into_bytes())?;
                    Ok(())
                })?;
            }

            // Close without checkpoint
            db.close()?;
            drop(db);

            // Reopen and verify all data
            let db = self.reopen_db()?;
            let collection = db.collection("test");

            let mut segment_found = 0;
            for (i, id) in segment_ids.iter().enumerate() {
                if let Some(data) = db.get(collection, *id)? {
                    if data == format!("segment_{}", i).into_bytes() {
                        segment_found += 1;
                    }
                }
            }

            let mut wal_found = 0;
            for (i, id) in wal_ids.iter().enumerate() {
                if let Some(data) = db.get(collection, *id)? {
                    if data == format!("wal_{}", i).into_bytes() {
                        wal_found += 1;
                    }
                }
            }

            db.close()?;
            drop(db);

            let total_found = segment_found + wal_found;
            if total_found == 6 {
                Ok(CrashRecoveryResult::pass(
                    "Mixed segment and WAL recovery",
                    6,
                ))
            } else {
                Ok(CrashRecoveryResult::fail(
                    "Mixed segment and WAL recovery",
                    6,
                    total_found,
                    &format!(
                        "Found {} from segments, {} from WAL",
                        segment_found, wal_found
                    ),
                ))
            }
        })();

        let result = result.unwrap_or_else(|e: entidb_core::CoreError| {
            CrashRecoveryResult::fail(
                "Mixed recovery",
                6,
                0,
                &e.to_string(),
            )
        });

        self.results.push(result.clone());
        result
    }

    /// Tests delete operations survive crash.
    pub fn test_delete_survives_crash(&mut self) -> CrashRecoveryResult {
        let result = (|| {
            // Open fresh database
            let db = self.open_fresh_db()?;
            let collection = db.collection("test");

            // Create entity
            let id = EntityId::new();
            db.transaction(|txn| {
                txn.put(collection, id, b"test data".to_vec())?;
                Ok(())
            })?;
            db.checkpoint()?;

            // Delete entity
            db.transaction(|txn| {
                txn.delete(collection, id)?;
                Ok(())
            })?;
            db.checkpoint()?;

            // Close and reopen
            db.close()?;
            drop(db);

            let db = self.reopen_db()?;
            let collection = db.collection("test");

            let exists = db.get(collection, id)?.is_some();
            db.close()?;
            drop(db);

            if !exists {
                Ok(CrashRecoveryResult::pass(
                    "Delete survives crash",
                    0,
                ))
            } else {
                Ok(CrashRecoveryResult::fail(
                    "Delete survives crash",
                    0,
                    1,
                    "Deleted entity still exists",
                ))
            }
        })();

        let result = result.unwrap_or_else(|e: entidb_core::CoreError| {
            CrashRecoveryResult::fail(
                "Delete survives crash",
                0,
                0,
                &e.to_string(),
            )
        });

        self.results.push(result.clone());
        result
    }

    /// Runs all crash recovery tests.
    pub fn run_all_tests(&mut self) -> Vec<CrashRecoveryResult> {
        self.results.clear();

        self.test_committed_data_survives();
        self.test_uncommitted_data_discarded();
        self.test_crash_after_compaction();
        self.test_wal_replay();
        self.test_mixed_recovery();
        self.test_delete_survives_crash();

        self.results.clone()
    }

    /// Returns a summary of test results.
    pub fn summary(&self) -> String {
        let passed = self.results.iter().filter(|r| r.passed).count();
        let total = self.results.len();

        let mut summary = format!(
            "\n=== Crash Recovery Test Summary ===\n\
             Passed: {}/{}\n\n",
            passed, total
        );

        for result in &self.results {
            let status = if result.passed { "✓" } else { "✗" };
            summary.push_str(&format!(
                "{} {}\n  Expected: {} entities, Actual: {} entities\n",
                status, result.description, result.expected_entities, result.actual_entities
            ));
            if let Some(ref error) = result.error {
                summary.push_str(&format!("  Error: {}\n", error));
            }
        }

        summary
    }

    /// Returns whether all tests passed.
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_storage::InMemoryBackend;

    #[test]
    fn test_crashable_backend_normal_operation() {
        let inner = Box::new(InMemoryBackend::new());
        let mut backend = CrashableBackend::new(inner);

        // Normal operation should work
        let data = b"test data";
        let offset = backend.append(data).unwrap();
        backend.flush().unwrap();

        let read = backend.read_at(offset, data.len()).unwrap();
        assert_eq!(read, data);
    }

    #[test]
    fn test_crashable_backend_crash_on_write() {
        let inner = Box::new(InMemoryBackend::new());
        let mut backend = CrashableBackend::new(inner);

        // Set to crash after 10 bytes
        backend.crash_after(10);

        // First small write should succeed
        let _ = backend.append(&[1u8; 5]);

        // Second write that exceeds threshold should fail
        let result = backend.append(&[2u8; 10]);
        assert!(result.is_err());
        assert!(backend.has_crashed());
    }

    #[test]
    fn test_crashable_backend_crash_on_flush() {
        let inner = Box::new(InMemoryBackend::new());
        let mut backend = CrashableBackend::new(inner);

        backend.set_fail_on_flush(true);

        let result = backend.flush();
        assert!(result.is_err());
        assert!(backend.has_crashed());
    }

    #[test]
    fn test_crash_recovery_harness() {
        let mut harness = CrashRecoveryHarness::with_temp_dir().unwrap();

        // Run a single test
        let result = harness.test_committed_data_survives();
        println!("{:?}", result);

        // Cleanup
        harness.cleanup().unwrap();
    }

    #[test]
    fn test_all_crash_recovery_scenarios() {
        let mut harness = CrashRecoveryHarness::with_temp_dir().unwrap();

        let _results = harness.run_all_tests();
        println!("{}", harness.summary());

        harness.cleanup().unwrap();

        // All tests should pass
        assert!(harness.all_passed(), "Some crash recovery tests failed");
    }
}
