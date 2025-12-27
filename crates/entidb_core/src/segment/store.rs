//! Segment store management.
//!
//! Segments are immutable, append-only files that store entity records.
//! The [`SegmentManager`] handles:
//! - Multiple segment files
//! - Auto-sealing when segments exceed `max_segment_size`
//! - Segment rotation (creating new segments)
//! - Index maintenance across segments

use crate::error::{CoreError, CoreResult};
use crate::segment::record::SegmentRecord;
use crate::types::{CollectionId, SequenceNumber};
use entidb_storage::StorageBackend;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Information about a segment file.
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    /// Segment ID (monotonically increasing).
    pub id: u64,
    /// Whether this segment is sealed (immutable).
    pub sealed: bool,
    /// Current size in bytes.
    pub size: u64,
    /// Number of records in this segment.
    pub record_count: usize,
}

/// Entry in the segment index, tracking where to find an entity version.
#[derive(Debug, Clone, Copy)]
struct IndexEntry {
    /// Segment ID containing this entity.
    segment_id: u64,
    /// Offset within the segment.
    offset: u64,
    /// Sequence number of this version.
    sequence: SequenceNumber,
}

/// Version chain for MVCC - stores multiple versions of an entity.
/// Entries are kept sorted by sequence number in descending order (newest first).
#[derive(Debug, Clone, Default)]
struct VersionChain {
    /// All versions, sorted by sequence descending (newest first).
    versions: Vec<IndexEntry>,
}

impl VersionChain {
    /// Creates a new empty version chain.
    fn new() -> Self {
        Self {
            versions: Vec::new(),
        }
    }

    /// Adds a new version to the chain, maintaining descending order by sequence.
    fn add(&mut self, entry: IndexEntry) {
        // Find insertion point to maintain descending order
        let pos = self
            .versions
            .iter()
            .position(|e| e.sequence < entry.sequence)
            .unwrap_or(self.versions.len());
        self.versions.insert(pos, entry);
    }

    /// Gets the version visible at the given snapshot sequence.
    /// Returns the newest version where sequence <= max_sequence.
    fn get_at(&self, max_sequence: SequenceNumber) -> Option<&IndexEntry> {
        self.versions
            .iter()
            .find(|e| e.sequence <= max_sequence)
    }

    /// Gets the latest version (for reads without snapshot).
    fn latest(&self) -> Option<&IndexEntry> {
        self.versions.first()
    }

    /// Returns true if the chain has any versions.
    fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// Returns the latest sequence number in this chain.
    #[allow(dead_code)]
    fn latest_sequence(&self) -> Option<SequenceNumber> {
        self.versions.first().map(|e| e.sequence)
    }

    /// Removes versions older than min_sequence (for garbage collection).
    /// Keeps at least one version (the latest).
    #[allow(dead_code)]
    fn prune_before(&mut self, min_sequence: SequenceNumber) {
        if self.versions.len() <= 1 {
            return;
        }
        // Keep versions that are >= min_sequence, but always keep at least the latest
        let keep_count = self
            .versions
            .iter()
            .take_while(|e| e.sequence >= min_sequence)
            .count()
            .max(1);
        self.versions.truncate(keep_count);
    }
}

/// Manages multiple segments and provides access to entity records.
///
/// The `SegmentManager` automatically:
/// - Seals segments when they exceed `max_segment_size`
/// - Creates new segments for writes after sealing
/// - Maintains an in-memory index across all segments
/// - Provides consistent reads from any segment
///
/// # Auto-Sealing
///
/// When a write would cause the active segment to exceed `max_segment_size`,
/// the manager seals the current segment (making it immutable) and creates
/// a new active segment for future writes.
///
/// # Example
///
/// ```ignore
/// use entidb_core::segment::SegmentManager;
/// use entidb_storage::InMemoryBackend;
///
/// // Create manager with 1MB max segment size
/// let manager = SegmentManager::new(
///     |_| Box::new(InMemoryBackend::new()),
///     1024 * 1024,
/// );
///
/// // Write records - manager handles rotation automatically
/// manager.append(&record)?;
///
/// // Check segment info
/// let segments = manager.list_segments();
/// for seg in &segments {
///     println!("Segment {}: {} bytes, sealed: {}", seg.id, seg.size, seg.sealed);
/// }
/// ```
pub struct SegmentManager {
    /// Factory function to create storage backends for new segments.
    backend_factory: Box<dyn Fn(u64) -> Box<dyn StorageBackend> + Send + Sync>,
    /// All segment backends, keyed by segment ID.
    segments: RwLock<HashMap<u64, Arc<RwLock<Box<dyn StorageBackend>>>>>,
    /// Segment metadata.
    segment_info: RwLock<HashMap<u64, SegmentInfo>>,
    /// Current active segment ID (the one receiving writes).
    active_segment_id: RwLock<u64>,
    /// Maximum segment size before sealing.
    max_segment_size: u64,
    /// MVCC index: (collection_id, entity_id) -> VersionChain
    /// Each entity has a chain of versions for snapshot isolation.
    #[allow(clippy::type_complexity)]
    index: RwLock<HashMap<(u32, [u8; 16]), VersionChain>>,
    /// Callback for when a segment is sealed.
    on_segment_sealed: RwLock<Option<Box<dyn Fn(u64) + Send + Sync>>>,
    /// Compaction lock to coordinate between compaction and segment sealing.
    /// When held, prevents new segments from being sealed during compaction.
    compaction_lock: parking_lot::Mutex<()>,
}

impl SegmentManager {
    /// Creates a new segment manager with a factory function.
    ///
    /// This will start with segment ID 1. For recovering existing segments
    /// from disk, use [`with_factory_and_existing`] instead.
    ///
    /// # Arguments
    ///
    /// * `backend_factory` - Function that creates a storage backend for a segment ID
    /// * `max_segment_size` - Maximum size before auto-sealing
    pub fn with_factory<F>(backend_factory: F, max_segment_size: u64) -> Self
    where
        F: Fn(u64) -> Box<dyn StorageBackend> + Send + Sync + 'static,
    {
        let initial_id = 1;
        let initial_backend = backend_factory(initial_id);
        let initial_size = initial_backend.size().unwrap_or(0);

        let mut segments = HashMap::new();
        segments.insert(initial_id, Arc::new(RwLock::new(initial_backend)));

        let mut segment_info = HashMap::new();
        segment_info.insert(
            initial_id,
            SegmentInfo {
                id: initial_id,
                sealed: false,
                size: initial_size,
                record_count: 0,
            },
        );

        Self {
            backend_factory: Box::new(backend_factory),
            segments: RwLock::new(segments),
            segment_info: RwLock::new(segment_info),
            active_segment_id: RwLock::new(initial_id),
            max_segment_size,
            index: RwLock::new(HashMap::new()),
            on_segment_sealed: RwLock::new(None),
            compaction_lock: parking_lot::Mutex::new(()),
        }
    }

    /// Creates a segment manager with a factory and loads existing segment files.
    ///
    /// This variant is used for recovery scenarios where segment files already
    /// exist on disk. It scans the given existing segment IDs, loads them, and
    /// sets the highest ID as the active segment.
    ///
    /// # Arguments
    ///
    /// * `backend_factory` - Function that creates a storage backend for a segment ID
    /// * `max_segment_size` - Maximum size before auto-sealing
    /// * `existing_segment_ids` - Sorted list of existing segment IDs to load
    pub fn with_factory_and_existing<F>(
        backend_factory: F,
        max_segment_size: u64,
        existing_segment_ids: Vec<u64>,
    ) -> Self
    where
        F: Fn(u64) -> Box<dyn StorageBackend> + Send + Sync + 'static,
    {
        if existing_segment_ids.is_empty() {
            // No existing segments, start fresh
            return Self::with_factory(backend_factory, max_segment_size);
        }

        let mut segments = HashMap::new();
        let mut segment_info = HashMap::new();
        let mut max_id = 0u64;

        // Load all existing segments
        for &segment_id in &existing_segment_ids {
            let backend = backend_factory(segment_id);
            let size = backend.size().unwrap_or(0);

            // All existing segments except the last one are considered sealed
            let is_last = segment_id == *existing_segment_ids.last().unwrap();

            segments.insert(segment_id, Arc::new(RwLock::new(backend)));
            segment_info.insert(
                segment_id,
                SegmentInfo {
                    id: segment_id,
                    sealed: !is_last,
                    size,
                    record_count: 0, // Will be updated during index rebuild
                },
            );

            max_id = max_id.max(segment_id);
        }

        Self {
            backend_factory: Box::new(backend_factory),
            segments: RwLock::new(segments),
            segment_info: RwLock::new(segment_info),
            active_segment_id: RwLock::new(max_id),
            max_segment_size,
            index: RwLock::new(HashMap::new()),
            on_segment_sealed: RwLock::new(None),
            compaction_lock: parking_lot::Mutex::new(()),
        }
    }

    /// Creates a segment manager with a single initial backend.
    ///
    /// This is a simpler constructor for use cases where you only need
    /// a single segment (e.g., testing) or where rotation creates
    /// new in-memory backends.
    ///
    /// For production use with file-based persistence, prefer `with_factory`
    /// to properly handle segment rotation with file paths.
    ///
    /// # Arguments
    ///
    /// * `backend` - Initial storage backend
    /// * `max_segment_size` - Maximum size before auto-sealing
    pub fn new(backend: Box<dyn StorageBackend>, max_segment_size: u64) -> Self {
        use entidb_storage::InMemoryBackend;

        let initial_id = 1;
        let initial_size = backend.size().unwrap_or(0);

        let mut segments = HashMap::new();
        segments.insert(initial_id, Arc::new(RwLock::new(backend)));

        let mut segment_info = HashMap::new();
        segment_info.insert(
            initial_id,
            SegmentInfo {
                id: initial_id,
                sealed: false,
                size: initial_size,
                record_count: 0,
            },
        );

        // When using this constructor, new segments get in-memory backends
        // (suitable for testing; production should use with_factory)
        Self {
            backend_factory: Box::new(|_| Box::new(InMemoryBackend::new())),
            segments: RwLock::new(segments),
            segment_info: RwLock::new(segment_info),
            active_segment_id: RwLock::new(initial_id),
            max_segment_size,
            index: RwLock::new(HashMap::new()),
            on_segment_sealed: RwLock::new(None),
            compaction_lock: parking_lot::Mutex::new(()),
        }
    }

    /// Creates a simple segment manager for testing (single in-memory backend).
    #[cfg(test)]
    pub fn new_in_memory(max_segment_size: u64) -> Self {
        use entidb_storage::InMemoryBackend;
        Self::with_factory(|_| Box::new(InMemoryBackend::new()), max_segment_size)
    }

    /// Sets a callback to be invoked when a segment is sealed.
    pub fn on_segment_sealed<F>(&self, callback: F)
    where
        F: Fn(u64) + Send + Sync + 'static,
    {
        *self.on_segment_sealed.write() = Some(Box::new(callback));
    }

    /// Returns the current active segment ID.
    pub fn active_segment_id(&self) -> u64 {
        *self.active_segment_id.read()
    }

    /// Lists all segments with their info.
    pub fn list_segments(&self) -> Vec<SegmentInfo> {
        let info = self.segment_info.read();
        let mut segments: Vec<_> = info.values().cloned().collect();
        segments.sort_by_key(|s| s.id);
        segments
    }

    /// Gets info for a specific segment.
    pub fn segment_info(&self, segment_id: u64) -> Option<SegmentInfo> {
        self.segment_info.read().get(&segment_id).cloned()
    }

    /// Checks if the active segment should be sealed.
    fn should_seal(&self, additional_bytes: usize) -> bool {
        let active_id = *self.active_segment_id.read();
        if let Some(info) = self.segment_info.read().get(&active_id) {
            return !info.sealed && info.size + additional_bytes as u64 > self.max_segment_size;
        }
        false
    }

    /// Seals the current active segment and creates a new one.
    ///
    /// This method coordinates with compaction by acquiring the compaction lock
    /// before sealing. This ensures that:
    /// - No segment is sealed while compaction is scanning sealed segments
    /// - Compaction sees a consistent view of sealed segments
    pub fn seal_and_rotate(&self) -> CoreResult<u64> {
        // Acquire compaction lock to prevent sealing during compaction
        let _compaction_guard = self.compaction_lock.lock();

        let old_id = *self.active_segment_id.read();

        // Seal the current segment
        {
            let mut info = self.segment_info.write();
            if let Some(seg_info) = info.get_mut(&old_id) {
                seg_info.sealed = true;
            }
        }

        // Flush the sealed segment
        if let Some(backend) = self.segments.read().get(&old_id) {
            backend.write().flush()?;
        }

        // Create new segment
        let new_id = old_id + 1;
        let new_backend = (self.backend_factory)(new_id);

        {
            let mut segments = self.segments.write();
            segments.insert(new_id, Arc::new(RwLock::new(new_backend)));
        }

        {
            let mut info = self.segment_info.write();
            info.insert(
                new_id,
                SegmentInfo {
                    id: new_id,
                    sealed: false,
                    size: 0,
                    record_count: 0,
                },
            );
        }

        *self.active_segment_id.write() = new_id;

        // Invoke callback
        if let Some(callback) = self.on_segment_sealed.read().as_ref() {
            callback(old_id);
        }

        Ok(new_id)
    }

    /// Appends a record to the current segment, auto-rotating if needed.
    ///
    /// Returns the (segment_id, offset) where the record was written.
    pub fn append(&self, record: &SegmentRecord) -> CoreResult<(u64, u64)> {
        let encoded = record.encode();
        let encoded_len = encoded.len();

        // Check if we need to seal and rotate
        if self.should_seal(encoded_len) {
            self.seal_and_rotate()?;
        }

        let segment_id = *self.active_segment_id.read();

        // Get the backend Arc while holding the segments lock, then release it
        let backend = {
            let segments = self.segments.read();
            segments
                .get(&segment_id)
                .ok_or_else(|| CoreError::segment_corruption("active segment not found"))?
                .clone()
        };

        let offset = backend.write().append(&encoded)?;

        // Update segment info
        {
            let mut info = self.segment_info.write();
            if let Some(seg_info) = info.get_mut(&segment_id) {
                seg_info.size += encoded_len as u64;
                seg_info.record_count += 1;
            }
        }

        // Update MVCC index - add new version to the chain
        let key = (record.collection_id.as_u32(), record.entity_id);
        let entry = IndexEntry {
            segment_id,
            offset,
            sequence: record.sequence,
        };
        self.index
            .write()
            .entry(key)
            .or_insert_with(VersionChain::new)
            .add(entry);

        Ok((segment_id, offset))
    }

    /// Legacy append method for compatibility (returns just offset).
    pub fn append_legacy(&self, record: &SegmentRecord) -> CoreResult<u64> {
        let (_seg_id, offset) = self.append(record)?;
        Ok(offset)
    }

    /// Gets an entity by collection and entity ID (latest version).
    ///
    /// Returns `None` if the entity doesn't exist or is deleted.
    /// For snapshot isolation, use `get_at_snapshot` instead.
    pub fn get(
        &self,
        collection_id: CollectionId,
        entity_id: &[u8; 16],
    ) -> CoreResult<Option<Vec<u8>>> {
        self.get_at_snapshot(collection_id, entity_id, None)
    }

    /// Gets an entity at a specific snapshot sequence.
    ///
    /// If `max_sequence` is `Some`, returns the newest version where
    /// `version.sequence <= max_sequence`. This provides snapshot isolation.
    ///
    /// If `max_sequence` is `None`, returns the latest version.
    ///
    /// Returns `None` if:
    /// - The entity doesn't exist
    /// - The entity was deleted (tombstone)
    /// - No version is visible at the given snapshot
    pub fn get_at_snapshot(
        &self,
        collection_id: CollectionId,
        entity_id: &[u8; 16],
        max_sequence: Option<SequenceNumber>,
    ) -> CoreResult<Option<Vec<u8>>> {
        let key = (collection_id.as_u32(), *entity_id);
        let index = self.index.read();

        let Some(chain) = index.get(&key) else {
            return Ok(None);
        };

        // Get the appropriate version based on snapshot
        let entry = match max_sequence {
            Some(seq) => chain.get_at(seq),
            None => chain.latest(),
        };

        let Some(entry) = entry else {
            return Ok(None);
        };

        // Read the record from the appropriate segment
        let record = self.read_at(entry.segment_id, entry.offset)?;

        if record.is_tombstone() {
            return Ok(None);
        }

        Ok(Some(record.payload))
    }

    /// Reads a record at a specific offset in a segment.
    pub fn read_at(&self, segment_id: u64, offset: u64) -> CoreResult<SegmentRecord> {
        let segments = self.segments.read();
        let backend = segments
            .get(&segment_id)
            .ok_or_else(|| CoreError::segment_corruption("segment not found"))?;

        let backend = backend.read();

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

    /// Scans all records across all segments.
    ///
    /// **Warning:** This includes the active segment. For compaction, use
    /// [`scan_sealed`] instead to avoid scanning data that may be concurrently
    /// written.
    pub fn scan_all(&self) -> CoreResult<Vec<SegmentRecord>> {
        let segments = self.segments.read();
        let _segment_info = self.segment_info.read();

        let mut all_records = Vec::new();
        let mut segment_ids: Vec<_> = segments.keys().copied().collect();
        segment_ids.sort();

        for seg_id in segment_ids {
            let records = self.scan_segment(seg_id)?;
            all_records.extend(records);
        }

        Ok(all_records)
    }

    /// Scans all records across sealed (immutable) segments only.
    ///
    /// This method is safe for compaction as it excludes the active segment,
    /// preventing races with concurrent writes. The active segment continues
    /// to receive new writes while sealed segments are being compacted.
    ///
    /// **Note:** For compaction, prefer using [`compact_sealed`] which performs
    /// the scan and replacement atomically while holding the compaction lock.
    ///
    /// # Returns
    ///
    /// A vector of all segment records from sealed segments, ordered by segment ID.
    pub fn scan_sealed(&self) -> CoreResult<Vec<SegmentRecord>> {
        let active_id = *self.active_segment_id.read();
        let segment_info = self.segment_info.read();

        // Collect sealed segment IDs only
        let mut sealed_ids: Vec<u64> = segment_info
            .iter()
            .filter(|(&id, info)| info.sealed && id != active_id)
            .map(|(&id, _)| id)
            .collect();
        sealed_ids.sort();

        drop(segment_info); // Release lock before scanning

        let mut all_records = Vec::new();
        for seg_id in sealed_ids {
            let records = self.scan_segment(seg_id)?;
            all_records.extend(records);
        }

        Ok(all_records)
    }

    /// Performs atomic compaction of sealed segments.
    ///
    /// This is the preferred method for compaction as it:
    /// 1. Acquires the compaction lock to prevent segment sealing during the operation
    /// 2. Scans all sealed segments
    /// 3. Applies the provided compaction function
    /// 4. Replaces sealed segments with compacted data
    ///
    /// The compaction lock ensures that no new segments are sealed while the
    /// operation is in progress, providing a consistent view of sealed segments.
    ///
    /// # Arguments
    ///
    /// * `compact_fn` - A function that takes the scanned records and returns
    ///   the compacted records to write
    ///
    /// # Returns
    ///
    /// A tuple of (removed_segment_ids, new_segment_id) on success.
    ///
    /// # Concurrency
    ///
    /// - Writers can continue appending to the active segment
    /// - Segment sealing is blocked until compaction completes
    /// - Reads are not blocked (MVCC index provides consistent view)
    pub fn compact_sealed<F>(
        &self,
        compact_fn: F,
    ) -> CoreResult<(Vec<SegmentRecord>, Vec<u64>, Option<u64>)>
    where
        F: FnOnce(Vec<SegmentRecord>) -> CoreResult<Vec<SegmentRecord>>,
    {
        // Acquire compaction lock for the entire operation
        // This prevents seal_and_rotate from completing while we're compacting
        let _compaction_guard = self.compaction_lock.lock();

        // Scan sealed segments while holding the lock
        let records = self.scan_sealed_internal()?;

        if records.is_empty() {
            return Ok((vec![], vec![], None));
        }

        // Apply compaction logic
        let compacted_records = compact_fn(records)?;

        // Replace sealed segments with compacted data
        let (removed_ids, new_segment_id) =
            self.replace_sealed_with_compacted_internal(compacted_records.clone())?;

        Ok((compacted_records, removed_ids, new_segment_id))
    }

    /// Internal scan that doesn't acquire the compaction lock (caller must hold it).
    fn scan_sealed_internal(&self) -> CoreResult<Vec<SegmentRecord>> {
        let active_id = *self.active_segment_id.read();
        let segment_info = self.segment_info.read();

        // Collect sealed segment IDs only
        let mut sealed_ids: Vec<u64> = segment_info
            .iter()
            .filter(|(&id, info)| info.sealed && id != active_id)
            .map(|(&id, _)| id)
            .collect();
        sealed_ids.sort();

        drop(segment_info); // Release lock before scanning

        let mut all_records = Vec::new();
        for seg_id in sealed_ids {
            let records = self.scan_segment(seg_id)?;
            all_records.extend(records);
        }

        Ok(all_records)
    }

    /// Internal replacement that doesn't acquire the compaction lock (caller must hold it).
    fn replace_sealed_with_compacted_internal(
        &self,
        compacted_records: Vec<SegmentRecord>,
    ) -> CoreResult<(Vec<u64>, Option<u64>)> {
        // Get list of sealed segments to remove
        let sealed_ids: Vec<u64> = {
            let info = self.segment_info.read();
            info.iter()
                .filter(|(_, seg)| seg.sealed)
                .map(|(&id, _)| id)
                .collect()
        };

        if sealed_ids.is_empty() && compacted_records.is_empty() {
            return Ok((vec![], None));
        }

        // Find the next segment ID to use
        let new_segment_id = {
            let info = self.segment_info.read();
            info.keys().copied().max().unwrap_or(0) + 1
        };

        // Create new segment for compacted data (if any records)
        let created_segment_id = if !compacted_records.is_empty() {
            let new_backend = (self.backend_factory)(new_segment_id);

            // Write all compacted records
            let mut total_size = 0u64;
            {
                let backend = new_backend;
                let backend_guard = Arc::new(RwLock::new(backend));

                for record in &compacted_records {
                    let encoded = record.encode();
                    total_size += encoded.len() as u64;
                    backend_guard.write().append(&encoded)?;
                }

                // Sync to disk
                backend_guard.write().sync()?;

                // Add to segments map
                self.segments.write().insert(new_segment_id, backend_guard);
            }

            // Add segment info (sealed since it contains only historical data)
            self.segment_info.write().insert(
                new_segment_id,
                SegmentInfo {
                    id: new_segment_id,
                    sealed: true,
                    size: total_size,
                    record_count: compacted_records.len(),
                },
            );

            Some(new_segment_id)
        } else {
            None
        };

        // Remove old sealed segments from memory
        {
            let mut segments = self.segments.write();
            let mut info = self.segment_info.write();

            for seg_id in &sealed_ids {
                segments.remove(seg_id);
                info.remove(seg_id);
            }
        }

        // Rebuild the index from remaining segments
        self.rebuild_index()?;

        Ok((sealed_ids, created_segment_id))
    }

    /// Scans all records in a specific segment.
    pub fn scan_segment(&self, segment_id: u64) -> CoreResult<Vec<SegmentRecord>> {
        let segments = self.segments.read();
        let backend = segments
            .get(&segment_id)
            .ok_or_else(|| CoreError::segment_corruption("segment not found"))?;

        let backend = backend.read();
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

    /// Rebuilds the in-memory MVCC index from all segment data.
    ///
    /// This builds version chains for each entity, enabling snapshot isolation.
    pub fn rebuild_index(&self) -> CoreResult<()> {
        let segments = self.segments.read();
        let mut segment_ids: Vec<_> = segments.keys().copied().collect();
        segment_ids.sort();

        let mut new_index: HashMap<(u32, [u8; 16]), VersionChain> = HashMap::new();

        for seg_id in segment_ids {
            let backend = segments
                .get(&seg_id)
                .ok_or_else(|| CoreError::segment_corruption("segment not found"))?;

            let backend = backend.read();
            let size = backend.size()?;
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

                // Add this version to the chain
                let entry = IndexEntry {
                    segment_id: seg_id,
                    offset,
                    sequence: record.sequence,
                };
                new_index
                    .entry(key)
                    .or_insert_with(VersionChain::new)
                    .add(entry);

                offset += record_len as u64;
            }
        }

        *self.index.write() = new_index;
        Ok(())
    }

    /// Flushes all segment writes.
    pub fn flush(&self) -> CoreResult<()> {
        let segments = self.segments.read();
        for backend in segments.values() {
            backend.write().flush()?;
        }
        Ok(())
    }

    /// Syncs all segments to durable storage.
    ///
    /// This calls `sync_all()` on all segment backends, ensuring data is
    /// persisted to disk. This is more expensive than `flush()` but provides
    /// crash safety guarantees.
    pub fn sync(&self) -> CoreResult<()> {
        let segments = self.segments.read();
        for backend in segments.values() {
            backend.write().sync()?;
        }
        Ok(())
    }

    /// Returns the total size across all segments.
    pub fn total_size(&self) -> CoreResult<u64> {
        let info = self.segment_info.read();
        Ok(info.values().map(|i| i.size).sum())
    }

    /// Returns the size of the active segment.
    pub fn size(&self) -> CoreResult<u64> {
        let active_id = *self.active_segment_id.read();
        let segments = self.segments.read();
        if let Some(backend) = segments.get(&active_id) {
            return Ok(backend.read().size()?);
        }
        Ok(0)
    }

    /// Returns the number of indexed entities.
    pub fn entity_count(&self) -> usize {
        self.index.read().len()
    }

    /// Checks if an entity exists (including tombstones in index).
    pub fn contains(&self, collection_id: CollectionId, entity_id: &[u8; 16]) -> bool {
        let key = (collection_id.as_u32(), *entity_id);
        let index = self.index.read();
        index
            .get(&key)
            .map(|chain| !chain.is_empty())
            .unwrap_or(false)
    }

    /// Iterates over all live entities in a collection (latest versions).
    ///
    /// For snapshot-isolated iteration, use `iter_collection_at_snapshot`.
    pub fn iter_collection(
        &self,
        collection_id: CollectionId,
    ) -> CoreResult<Vec<([u8; 16], Vec<u8>)>> {
        self.iter_collection_at_snapshot(collection_id, None)
    }

    /// Iterates over all live entities in a collection at a specific snapshot.
    ///
    /// If `max_sequence` is `Some`, returns versions visible at that snapshot.
    /// If `max_sequence` is `None`, returns latest versions.
    pub fn iter_collection_at_snapshot(
        &self,
        collection_id: CollectionId,
        max_sequence: Option<SequenceNumber>,
    ) -> CoreResult<Vec<([u8; 16], Vec<u8>)>> {
        let index = self.index.read();
        let mut results = Vec::new();

        for (&(col_id, entity_id), chain) in index.iter() {
            if col_id != collection_id.as_u32() {
                continue;
            }

            // Get the appropriate version based on snapshot
            let entry = match max_sequence {
                Some(seq) => chain.get_at(seq),
                None => chain.latest(),
            };

            let Some(entry) = entry else {
                continue;
            };

            let record = self.read_at(entry.segment_id, entry.offset)?;
            if !record.is_tombstone() {
                results.push((entity_id, record.payload));
            }
        }

        Ok(results)
    }

    /// Gets the number of sealed segments.
    pub fn sealed_segment_count(&self) -> usize {
        self.segment_info
            .read()
            .values()
            .filter(|i| i.sealed)
            .count()
    }

    /// Gets the total number of segments.
    pub fn segment_count(&self) -> usize {
        self.segment_info.read().len()
    }

    /// Replaces all sealed segments with compacted records.
    ///
    /// This is the core operation for compaction:
    /// 1. Acquires the compaction lock to prevent segment sealing
    /// 2. Creates a new segment for the compacted data
    /// 3. Writes all compacted records to it
    /// 4. Removes the old sealed segments
    /// 5. Rebuilds the index
    ///
    /// The active (unsealed) segment is preserved.
    ///
    /// **Note:** For atomic compaction (scan + replace), prefer using [`compact_sealed`]
    /// which holds the compaction lock throughout the entire operation.
    ///
    /// # Returns
    ///
    /// A tuple of (removed_segment_ids, new_segment_id) on success.
    pub fn replace_sealed_with_compacted(
        &self,
        compacted_records: Vec<SegmentRecord>,
    ) -> CoreResult<(Vec<u64>, Option<u64>)> {
        // Acquire compaction lock to coordinate with seal_and_rotate
        let _compaction_guard = self.compaction_lock.lock();
        self.replace_sealed_with_compacted_internal(compacted_records)
    }

    /// Gets the list of sealed segment IDs.
    ///
    /// This is useful for the database to delete the corresponding files
    /// after successful compaction.
    pub fn sealed_segment_ids(&self) -> Vec<u64> {
        self.segment_info
            .read()
            .iter()
            .filter(|(_, seg)| seg.sealed)
            .map(|(&id, _)| id)
            .collect()
    }
}

impl std::fmt::Debug for SegmentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentManager")
            .field("max_segment_size", &self.max_segment_size)
            .field("active_segment_id", &*self.active_segment_id.read())
            .field("segment_count", &self.segment_count())
            .field("entity_count", &self.entity_count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_manager() -> SegmentManager {
        SegmentManager::new_in_memory(1024 * 1024)
    }

    fn create_manager_small() -> SegmentManager {
        // Small segment size to test rotation
        SegmentManager::new_in_memory(200)
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

    #[test]
    fn auto_seal_and_rotate() {
        let manager = create_manager_small();

        // Initially one segment
        assert_eq!(manager.segment_count(), 1);
        assert_eq!(manager.sealed_segment_count(), 0);

        // Add enough records to trigger rotation
        // Each record is about 37+ bytes, so 200 bytes should trigger after ~5 records
        for i in 0..10u8 {
            let record = SegmentRecord::put(
                CollectionId::new(1),
                [i; 16],
                vec![i; 20], // larger payload
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        // Should have rotated to multiple segments
        assert!(manager.segment_count() > 1, "Expected multiple segments");
        assert!(
            manager.sealed_segment_count() > 0,
            "Expected at least one sealed segment"
        );
    }

    #[test]
    fn read_across_segments() {
        let manager = create_manager_small();

        // Add records that will span multiple segments
        for i in 0..10u8 {
            let record = SegmentRecord::put(
                CollectionId::new(1),
                [i; 16],
                vec![i; 20],
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        // All records should still be readable
        for i in 0..10u8 {
            let result = manager.get(CollectionId::new(1), &[i; 16]).unwrap();
            assert_eq!(result, Some(vec![i; 20]));
        }
    }

    #[test]
    fn list_segments() {
        let manager = create_manager_small();

        // Add records to cause rotation
        for i in 0..10u8 {
            let record = SegmentRecord::put(
                CollectionId::new(1),
                [i; 16],
                vec![i; 20],
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        let segments = manager.list_segments();
        assert!(!segments.is_empty());

        // All but the last should be sealed
        for (i, seg) in segments.iter().enumerate() {
            if i < segments.len() - 1 {
                assert!(seg.sealed, "Segment {} should be sealed", seg.id);
            } else {
                assert!(!seg.sealed, "Active segment should not be sealed");
            }
        }
    }

    #[test]
    fn seal_callback() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let manager = create_manager_small();
        let sealed_count = Arc::new(AtomicU64::new(0));
        let sealed_clone = Arc::clone(&sealed_count);

        manager.on_segment_sealed(move |_| {
            sealed_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Add records to cause rotation
        for i in 0..10u8 {
            let record = SegmentRecord::put(
                CollectionId::new(1),
                [i; 16],
                vec![i; 20],
                SequenceNumber::new(u64::from(i)),
            );
            manager.append(&record).unwrap();
        }

        // Callback should have been invoked
        assert!(
            sealed_count.load(Ordering::SeqCst) > 0,
            "Seal callback should have been invoked"
        );
    }

    #[test]
    fn manual_seal_and_rotate() {
        let manager = create_manager();

        assert_eq!(manager.active_segment_id(), 1);
        assert_eq!(manager.sealed_segment_count(), 0);

        let new_id = manager.seal_and_rotate().unwrap();
        assert_eq!(new_id, 2);
        assert_eq!(manager.active_segment_id(), 2);
        assert_eq!(manager.sealed_segment_count(), 1);
    }

    #[test]
    fn scan_sealed_excludes_active_segment() {
        let manager = create_manager();
        let collection = CollectionId::new(1);

        // Add some records to segment 1 (active)
        for i in 0..3u8 {
            let record = SegmentRecord::put(
                collection,
                [i; 16],
                vec![i; 10],
                SequenceNumber::new(u64::from(i) + 1),
            );
            manager.append(&record).unwrap();
        }

        // scan_sealed should return empty (no sealed segments yet)
        let sealed_records = manager.scan_sealed().unwrap();
        assert!(sealed_records.is_empty(), "No sealed segments should exist yet");

        // Seal segment 1 and rotate to segment 2
        manager.seal_and_rotate().unwrap();

        // Add more records to segment 2 (now active)
        for i in 3..6u8 {
            let record = SegmentRecord::put(
                collection,
                [i; 16],
                vec![i; 10],
                SequenceNumber::new(u64::from(i) + 1),
            );
            manager.append(&record).unwrap();
        }

        // scan_sealed should only return records from segment 1
        let sealed_records = manager.scan_sealed().unwrap();
        assert_eq!(sealed_records.len(), 3, "Should have 3 records from sealed segment");

        // Verify the records are from the first batch
        for record in &sealed_records {
            assert!(record.entity_id[0] < 3, "Record should be from first batch");
        }

        // scan_all should return all 6 records
        let all_records = manager.scan_all().unwrap();
        assert_eq!(all_records.len(), 6, "Should have all 6 records");
    }

    #[test]
    fn compact_sealed_is_atomic() {
        let manager = create_manager();
        let collection = CollectionId::new(1);

        // Add records and seal segment
        for i in 0..3u8 {
            let record = SegmentRecord::put(
                collection,
                [i; 16],
                vec![i; 10],
                SequenceNumber::new(u64::from(i) + 1),
            );
            manager.append(&record).unwrap();
        }
        manager.seal_and_rotate().unwrap();

        // Add duplicate records with higher sequence numbers
        for i in 0..2u8 {
            let record = SegmentRecord::put(
                collection,
                [i; 16],
                vec![i + 100; 10], // Updated payload
                SequenceNumber::new(u64::from(i) + 10),
            );
            manager.append(&record).unwrap();
        }
        manager.seal_and_rotate().unwrap();

        // Add records to active segment (should not be affected)
        let active_record = SegmentRecord::put(
            collection,
            [10u8; 16],
            vec![10; 10],
            SequenceNumber::new(100),
        );
        manager.append(&active_record).unwrap();

        // Perform atomic compaction - should only compact sealed segments
        let (compacted, removed_ids, new_seg_id) = manager
            .compact_sealed(|records| {
                // Simple deduplication: keep highest sequence per entity
                use std::collections::HashMap;
                let mut latest: HashMap<[u8; 16], SegmentRecord> = HashMap::new();
                for record in records {
                    latest
                        .entry(record.entity_id)
                        .and_modify(|e| {
                            if record.sequence > e.sequence {
                                *e = record.clone();
                            }
                        })
                        .or_insert(record);
                }
                Ok(latest.into_values().collect())
            })
            .unwrap();

        // Should have removed 2 sealed segments
        assert_eq!(removed_ids.len(), 2);

        // Should have created 1 new compacted segment
        assert!(new_seg_id.is_some());

        // Compacted result should have 3 unique entities
        assert_eq!(compacted.len(), 3);

        // Active segment record should still be readable
        let active_data = manager.get(collection, &[10u8; 16]).unwrap();
        assert!(active_data.is_some());
        assert_eq!(active_data.unwrap(), vec![10; 10]);
    }

    #[test]
    fn replace_sealed_preserves_active_segment() {
        let manager = create_manager();
        let collection = CollectionId::new(1);

        // Add and seal a segment
        let sealed_record = SegmentRecord::put(
            collection,
            [1u8; 16],
            vec![1; 10],
            SequenceNumber::new(1),
        );
        manager.append(&sealed_record).unwrap();
        manager.seal_and_rotate().unwrap();

        // Add to active segment
        let active_record = SegmentRecord::put(
            collection,
            [2u8; 16],
            vec![2; 10],
            SequenceNumber::new(2),
        );
        manager.append(&active_record).unwrap();

        // Replace sealed segments with empty compacted data
        let (removed, _new_id) = manager.replace_sealed_with_compacted(vec![]).unwrap();

        // Should have removed the sealed segment
        assert_eq!(removed.len(), 1);

        // Active segment data should still be accessible
        let data = manager.get(collection, &[2u8; 16]).unwrap();
        assert!(data.is_some());
        assert_eq!(data.unwrap(), vec![2; 10]);

        // Sealed segment data should be gone
        let old_data = manager.get(collection, &[1u8; 16]).unwrap();
        assert!(old_data.is_none());
    }
}
