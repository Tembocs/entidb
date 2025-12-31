//! Index Engine - automatic, transactional index management.
//!
//! The IndexEngine is the internal component that manages all indexes transparently.
//! Users define indexes on collections (via field extractors), and the engine:
//!
//! 1. Automatically maintains indexes during commits
//! 2. Persists index definitions to the manifest
//! 3. Rebuilds indexes on database open
//! 4. Provides transparent access path selection
//!
//! # Invariants
//!
//! - Users MUST NOT reference indexes by name during queries
//! - Index updates MUST be atomic with transaction commit
//! - Index state MUST be derivable from segments + WAL
//! - Index corruption MUST NOT corrupt entity data

use crate::entity::EntityId;
use crate::error::{CoreError, CoreResult};
use crate::index::{BTreeIndex, HashIndex, Index, IndexSpec};
use crate::segment::SegmentRecord;
use crate::types::{CollectionId, SequenceNumber};
use entidb_codec::Value;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Type of index for manifest persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum IndexKind {
    /// Hash index for O(1) equality lookups.
    Hash = 0,
    /// BTree index for ordered traversal and range queries.
    BTree = 1,
}

impl TryFrom<u8> for IndexKind {
    type Error = CoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(IndexKind::Hash),
            1 => Ok(IndexKind::BTree),
            _ => Err(CoreError::invalid_format(format!(
                "unknown index kind: {}",
                value
            ))),
        }
    }
}

/// Persisted index definition stored in manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDefinition {
    /// Auto-generated unique ID for this index.
    pub id: u64,
    /// Collection this index belongs to.
    pub collection_id: CollectionId,
    /// Field path for key extraction (CBOR path, e.g., ["address", "city"]).
    pub field_path: Vec<String>,
    /// Type of index (Hash or BTree).
    pub kind: IndexKind,
    /// Whether the index enforces uniqueness.
    pub unique: bool,
    /// Sequence number when index was created.
    pub created_at_seq: SequenceNumber,
}

impl IndexDefinition {
    /// Creates a deterministic index name from its definition.
    #[must_use]
    pub fn canonical_name(&self) -> String {
        format!(
            "__idx_{}_{}_{:?}",
            self.collection_id.as_u32(),
            self.field_path.join("."),
            self.kind
        )
    }
}

/// A pending index update to be applied atomically on commit.
///
/// Reserved for future transactional index integration.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum IndexUpdate {
    /// Insert a key-entity mapping.
    Insert {
        index_id: u64,
        key: Vec<u8>,
        entity_id: EntityId,
    },
    /// Remove a key-entity mapping.
    Remove {
        index_id: u64,
        key: Vec<u8>,
        entity_id: EntityId,
    },
}

/// Statistics about index usage for telemetry.
///
/// Reserved for future telemetry integration.
#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct IndexStats {
    /// Number of index lookups performed.
    pub lookups: u64,
    /// Number of full scans performed (could have used index).
    pub scans: u64,
    /// Number of index updates.
    pub updates: u64,
}

/// Configuration for the index engine.
///
/// Reserved for future strict mode and telemetry integration.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IndexEngineConfig {
    /// Emit warning when full scan exceeds this threshold.
    pub scan_warning_threshold: usize,
    /// In strict mode, error on unindexed queries (for testing).
    pub forbid_full_scans: bool,
}

impl Default for IndexEngineConfig {
    fn default() -> Self {
        Self {
            scan_warning_threshold: 1000,
            forbid_full_scans: false,
        }
    }
}

/// The internal index engine managing all indexes.
///
/// This is invisible to users - they interact with collections and
/// the engine handles index maintenance automatically.
#[allow(dead_code)]
pub struct IndexEngine {
    /// Configuration.
    config: IndexEngineConfig,
    /// Index definitions keyed by ID.
    definitions: RwLock<HashMap<u64, IndexDefinition>>,
    /// Hash indexes keyed by index ID.
    hash_indexes: RwLock<HashMap<u64, HashIndex<Vec<u8>>>>,
    /// BTree indexes keyed by index ID.
    btree_indexes: RwLock<HashMap<u64, BTreeIndex<Vec<u8>>>>,
    /// Collection+field -> index ID lookup for access path selection.
    field_index_map: RwLock<HashMap<(CollectionId, Vec<String>), u64>>,
    /// Next index ID to assign.
    next_index_id: AtomicU64,
    /// Statistics.
    stats: RwLock<IndexStats>,
}

// Allow dead_code for methods reserved for future transactional integration.
// These will be used when full transactional index maintenance is wired.
#[allow(dead_code)]
impl IndexEngine {
    /// Creates a new empty index engine.
    #[must_use]
    pub fn new(config: IndexEngineConfig) -> Self {
        Self {
            config,
            definitions: RwLock::new(HashMap::new()),
            hash_indexes: RwLock::new(HashMap::new()),
            btree_indexes: RwLock::new(HashMap::new()),
            field_index_map: RwLock::new(HashMap::new()),
            next_index_id: AtomicU64::new(1),
            stats: RwLock::new(IndexStats::default()),
        }
    }

    /// Creates an index engine from persisted definitions.
    pub fn from_definitions(
        config: IndexEngineConfig,
        definitions: Vec<IndexDefinition>,
    ) -> Self {
        let mut defs_map = HashMap::new();
        let mut field_map = HashMap::new();
        let mut hash_indexes = HashMap::new();
        let mut btree_indexes = HashMap::new();
        let mut max_id = 0u64;

        for def in definitions {
            max_id = max_id.max(def.id);
            let key = (def.collection_id, def.field_path.clone());
            field_map.insert(key, def.id);

            // Create empty indexes (will be rebuilt from segments)
            let spec = IndexSpec::new(def.collection_id, def.canonical_name());
            let spec = if def.unique { spec.unique() } else { spec };

            match def.kind {
                IndexKind::Hash => {
                    hash_indexes.insert(def.id, HashIndex::new(spec));
                }
                IndexKind::BTree => {
                    btree_indexes.insert(def.id, BTreeIndex::new(spec));
                }
            }

            defs_map.insert(def.id, def);
        }

        Self {
            config,
            definitions: RwLock::new(defs_map),
            hash_indexes: RwLock::new(hash_indexes),
            btree_indexes: RwLock::new(btree_indexes),
            field_index_map: RwLock::new(field_map),
            next_index_id: AtomicU64::new(max_id + 1),
            stats: RwLock::new(IndexStats::default()),
        }
    }

    /// Returns all index definitions for persistence.
    #[must_use]
    pub fn definitions(&self) -> Vec<IndexDefinition> {
        self.definitions.read().values().cloned().collect()
    }

    /// Registers an index from a persisted definition (used during recovery).
    pub fn register_index(&self, def: IndexDefinition) {
        let key = (def.collection_id, def.field_path.clone());
        
        // Skip if already exists
        {
            let field_map = self.field_index_map.read();
            if field_map.contains_key(&key) {
                return;
            }
        }

        let spec = IndexSpec::new(def.collection_id, def.canonical_name());
        let spec = if def.unique { spec.unique() } else { spec };

        // Insert index
        match def.kind {
            IndexKind::Hash => {
                self.hash_indexes.write().insert(def.id, HashIndex::new(spec));
            }
            IndexKind::BTree => {
                self.btree_indexes.write().insert(def.id, BTreeIndex::new(spec));
            }
        }

        // Update next_index_id if needed
        let next_id = self.next_index_id.load(Ordering::SeqCst);
        if def.id >= next_id {
            self.next_index_id.store(def.id + 1, Ordering::SeqCst);
        }

        self.field_index_map.write().insert(key, def.id);
        self.definitions.write().insert(def.id, def);
    }

    /// Creates a new index definition.
    ///
    /// Returns the index ID if successful.
    pub fn create_index(
        &self,
        collection_id: CollectionId,
        field_path: Vec<String>,
        kind: IndexKind,
        unique: bool,
        current_seq: SequenceNumber,
    ) -> CoreResult<u64> {
        let key = (collection_id, field_path.clone());

        // Check if already exists
        {
            let field_map = self.field_index_map.read();
            if let Some(&_existing_id) = field_map.get(&key) {
                return Err(CoreError::invalid_operation(format!(
                    "index on {:?} already exists",
                    field_path
                )));
            }
        }

        // Create new index
        let id = self.next_index_id.fetch_add(1, Ordering::SeqCst);
        let def = IndexDefinition {
            id,
            collection_id,
            field_path: field_path.clone(),
            kind,
            unique,
            created_at_seq: current_seq,
        };

        let spec = IndexSpec::new(collection_id, def.canonical_name());
        let spec = if unique { spec.unique() } else { spec };

        // Insert index
        match kind {
            IndexKind::Hash => {
                self.hash_indexes.write().insert(id, HashIndex::new(spec));
            }
            IndexKind::BTree => {
                self.btree_indexes.write().insert(id, BTreeIndex::new(spec));
            }
        }

        self.field_index_map.write().insert(key, id);
        self.definitions.write().insert(id, def);

        Ok(id)
    }

    /// Drops an index by field path.
    pub fn drop_index(
        &self,
        collection_id: CollectionId,
        field_path: &[String],
    ) -> CoreResult<bool> {
        let key = (collection_id, field_path.to_vec());

        let id = {
            let mut field_map = self.field_index_map.write();
            match field_map.remove(&key) {
                Some(id) => id,
                None => return Ok(false),
            }
        };

        self.definitions.write().remove(&id);
        self.hash_indexes.write().remove(&id);
        self.btree_indexes.write().remove(&id);

        Ok(true)
    }

    /// Gets the index ID for a field path, if an index exists.
    #[must_use]
    pub fn get_index_for_field(
        &self,
        collection_id: CollectionId,
        field_path: &[String],
    ) -> Option<u64> {
        let key = (collection_id, field_path.to_vec());
        self.field_index_map.read().get(&key).copied()
    }

    /// Applies a batch of index updates atomically.
    ///
    /// This is called by the transaction manager during commit.
    pub fn apply_updates(&self, updates: &[IndexUpdate]) -> CoreResult<()> {
        let mut hash_indexes = self.hash_indexes.write();
        let mut btree_indexes = self.btree_indexes.write();
        let mut stats = self.stats.write();

        for update in updates {
            match update {
                IndexUpdate::Insert {
                    index_id,
                    key,
                    entity_id,
                } => {
                    if let Some(index) = hash_indexes.get_mut(index_id) {
                        index.insert(key.clone(), *entity_id)?;
                    } else if let Some(index) = btree_indexes.get_mut(index_id) {
                        index.insert(key.clone(), *entity_id)?;
                    }
                    stats.updates += 1;
                }
                IndexUpdate::Remove {
                    index_id,
                    key,
                    entity_id,
                } => {
                    if let Some(index) = hash_indexes.get_mut(index_id) {
                        index.remove(key, *entity_id)?;
                    } else if let Some(index) = btree_indexes.get_mut(index_id) {
                        index.remove(key, *entity_id)?;
                    }
                    stats.updates += 1;
                }
            }
        }

        Ok(())
    }

    /// Performs equality lookup on an index.
    pub fn lookup_eq(
        &self,
        collection_id: CollectionId,
        field_path: &[String],
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        let key_lookup = (collection_id, field_path.to_vec());

        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                // No index - return error if strict mode
                if self.config.forbid_full_scans {
                    return Err(CoreError::invalid_operation(format!(
                        "no index on field {:?} and full scans are forbidden",
                        field_path
                    )));
                }
                self.stats.write().scans += 1;
                return Err(CoreError::invalid_operation(format!(
                    "no index on field {:?}; use scan() for full collection access",
                    field_path
                )));
            }
        };

        self.stats.write().lookups += 1;

        // Try hash index first
        {
            let hash_indexes = self.hash_indexes.read();
            if let Some(index) = hash_indexes.get(&index_id) {
                return index.lookup(&key.to_vec());
            }
        }

        // Then btree
        {
            let btree_indexes = self.btree_indexes.read();
            if let Some(index) = btree_indexes.get(&index_id) {
                return index.lookup(&key.to_vec());
            }
        }

        Err(CoreError::invalid_format("index not found"))
    }

    /// Performs range lookup on a BTree index.
    pub fn lookup_range(
        &self,
        collection_id: CollectionId,
        field_path: &[String],
        min_key: Option<&[u8]>,
        max_key: Option<&[u8]>,
    ) -> CoreResult<Vec<EntityId>> {
        let key_lookup = (collection_id, field_path.to_vec());

        let index_id = match self.field_index_map.read().get(&key_lookup).copied() {
            Some(id) => id,
            None => {
                if self.config.forbid_full_scans {
                    return Err(CoreError::invalid_operation(format!(
                        "no index on field {:?} and full scans are forbidden",
                        field_path
                    )));
                }
                return Err(CoreError::invalid_operation(format!(
                    "no BTree index on field {:?}; use scan() for full collection access",
                    field_path
                )));
            }
        };

        self.stats.write().lookups += 1;

        let btree_indexes = self.btree_indexes.read();
        let index = btree_indexes.get(&index_id).ok_or_else(|| {
            CoreError::invalid_operation("index is not a BTree index")
        })?;

        use std::ops::Bound;
        let start = match min_key {
            Some(k) => Bound::Included(k.to_vec()),
            None => Bound::Unbounded,
        };
        let end = match max_key {
            Some(k) => Bound::Included(k.to_vec()),
            None => Bound::Unbounded,
        };

        index.range((start, end))
    }

    /// Rebuilds all indexes from segment records.
    ///
    /// Called during database open to restore index state.
    pub fn rebuild_from_records<'a, I>(&self, records: I)
    where
        I: IntoIterator<Item = &'a SegmentRecord>,
    {
        // Group records by (collection_id, entity_id) and keep latest
        let mut latest_records: HashMap<(CollectionId, [u8; 16]), &SegmentRecord> = HashMap::new();

        for record in records {
            let key = (record.collection_id, record.entity_id);
            let entry = latest_records.entry(key);
            
            entry
                .and_modify(|existing| {
                    if record.sequence > existing.sequence {
                        *existing = record;
                    }
                })
                .or_insert(record);
        }

        // Clear existing index data
        {
            let mut hash_indexes = self.hash_indexes.write();
            for index in hash_indexes.values_mut() {
                index.clear();
            }
        }
        {
            let mut btree_indexes = self.btree_indexes.write();
            for index in btree_indexes.values_mut() {
                index.clear();
            }
        }

        // Rebuild from latest records
        let defs = self.definitions.read();
        
        for ((_coll_id, entity_id_bytes), record) in latest_records {
            if record.is_tombstone() {
                continue; // Skip deleted entities
            }

            let entity_id = EntityId::from(entity_id_bytes);

            // For each index on this collection
            for def in defs.values() {
                if def.collection_id != record.collection_id {
                    continue;
                }

                // Extract key from payload based on field_path
                if let Some(key) = self.extract_key_from_cbor(&record.payload, &def.field_path) {
                    match def.kind {
                        IndexKind::Hash => {
                            let mut indexes = self.hash_indexes.write();
                            if let Some(index) = indexes.get_mut(&def.id) {
                                let _ = index.insert(key, entity_id);
                            }
                        }
                        IndexKind::BTree => {
                            let mut indexes = self.btree_indexes.write();
                            if let Some(index) = indexes.get_mut(&def.id) {
                                let _ = index.insert(key, entity_id);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Extracts a key value from CBOR payload given a field path.
    ///
    /// Returns the key as bytes for indexing.
    fn extract_key_from_cbor(&self, payload: &[u8], field_path: &[String]) -> Option<Vec<u8>> {
        // Use entidb_codec to parse CBOR and extract field
        // For now, we use a simple approach: if field_path is empty, use entire payload
        // Otherwise, attempt CBOR navigation
        
        if field_path.is_empty() {
            return Some(payload.to_vec());
        }

        // Parse CBOR and navigate to field
        let value = entidb_codec::from_cbor(payload).ok()?;
        let mut current = &value;

        for field in field_path {
            current = match current {
                Value::Map(map) => {
                    map.iter()
                        .find(|(k, _)| matches!(k, Value::Text(s) if s == field))
                        .map(|(_, v)| v)?
                }
                _ => return None,
            };
        }

        // Serialize the extracted value back to CBOR bytes for indexing
        let key_bytes = entidb_codec::to_canonical_cbor(current).ok()?;
        Some(key_bytes)
    }

    /// Returns current statistics.
    #[must_use]
    pub fn stats(&self) -> IndexStats {
        self.stats.read().clone()
    }

    /// Clears all indexes (used during testing).
    pub fn clear(&self) {
        let mut hash_indexes = self.hash_indexes.write();
        for index in hash_indexes.values_mut() {
            index.clear();
        }
        let mut btree_indexes = self.btree_indexes.write();
        for index in btree_indexes.values_mut() {
            index.clear();
        }
    }

    // ========================================================================
    // Legacy compatibility methods (for gradual migration)
    // ========================================================================

    /// Creates a hash index with explicit name (legacy API, will be deprecated).
    #[doc(hidden)]
    pub fn create_hash_index_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        unique: bool,
    ) -> CoreResult<()> {
        // Use name as single-element field path for legacy compatibility
        let field_path = vec![name.to_string()];
        self.create_index(
            collection_id,
            field_path,
            IndexKind::Hash,
            unique,
            SequenceNumber::new(0),
        )?;
        Ok(())
    }

    /// Creates a btree index with explicit name (legacy API, will be deprecated).
    #[doc(hidden)]
    pub fn create_btree_index_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        unique: bool,
    ) -> CoreResult<()> {
        let field_path = vec![name.to_string()];
        self.create_index(
            collection_id,
            field_path,
            IndexKind::BTree,
            unique,
            SequenceNumber::new(0),
        )?;
        Ok(())
    }

    /// Inserts into hash index by name (legacy API).
    #[doc(hidden)]
    pub fn hash_index_insert_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        key: Vec<u8>,
        entity_id: EntityId,
    ) -> CoreResult<()> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "hash index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let mut hash_indexes = self.hash_indexes.write();
        let index = hash_indexes.get_mut(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        index.insert(key, entity_id)
    }

    /// Removes from hash index by name (legacy API).
    #[doc(hidden)]
    pub fn hash_index_remove_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        key: &[u8],
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "hash index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let mut hash_indexes = self.hash_indexes.write();
        let index = hash_indexes.get_mut(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        index.remove(&key.to_vec(), entity_id)
    }

    /// Looks up in hash index by name (legacy API).
    #[doc(hidden)]
    pub fn hash_index_lookup_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "hash index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let hash_indexes = self.hash_indexes.read();
        let index = hash_indexes.get(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        self.stats.write().lookups += 1;
        index.lookup(&key.to_vec())
    }

    /// Gets hash index length by name (legacy API).
    #[doc(hidden)]
    pub fn hash_index_len_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
    ) -> CoreResult<usize> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "hash index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let hash_indexes = self.hash_indexes.read();
        let index = hash_indexes.get(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        Ok(index.len())
    }

    /// Drops hash index by name (legacy API).
    #[doc(hidden)]
    pub fn drop_hash_index_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
    ) -> CoreResult<bool> {
        let field_path = vec![name.to_string()];
        self.drop_index(collection_id, &field_path)
    }

    /// Creates btree index insert by name (legacy API).
    #[doc(hidden)]
    pub fn btree_index_insert_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        key: Vec<u8>,
        entity_id: EntityId,
    ) -> CoreResult<()> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "btree index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let mut btree_indexes = self.btree_indexes.write();
        let index = btree_indexes.get_mut(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        index.insert(key, entity_id)
    }

    /// Removes from btree index by name (legacy API).
    #[doc(hidden)]
    pub fn btree_index_remove_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        key: &[u8],
        entity_id: EntityId,
    ) -> CoreResult<bool> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "btree index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let mut btree_indexes = self.btree_indexes.write();
        let index = btree_indexes.get_mut(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        index.remove(&key.to_vec(), entity_id)
    }

    /// Looks up in btree index by name (legacy API).
    #[doc(hidden)]
    pub fn btree_index_lookup_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        key: &[u8],
    ) -> CoreResult<Vec<EntityId>> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "btree index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let btree_indexes = self.btree_indexes.read();
        let index = btree_indexes.get(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        self.stats.write().lookups += 1;
        index.lookup(&key.to_vec())
    }

    /// Range query on btree index by name (legacy API).
    #[doc(hidden)]
    pub fn btree_index_range_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
        min_key: Option<&[u8]>,
        max_key: Option<&[u8]>,
    ) -> CoreResult<Vec<EntityId>> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "btree index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let btree_indexes = self.btree_indexes.read();
        let index = btree_indexes.get(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        self.stats.write().lookups += 1;

        use std::ops::Bound;
        let start = match min_key {
            Some(k) => Bound::Included(k.to_vec()),
            None => Bound::Unbounded,
        };
        let end = match max_key {
            Some(k) => Bound::Included(k.to_vec()),
            None => Bound::Unbounded,
        };

        index.range((start, end))
    }

    /// Gets btree index length by name (legacy API).
    #[doc(hidden)]
    pub fn btree_index_len_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
    ) -> CoreResult<usize> {
        let field_path = vec![name.to_string()];
        let key_lookup = (collection_id, field_path);
        
        let index_id = self.field_index_map.read().get(&key_lookup).copied();
        let index_id = match index_id {
            Some(id) => id,
            None => {
                return Err(CoreError::invalid_format(format!(
                    "btree index '{}' not found on collection {}",
                    name,
                    collection_id.as_u32()
                )));
            }
        };

        let btree_indexes = self.btree_indexes.read();
        let index = btree_indexes.get(&index_id).ok_or_else(|| {
            CoreError::invalid_format("index not found")
        })?;
        
        Ok(index.len())
    }

    /// Drops btree index by name (legacy API).
    #[doc(hidden)]
    pub fn drop_btree_index_legacy(
        &self,
        collection_id: CollectionId,
        name: &str,
    ) -> CoreResult<bool> {
        let field_path = vec![name.to_string()];
        self.drop_index(collection_id, &field_path)
    }
}

impl Default for IndexEngine {
    fn default() -> Self {
        Self::new(IndexEngineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_index() {
        let engine = IndexEngine::new(IndexEngineConfig::default());
        let coll = CollectionId::new(1);
        
        let id = engine.create_index(
            coll,
            vec!["email".to_string()],
            IndexKind::Hash,
            true,
            SequenceNumber::new(0),
        ).unwrap();
        
        assert!(id > 0);
        
        // Creating same index again should fail
        let result = engine.create_index(
            coll,
            vec!["email".to_string()],
            IndexKind::Hash,
            true,
            SequenceNumber::new(0),
        );
        
        assert!(result.is_err());
    }

    #[test]
    fn test_register_index_from_definition() {
        let engine = IndexEngine::new(IndexEngineConfig::default());
        let coll = CollectionId::new(1);
        
        let def = IndexDefinition {
            id: 42,
            collection_id: coll,
            field_path: vec!["email".to_string()],
            kind: IndexKind::Hash,
            unique: true,
            created_at_seq: SequenceNumber::new(0),
        };
        
        engine.register_index(def.clone());
        
        let defs = engine.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].id, 42);
    }

    #[test]
    fn test_legacy_hash_index() {
        let engine = IndexEngine::new(IndexEngineConfig::default());
        let coll = CollectionId::new(1);
        
        engine.create_hash_index_legacy(coll, "email", false).unwrap();
        
        let entity_id = EntityId::new();
        engine.hash_index_insert_legacy(coll, "email", vec![1, 2, 3], entity_id).unwrap();
        
        let results = engine.hash_index_lookup_legacy(coll, "email", &[1, 2, 3]).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], entity_id);
        
        assert_eq!(engine.hash_index_len_legacy(coll, "email").unwrap(), 1);
        
        engine.hash_index_remove_legacy(coll, "email", &[1, 2, 3], entity_id).unwrap();
        assert_eq!(engine.hash_index_len_legacy(coll, "email").unwrap(), 0);
    }

    #[test]
    fn test_legacy_btree_index() {
        let engine = IndexEngine::new(IndexEngineConfig::default());
        let coll = CollectionId::new(1);
        
        engine.create_btree_index_legacy(coll, "age", false).unwrap();
        
        let entity_id1 = EntityId::new();
        let entity_id2 = EntityId::new();
        
        engine.btree_index_insert_legacy(coll, "age", vec![0, 0, 0, 25], entity_id1).unwrap();
        engine.btree_index_insert_legacy(coll, "age", vec![0, 0, 0, 30], entity_id2).unwrap();
        
        let results = engine.btree_index_range_legacy(coll, "age", None, None).unwrap();
        assert_eq!(results.len(), 2);
        
        let results = engine.btree_index_range_legacy(
            coll, "age",
            Some(&[0, 0, 0, 26]),
            None
        ).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_index_definitions_persistence() {
        let engine = IndexEngine::new(IndexEngineConfig::default());
        let coll = CollectionId::new(1);
        
        engine.create_index(
            coll,
            vec!["email".to_string()],
            IndexKind::Hash,
            true,
            SequenceNumber::new(5),
        ).unwrap();
        
        engine.create_index(
            coll,
            vec!["age".to_string()],
            IndexKind::BTree,
            false,
            SequenceNumber::new(6),
        ).unwrap();
        
        let defs = engine.definitions();
        assert_eq!(defs.len(), 2);
        
        // Recreate engine from definitions
        let engine2 = IndexEngine::from_definitions(IndexEngineConfig::default(), defs);
        let defs2 = engine2.definitions();
        assert_eq!(defs2.len(), 2);
    }
}
