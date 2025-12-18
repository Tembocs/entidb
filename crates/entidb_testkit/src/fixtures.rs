//! Test fixtures and database helpers.
//!
//! Provides convenience functions for setting up test databases
//! and common test scenarios.

use entidb_core::{Config, Database};
use entidb_storage::FileBackend;
use std::path::PathBuf;
use tempfile::TempDir;

/// A test database with automatic cleanup.
pub struct TestDatabase {
    /// The database instance.
    pub db: Database,
    /// The temporary directory (kept alive to prevent cleanup).
    _temp_dir: Option<TempDir>,
}

impl TestDatabase {
    /// Creates a new in-memory test database.
    pub fn memory() -> Self {
        Self {
            db: Database::open_in_memory().expect("Failed to open in-memory database"),
            _temp_dir: None,
        }
    }

    /// Creates a new file-based test database.
    pub fn file() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let wal_path = temp_dir.path().join("wal.log");
        let segment_path = temp_dir.path().join("segments.dat");
        
        let wal_backend = FileBackend::open_with_create_dirs(&wal_path)
            .expect("Failed to create WAL backend");
        let segment_backend = FileBackend::open_with_create_dirs(&segment_path)
            .expect("Failed to create segment backend");
        
        let db = Database::open_with_backends(
            Config::default(),
            Box::new(wal_backend),
            Box::new(segment_backend),
        )
        .expect("Failed to open file database");
        
        Self {
            db,
            _temp_dir: Some(temp_dir),
        }
    }

    /// Returns the database path if file-based, None if in-memory.
    pub fn path(&self) -> Option<PathBuf> {
        self._temp_dir.as_ref().map(|d| d.path().join("test.entidb"))
    }
}

impl std::ops::Deref for TestDatabase {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.db
    }
}

impl std::ops::DerefMut for TestDatabase {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.db
    }
}

/// Runs a test with a temporary in-memory database.
///
/// # Example
///
/// ```rust,ignore
/// use entidb_testkit::with_temp_db;
///
/// #[test]
/// fn my_test() {
///     with_temp_db(|db| {
///         let collection = db.collection("test");
///         // ... test operations
///     });
/// }
/// ```
pub fn with_temp_db<F, R>(f: F) -> R
where
    F: FnOnce(&Database) -> R,
{
    let test_db = TestDatabase::memory();
    f(&test_db.db)
}

/// Runs a test with a temporary file-based database.
pub fn with_file_db<F, R>(f: F) -> R
where
    F: FnOnce(&Database, &std::path::Path) -> R,
{
    let test_db = TestDatabase::file();
    let path = test_db.path().expect("File database should have a path");
    f(&test_db.db, &path)
}

/// Runs a mutable test with a temporary database.
pub fn with_temp_db_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut Database) -> R,
{
    let mut test_db = TestDatabase::memory();
    f(&mut test_db.db)
}

/// Test scenario helpers.
pub mod scenarios {
    use super::*;
    use entidb_core::{CollectionId, EntityId};

    /// Creates a database with some pre-populated data.
    pub fn populated_database(entity_count: usize) -> TestDatabase {
        let test_db = TestDatabase::memory();
        let collection_id = test_db.db.collection("test");

        for i in 0..entity_count {
            let entity_id = EntityId::new();
            let data = format!(r#"{{"index":{}}}"#, i).into_bytes();
            test_db
                .db
                .transaction(|txn| {
                    txn.put(collection_id, entity_id, data)?;
                    Ok(())
                })
                .expect("Failed to put entity");
        }

        test_db
    }

    /// Creates a database with multiple collections.
    pub fn multi_collection_database(collection_count: usize) -> (TestDatabase, Vec<CollectionId>) {
        let test_db = TestDatabase::memory();
        let mut collections = Vec::with_capacity(collection_count);

        for i in 0..collection_count {
            let name = format!("collection_{}", i);
            let collection_id = test_db.db.collection(&name);
            collections.push(collection_id);

            // Add one entity per collection
            let entity_id = EntityId::new();
            let data = format!(r#"{{"collection":{}}}"#, i).into_bytes();
            test_db
                .db
                .transaction(|txn| {
                    txn.put(collection_id, entity_id, data)?;
                    Ok(())
                })
                .expect("Failed to put entity");
        }

        (test_db, collections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_database() {
        let test_db = TestDatabase::memory();
        let collection = test_db.collection("test");
        // Verify collection was created - just ensure it doesn't panic
        let _ = collection;
    }

    #[test]
    fn test_with_temp_db() {
        with_temp_db(|db| {
            let collection = db.collection("test");
            // Verify collection was created (ID will be > 0 for non-default)
            let _ = collection; // Just ensure it works
        });
    }

    #[test]
    fn test_populated_scenario() {
        let test_db = scenarios::populated_database(10);
        // Database should be usable
        let _collection = test_db.collection("test");
    }
}
