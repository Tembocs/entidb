//! Segment store management.

use crate::error::{CoreError, CoreResult};
use crate::segment::record::SegmentRecord;
use crate::types::{CollectionId, SequenceNumber};
use entidb_storage::StorageBackend;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Manages multiple segments and provides access to entity records.
pub struct SegmentManager {
    /// Storage backend for segment data.
    backend: Arc<RwLock<Box<dyn StorageBackend>>>,
    /// Maximum segment size before sealing.
    max_segment_size: u64,
    /// In-memory index: (collection_id, entity_id) -> (offset, sequence)
    #[allow(clippy::type_complexity)]
    index: RwLock<HashMap<(u32, [u8; 16]), (u64, SequenceNumber)>>,
}

impl SegmentManager {
    /// Creates a new segment manager.
    pub fn new(backend: Box<dyn StorageBackend>, max_segment_size: u64) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            max_segment_size,
            index: RwLock::new(HashMap::new()),
        }
    }

    /// Appends a record to the current segment.
    ///
    /// Returns the offset where the record was written.
    pub fn append(&self, record: &SegmentRecord) -> CoreResult<u64> {
        let encoded = record.encode();
        let mut backend = self.backend.write();
        let offset = backend.append(&encoded)?;

        // Update in-memory index
        let key = (record.collection_id.as_u32(), record.entity_id);
        self.index.write().insert(key, (offset, record.sequence));

        Ok(offset)
    }

    /// Gets an entity by collection and entity ID.
    ///
    /// Returns `None` if the entity doesn't exist or is deleted.
    pub fn get(
        &self,
        collection_id: CollectionId,
        entity_id: &[u8; 16],
    ) -> CoreResult<Option<Vec<u8>>> {
        let key = (collection_id.as_u32(), *entity_id);
        let index = self.index.read();

        let Some(&(offset, _)) = index.get(&key) else {
            return Ok(None);
        };

        // Read the record
        let record = self.read_at(offset)?;

        if record.is_tombstone() {
            return Ok(None);
        }

        Ok(Some(record.payload))
    }

    /// Reads a record at a specific offset.
    pub fn read_at(&self, offset: u64) -> CoreResult<SegmentRecord> {
        let backend = self.backend.read();

        // First read the length
        if offset + 4 > backend.size()? {
            return Err(CoreError::segment_corruption("offset beyond segment"));
        }

        let len_bytes = backend.read_at(offset, 4)?;
        let record_len =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;

        if offset + record_len as u64 > backend.size()? {
            return Err(CoreError::segment_corruption(
                "record extends beyond segment",
            ));
        }

        let data = backend.read_at(offset, record_len)?;
        SegmentRecord::decode(&data)
    }

    /// Scans all records in the segment.
    pub fn scan_all(&self) -> CoreResult<Vec<SegmentRecord>> {
        let backend = self.backend.read();
        let size = backend.size()?;

        let mut records = Vec::new();
        let mut offset = 0u64;

        while offset < size {
            // Read length
            if offset + 4 > size {
                break;
            }

            let len_bytes = backend.read_at(offset, 4)?;
            let record_len =
                u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]])
                    as usize;

            if offset + record_len as u64 > size {
                break;
            }

            let data = backend.read_at(offset, record_len)?;
            let record = SegmentRecord::decode(&data)?;
            records.push(record);

            offset += record_len as u64;
        }

        Ok(records)
    }

    /// Rebuilds the in-memory index from segment data.
    pub fn rebuild_index(&self) -> CoreResult<()> {
        let backend = self.backend.read();
        let size = backend.size()?;

        let mut index = HashMap::new();
        let mut offset = 0u64;

        while offset < size {
            if offset + 4 > size {
                break;
            }

            let len_bytes = backend.read_at(offset, 4)?;
            let record_len =
                u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]])
                    as usize;

            if offset + record_len as u64 > size {
                break;
            }

            let data = backend.read_at(offset, record_len)?;
            let record = SegmentRecord::decode(&data)?;

            let key = (record.collection_id.as_u32(), record.entity_id);

            // Only update if this record has a higher sequence number
            let should_update = index
                .get(&key)
                .map_or(true, |&(_, existing_seq)| record.sequence > existing_seq);

            if should_update {
                index.insert(key, (offset, record.sequence));
            }

            offset += record_len as u64;
        }

        *self.index.write() = index;
        Ok(())
    }

    /// Flushes all pending writes.
    pub fn flush(&self) -> CoreResult<()> {
        self.backend.write().flush()?;
        Ok(())
    }

    /// Returns the current segment size.
    pub fn size(&self) -> CoreResult<u64> {
        Ok(self.backend.read().size()?)
    }

    /// Returns the number of indexed entities.
    pub fn entity_count(&self) -> usize {
        self.index.read().len()
    }

    /// Checks if an entity exists (including tombstones in index).
    pub fn contains(&self, collection_id: CollectionId, entity_id: &[u8; 16]) -> bool {
        let key = (collection_id.as_u32(), *entity_id);
        self.index.read().contains_key(&key)
    }

    /// Iterates over all live entities in a collection.
    pub fn iter_collection(
        &self,
        collection_id: CollectionId,
    ) -> CoreResult<Vec<([u8; 16], Vec<u8>)>> {
        let index = self.index.read();
        let mut results = Vec::new();

        for (&(col_id, entity_id), &(offset, _)) in index.iter() {
            if col_id != collection_id.as_u32() {
                continue;
            }

            let record = self.read_at(offset)?;
            if !record.is_tombstone() {
                results.push((entity_id, record.payload));
            }
        }

        Ok(results)
    }
}

impl std::fmt::Debug for SegmentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentManager")
            .field("max_segment_size", &self.max_segment_size)
            .field("entity_count", &self.entity_count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use entidb_storage::InMemoryBackend;

    fn create_manager() -> SegmentManager {
        SegmentManager::new(Box::new(InMemoryBackend::new()), 1024 * 1024)
    }

    #[test]
    fn append_and_get() {
        let manager = create_manager();
        let collection = CollectionId::new(1);
        let entity_id = [1u8; 16];
        let payload = vec![0xCA, 0xFE];

        let record = SegmentRecord::put(
            collection,
            entity_id,
            payload.clone(),
            SequenceNumber::new(1),
        );
        manager.append(&record).unwrap();

        let retrieved = manager.get(collection, &entity_id).unwrap();
        assert_eq!(retrieved, Some(payload));
    }

    #[test]
    fn get_nonexistent() {
        let manager = create_manager();
        let result = manager.get(CollectionId::new(1), &[0u8; 16]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn tombstone_hides_entity() {
        let manager = create_manager();
        let collection = CollectionId::new(1);
        let entity_id = [1u8; 16];

        // Put entity
        let put = SegmentRecord::put(collection, entity_id, vec![1, 2, 3], SequenceNumber::new(1));
        manager.append(&put).unwrap();

        // Delete it
        let tombstone = SegmentRecord::tombstone(collection, entity_id, SequenceNumber::new(2));
        manager.append(&tombstone).unwrap();

        // Should not be visible
        let result = manager.get(collection, &entity_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn latest_version_wins() {
        let manager = create_manager();
        let collection = CollectionId::new(1);
        let entity_id = [1u8; 16];

        // Version 1
        let v1 = SegmentRecord::put(collection, entity_id, vec![1], SequenceNumber::new(1));
        manager.append(&v1).unwrap();

        // Version 2
        let v2 = SegmentRecord::put(collection, entity_id, vec![2], SequenceNumber::new(2));
        manager.append(&v2).unwrap();

        // Should get version 2
        let result = manager.get(collection, &entity_id).unwrap();
        assert_eq!(result, Some(vec![2]));
    }

    #[test]
    fn scan_all_records() {
        let manager = create_manager();

        for i in 0..5u8 {
            let record = SegmentRecord::put(
                CollectionId::new(1),
                [i; 16],
                vec![i * 10],
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        let records = manager.scan_all().unwrap();
        assert_eq!(records.len(), 5);
    }

    #[test]
    fn rebuild_index() {
        let manager = create_manager();
        let collection = CollectionId::new(1);
        let entity_id = [1u8; 16];

        let record = SegmentRecord::put(collection, entity_id, vec![42], SequenceNumber::new(1));
        manager.append(&record).unwrap();

        // Clear index
        manager.index.write().clear();
        assert!(manager.get(collection, &entity_id).unwrap().is_none());

        // Rebuild
        manager.rebuild_index().unwrap();

        // Should work again
        let result = manager.get(collection, &entity_id).unwrap();
        assert_eq!(result, Some(vec![42]));
    }

    #[test]
    fn iter_collection() {
        let manager = create_manager();
        let collection = CollectionId::new(1);

        for i in 0..3u8 {
            let record = SegmentRecord::put(
                collection,
                [i; 16],
                vec![i],
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        // Add one to different collection
        let record = SegmentRecord::put(
            CollectionId::new(2),
            [99; 16],
            vec![99],
            SequenceNumber::new(10),
        );
        manager.append(&record).unwrap();

        let results = manager.iter_collection(collection).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn entity_count() {
        let manager = create_manager();

        assert_eq!(manager.entity_count(), 0);

        for i in 0..5u8 {
            let record = SegmentRecord::put(
                CollectionId::new(1),
                [i; 16],
                vec![i],
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        assert_eq!(manager.entity_count(), 5);
    }
}
