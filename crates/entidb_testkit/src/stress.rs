//! Stress tests for EntiDB.
//!
//! These tests verify behavior under heavy load and concurrent access.

use entidb_core::{CollectionId, Database, EntityId, SequenceNumber};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Result of a stress test run.
#[derive(Debug, Clone)]
pub struct StressTestResult {
    /// Total operations performed.
    pub total_ops: usize,
    /// Successful operations.
    pub successful_ops: usize,
    /// Failed operations.
    pub failed_ops: usize,
    /// Total duration.
    pub duration: Duration,
    /// Operations per second.
    pub ops_per_second: f64,
}

impl StressTestResult {
    /// Creates a new result.
    pub fn new(successful: usize, failed: usize, duration: Duration) -> Self {
        let total = successful + failed;
        let ops_per_second = if duration.as_secs_f64() > 0.0 {
            total as f64 / duration.as_secs_f64()
        } else {
            0.0
        };

        Self {
            total_ops: total,
            successful_ops: successful,
            failed_ops: failed,
            duration,
            ops_per_second,
        }
    }

    /// Prints a summary of the test.
    pub fn print_summary(&self, name: &str) {
        println!("\n=== {} ===", name);
        println!("Total operations: {}", self.total_ops);
        println!("Successful: {}", self.successful_ops);
        println!("Failed: {}", self.failed_ops);
        println!("Duration: {:?}", self.duration);
        println!("Throughput: {:.2} ops/sec", self.ops_per_second);
    }
}

/// Configuration for stress tests.
#[derive(Debug, Clone)]
pub struct StressConfig {
    /// Number of operations to perform.
    pub operations: usize,
    /// Number of concurrent threads (for concurrent tests).
    pub threads: usize,
    /// Size of entity data in bytes.
    pub entity_size: usize,
    /// Number of distinct entities.
    pub entity_count: usize,
}

impl Default for StressConfig {
    fn default() -> Self {
        Self {
            operations: 10_000,
            threads: 4,
            entity_size: 256,
            entity_count: 1_000,
        }
    }
}

/// Run a sequential write stress test.
pub fn stress_sequential_writes(db: &Database, config: &StressConfig) -> StressTestResult {
    let collection = CollectionId::new(1);
    let data = vec![0xABu8; config.entity_size];

    let start = Instant::now();
    let mut successful = 0usize;
    let mut failed = 0usize;

    for i in 0..config.operations {
        let id = EntityId::from_bytes([(i % 256) as u8; 16]);

        match db.transaction(|tx| {
            tx.put(collection, id, data.clone())?;
            Ok(())
        }) {
            Ok(_) => successful += 1,
            Err(_) => failed += 1,
        }
    }

    StressTestResult::new(successful, failed, start.elapsed())
}

/// Run a sequential read stress test.
pub fn stress_sequential_reads(db: &Database, config: &StressConfig) -> StressTestResult {
    let collection = CollectionId::new(1);

    // First, populate the database
    let data = vec![0xABu8; config.entity_size];
    for i in 0..config.entity_count {
        let id = EntityId::from_bytes([(i % 256) as u8; 16]);
        let _ = db.transaction(|tx| {
            tx.put(collection, id, data.clone())?;
            Ok(())
        });
    }

    let start = Instant::now();
    let mut successful = 0usize;
    let mut failed = 0usize;

    for i in 0..config.operations {
        let id = EntityId::from_bytes([(i % config.entity_count % 256) as u8; 16]);

        match db.get(collection, id) {
            Ok(Some(_)) => successful += 1,
            Ok(None) => successful += 1, // Not found is still a successful read
            Err(_) => failed += 1,
        }
    }

    StressTestResult::new(successful, failed, start.elapsed())
}

/// Run a mixed read/write stress test.
pub fn stress_mixed_operations(db: &Database, config: &StressConfig) -> StressTestResult {
    let collection = CollectionId::new(1);
    let data = vec![0xABu8; config.entity_size];

    let start = Instant::now();
    let mut successful = 0usize;
    let mut failed = 0usize;

    for i in 0..config.operations {
        let id = EntityId::from_bytes([(i % config.entity_count % 256) as u8; 16]);

        let result = if i % 3 == 0 {
            // Write (33%)
            db.transaction(|tx| {
                tx.put(collection, id, data.clone())?;
                Ok(())
            })
        } else if i % 3 == 1 {
            // Read (33%)
            db.get(collection, id).map(|_| ())
        } else {
            // Delete (33%)
            db.transaction(|tx| {
                tx.delete(collection, id)?;
                Ok(())
            })
        };

        match result {
            Ok(_) => successful += 1,
            Err(_) => failed += 1,
        }
    }

    StressTestResult::new(successful, failed, start.elapsed())
}

/// Run a concurrent read stress test.
pub fn stress_concurrent_reads(db: Arc<Database>, config: &StressConfig) -> StressTestResult {
    let collection = CollectionId::new(1);

    // Populate database first
    let data = vec![0xABu8; config.entity_size];
    for i in 0..config.entity_count {
        let id = EntityId::from_bytes([(i % 256) as u8; 16]);
        let _ = db.transaction(|tx| {
            tx.put(collection, id, data.clone())?;
            Ok(())
        });
    }

    let successful = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let ops_per_thread = config.operations / config.threads;

    let start = Instant::now();

    let handles: Vec<_> = (0..config.threads)
        .map(|t| {
            let db = Arc::clone(&db);
            let successful = Arc::clone(&successful);
            let failed = Arc::clone(&failed);
            let entity_count = config.entity_count;

            thread::spawn(move || {
                for i in 0..ops_per_thread {
                    let idx = (t * ops_per_thread + i) % entity_count;
                    let id = EntityId::from_bytes([(idx % 256) as u8; 16]);

                    match db.get(collection, id) {
                        Ok(_) => {
                            successful.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    StressTestResult::new(
        successful.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed),
        start.elapsed(),
    )
}

/// Run a transaction abort stress test.
pub fn stress_transaction_aborts(db: &Database, config: &StressConfig) -> StressTestResult {
    let collection = CollectionId::new(1);
    let data = vec![0xABu8; config.entity_size];

    let start = Instant::now();
    let mut successful = 0usize;
    let mut failed = 0usize;

    for i in 0..config.operations {
        let id = EntityId::from_bytes([(i % 256) as u8; 16]);

        // Every other transaction will fail intentionally
        let should_fail = i % 2 == 0;

        let result = db.transaction(|tx| {
            tx.put(collection, id, data.clone())?;

            if should_fail {
                Err(entidb_core::CoreError::transaction_aborted("intentional"))
            } else {
                Ok(())
            }
        });

        match result {
            Ok(_) => successful += 1,
            Err(_) => failed += 1,
        }
    }

    StressTestResult::new(successful, failed, start.elapsed())
}

/// Run a large transaction stress test.
pub fn stress_large_transactions(db: &Database, config: &StressConfig) -> StressTestResult {
    let collection = CollectionId::new(1);
    let data = vec![0xABu8; config.entity_size];
    let batch_size = 100; // Entities per transaction

    let start = Instant::now();
    let mut successful = 0usize;
    let mut failed = 0usize;

    for batch in 0..(config.operations / batch_size) {
        let result = db.transaction(|tx| {
            for i in 0..batch_size {
                let idx = batch * batch_size + i;
                let id = EntityId::from_bytes([(idx % 256) as u8; 16]);
                tx.put(collection, id, data.clone())?;
            }
            Ok(())
        });

        match result {
            Ok(_) => successful += batch_size,
            Err(_) => failed += batch_size,
        }
    }

    StressTestResult::new(successful, failed, start.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Database {
        Database::open_in_memory().expect("Failed to create database")
    }

    #[test]
    fn test_sequential_writes() {
        let db = create_test_db();
        let config = StressConfig {
            operations: 1_000,
            entity_size: 64,
            ..Default::default()
        };

        let result = stress_sequential_writes(&db, &config);
        assert_eq!(result.failed_ops, 0);
        assert_eq!(result.successful_ops, 1_000);
    }

    #[test]
    fn test_sequential_reads() {
        let db = create_test_db();
        let config = StressConfig {
            operations: 1_000,
            entity_count: 100,
            entity_size: 64,
            ..Default::default()
        };

        let result = stress_sequential_reads(&db, &config);
        assert_eq!(result.failed_ops, 0);
    }

    #[test]
    fn test_mixed_operations() {
        let db = create_test_db();
        let config = StressConfig {
            operations: 1_000,
            entity_count: 100,
            entity_size: 64,
            ..Default::default()
        };

        let result = stress_mixed_operations(&db, &config);
        assert_eq!(result.failed_ops, 0);
    }

    #[test]
    fn test_concurrent_reads() {
        let db = Arc::new(create_test_db());
        let config = StressConfig {
            operations: 1_000,
            threads: 4,
            entity_count: 100,
            entity_size: 64,
        };

        let result = stress_concurrent_reads(db, &config);
        assert_eq!(result.failed_ops, 0);
    }

    #[test]
    fn test_transaction_aborts() {
        let db = create_test_db();
        let config = StressConfig {
            operations: 100,
            entity_size: 64,
            ..Default::default()
        };

        let result = stress_transaction_aborts(&db, &config);
        // Half should succeed, half should fail (intentionally)
        assert_eq!(result.successful_ops, 50);
        assert_eq!(result.failed_ops, 50);
    }

    #[test]
    fn test_large_transactions() {
        let db = create_test_db();
        let config = StressConfig {
            operations: 1_000,
            entity_size: 64,
            ..Default::default()
        };

        let result = stress_large_transactions(&db, &config);
        assert_eq!(result.failed_ops, 0);
    }
}
