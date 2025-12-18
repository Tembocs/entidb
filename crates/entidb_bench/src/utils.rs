//! Benchmark utilities.

use entidb_core::EntityId;
use rand::Rng;

/// Generate random entity data of the specified size.
pub fn random_data(size: usize) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0..size).map(|_| rng.gen()).collect()
}

/// Generate a batch of entity IDs.
pub fn generate_ids(count: usize) -> Vec<EntityId> {
    (0..count).map(|_| EntityId::new()).collect()
}

/// Generate test entities with specified payload size.
pub fn generate_entities(count: usize, payload_size: usize) -> Vec<(EntityId, Vec<u8>)> {
    let ids = generate_ids(count);
    ids.into_iter()
        .map(|id| (id, random_data(payload_size)))
        .collect()
}
