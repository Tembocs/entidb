//! Cross-crate integration test helpers.
//!
//! Provides utilities for testing interactions between
//! multiple EntiDB crates.

use entidb_codec::CanonicalEncoder;
use entidb_core::{CollectionId, CoreError, Database, EntityId};
use entidb_storage::StorageBackend;
use std::collections::HashMap;

/// A test harness for integration testing.
pub struct IntegrationHarness {
    /// The database instance.
    pub db: Database,
    /// Entity tracking for verification.
    entities: HashMap<(CollectionId, EntityId), Vec<u8>>,
}

impl IntegrationHarness {
    /// Creates a new integration harness with an in-memory database.
    pub fn new() -> Self {
        Self {
            db: Database::open_in_memory().expect("Failed to open database"),
            entities: HashMap::new(),
        }
    }

    /// Puts an entity and tracks it for later verification.
    pub fn put(&mut self, collection: CollectionId, id: EntityId, data: Vec<u8>) {
        self.db
            .transaction(|txn| {
                txn.put(collection, id, data.clone())?;
                Ok(())
            })
            .expect("Failed to put entity");
        self.entities.insert((collection, id), data);
    }

    /// Gets an entity and verifies it matches the tracked value.
    pub fn get_and_verify(&self, collection: CollectionId, id: EntityId) -> Option<Vec<u8>> {
        let actual = self.db.get(collection, id).expect("Failed to get entity");

        if let Some(expected) = self.entities.get(&(collection, id)) {
            assert_eq!(
                actual.as_ref(),
                Some(expected),
                "Entity data mismatch for {:?}",
                id
            );
        }

        actual
    }

    /// Deletes an entity and updates tracking.
    pub fn delete(&mut self, collection: CollectionId, id: EntityId) {
        self.db
            .transaction(|txn| {
                txn.delete(collection, id)?;
                Ok(())
            })
            .expect("Failed to delete entity");
        self.entities.remove(&(collection, id));
    }

    /// Verifies all tracked entities are in the database.
    pub fn verify_all(&self) {
        for ((collection, id), expected) in &self.entities {
            let actual = self.db.get(*collection, *id).expect("Failed to get entity");
            assert_eq!(
                actual.as_ref(),
                Some(expected),
                "Entity data mismatch for {:?}",
                id
            );
        }
    }

    /// Returns the count of tracked entities.
    pub fn tracked_count(&self) -> usize {
        self.entities.len()
    }
}

impl Default for IntegrationHarness {
    fn default() -> Self {
        Self::new()
    }
}

/// Test codec/storage integration.
pub mod codec_storage {
    use super::*;
    use entidb_codec::Value;

    /// Verifies that CBOR data can be encoded, stored, and retrieved.
    pub fn test_encode_store_retrieve(db: &Database, collection: CollectionId, value: Value) {
        let mut encoder = CanonicalEncoder::new();
        encoder.encode(&value).expect("Failed to encode");
        let encoded = encoder.into_bytes();

        let id = EntityId::new();

        db.transaction(|txn| {
            txn.put(collection, id, encoded.clone())?;
            Ok(())
        })
        .expect("Failed to put");

        let retrieved = db.get(collection, id).expect("Failed to get");
        assert!(retrieved.is_some(), "Entity should exist");
        assert_eq!(
            encoded,
            retrieved.unwrap(),
            "Retrieved bytes should match encoded"
        );
    }

    /// Tests that the storage backend properly persists data.
    pub fn test_storage_persistence(backend: &mut dyn StorageBackend, data: &[u8]) {
        let offset = backend.append(data).expect("Failed to append");
        backend.flush().expect("Failed to flush");

        let retrieved = backend.read_at(offset, data.len()).expect("Failed to read");
        assert_eq!(data, &retrieved[..], "Retrieved data should match");
    }
}

/// Test transaction integration.
pub mod transaction {
    use super::*;

    /// Tests that transactions are properly isolated.
    pub fn test_transaction_isolation(db: &Database) {
        let collection = db.collection("isolation_test");
        let id = EntityId::new();
        let data1 = b"version1".to_vec();
        let data2 = b"version2".to_vec();

        // Put initial data
        db.transaction(|txn| {
            txn.put(collection, id, data1.clone())?;
            Ok(())
        })
        .expect("Failed to put initial data");

        // Start a read snapshot
        let snapshot_data = db.get(collection, id).expect("Failed to get");
        assert_eq!(snapshot_data, Some(data1.clone()));

        // Update in another transaction
        db.transaction(|txn| {
            txn.put(collection, id, data2.clone())?;
            Ok(())
        })
        .expect("Failed to update");

        // New read should see the update
        let new_data = db.get(collection, id).expect("Failed to get");
        assert_eq!(new_data, Some(data2));
    }

    /// Tests that aborted transactions don't affect the database.
    pub fn test_transaction_abort(db: &Database) {
        let collection = db.collection("abort_test");
        let id = EntityId::new();
        let original = b"original".to_vec();
        let modified = b"modified".to_vec();

        // Put initial data
        db.transaction(|txn| {
            txn.put(collection, id, original.clone())?;
            Ok(())
        })
        .expect("Failed to put initial data");

        // Try to update but abort (return error to simulate abort)
        let result: Result<(), CoreError> = db.transaction(|txn| {
            txn.put(collection, id, modified)?;
            Err(CoreError::InvalidOperation {
                message: "Simulated abort".into(),
            })
        });
        assert!(result.is_err());

        // Data should be unchanged
        let data = db.get(collection, id).expect("Failed to get");
        assert_eq!(data, Some(original));
    }
}

/// Test index integration.
pub mod index {
    use super::*;

    /// Tests that indexes are updated correctly with entity changes.
    pub fn test_index_consistency(db: &Database) {
        let collection = db.collection("index_test");

        // Add several entities
        let mut ids = Vec::new();
        for i in 0..10 {
            let id = EntityId::new();
            let data = format!(r#"{{"value":{}}}"#, i).into_bytes();
            db.transaction(|txn| {
                txn.put(collection, id, data)?;
                Ok(())
            })
            .expect("Failed to put");
            ids.push(id);
        }

        // Delete some
        for id in ids.iter().take(5) {
            db.transaction(|txn| {
                txn.delete(collection, *id)?;
                Ok(())
            })
            .expect("Failed to delete");
        }

        // Verify remaining entities are accessible
        for id in ids.iter().skip(5) {
            let data = db.get(collection, *id).expect("Failed to get");
            assert!(data.is_some(), "Entity should exist");
        }

        // Verify deleted entities are gone
        for id in ids.iter().take(5) {
            let data = db.get(collection, *id).expect("Failed to get");
            assert!(data.is_none(), "Entity should not exist");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_harness() {
        let mut harness = IntegrationHarness::new();
        let collection = harness.db.collection("test");
        let id = EntityId::new();
        let data = b"test data".to_vec();

        harness.put(collection, id, data.clone());
        assert_eq!(harness.tracked_count(), 1);

        let retrieved = harness.get_and_verify(collection, id);
        assert_eq!(retrieved, Some(data));

        harness.verify_all();
    }

    #[test]
    fn test_transaction_isolation() {
        let db = Database::open_in_memory().expect("Failed to open database");
        transaction::test_transaction_isolation(&db);
    }

    #[test]
    fn test_transaction_abort() {
        let db = Database::open_in_memory().expect("Failed to open database");
        transaction::test_transaction_abort(&db);
    }

    #[test]
    fn test_index_consistency() {
        let db = Database::open_in_memory().expect("Failed to open database");
        index::test_index_consistency(&db);
    }
}
