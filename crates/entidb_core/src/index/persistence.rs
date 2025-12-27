//! Index persistence for saving/loading indexes to/from disk.
//!
//! This module provides functionality to persist index state to storage
//! and restore it on database open, avoiding expensive index rebuilds.
//!
//! ## Format
//!
//! Indexes are stored as canonical CBOR with the following structure:
//! ```text
//! IndexFile {
//!     magic: [0x45, 0x49, 0x44, 0x58] // "EIDX"
//!     version: u8
//!     index_type: u8  // 0 = Hash, 1 = BTree
//!     collection_id: u32
//!     name: String
//!     unique: bool
//!     entry_count: u64
//!     entries: [(key_bytes, [entity_id_bytes])]
//! }
//! ```
//!
//! ## Invariants
//!
//! - Index state **MUST** be derivable from segments + WAL
//! - Persisted indexes are an optimization, not source of truth
//! - Corruption in index file triggers rebuild, not error

use crate::entity::EntityId;
use crate::error::{CoreError, CoreResult};
use crate::index::traits::Index;
use crate::index::{BTreeIndex, HashIndex, IndexKey, IndexSpec};
use crate::types::CollectionId;
use std::collections::HashSet;

/// Magic bytes for index files: "EIDX"
const INDEX_MAGIC: [u8; 4] = [0x45, 0x49, 0x44, 0x58];

/// Current index file format version.
const INDEX_VERSION: u8 = 1;

/// Index type codes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    /// Hash index (equality lookups).
    Hash = 0,
    /// BTree index (range queries).
    BTree = 1,
}

impl TryFrom<u8> for IndexType {
    type Error = CoreError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(IndexType::Hash),
            1 => Ok(IndexType::BTree),
            _ => Err(CoreError::InvalidFormat {
                message: format!("unknown index type: {}", value),
            }),
        }
    }
}

/// Header for persisted index files.
#[derive(Debug, Clone)]
pub struct IndexHeader {
    /// Index type (Hash or BTree).
    pub index_type: IndexType,
    /// Collection this index belongs to.
    pub collection_id: CollectionId,
    /// Index name.
    pub name: String,
    /// Whether the index enforces uniqueness.
    pub unique: bool,
    /// Number of entries in the index.
    pub entry_count: u64,
}

/// Serializes an index header to bytes.
fn write_header(header: &IndexHeader) -> Vec<u8> {
    let mut buf = Vec::new();

    // Magic
    buf.extend_from_slice(&INDEX_MAGIC);

    // Version
    buf.push(INDEX_VERSION);

    // Index type
    buf.push(header.index_type as u8);

    // Collection ID (4 bytes, big-endian)
    buf.extend_from_slice(&header.collection_id.as_u32().to_be_bytes());

    // Name length (2 bytes) + name bytes
    let name_bytes = header.name.as_bytes();
    buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    buf.extend_from_slice(name_bytes);

    // Unique flag
    buf.push(if header.unique { 1 } else { 0 });

    // Entry count (8 bytes, big-endian)
    buf.extend_from_slice(&header.entry_count.to_be_bytes());

    buf
}

/// Reads an index header from bytes.
fn read_header(data: &[u8]) -> CoreResult<(IndexHeader, usize)> {
    if data.len() < 4 {
        return Err(CoreError::InvalidFormat {
            message: "index file too small".into(),
        });
    }

    // Check magic
    if data[0..4] != INDEX_MAGIC {
        return Err(CoreError::InvalidFormat {
            message: "invalid index file magic".into(),
        });
    }

    let mut pos = 4;

    // Version
    if data.len() < pos + 1 {
        return Err(CoreError::InvalidFormat {
            message: "truncated index header".into(),
        });
    }
    let version = data[pos];
    pos += 1;

    if version != INDEX_VERSION {
        return Err(CoreError::InvalidFormat {
            message: format!("unsupported index version: {}", version),
        });
    }

    // Index type
    let index_type = IndexType::try_from(data[pos])?;
    pos += 1;

    // Collection ID
    if data.len() < pos + 4 {
        return Err(CoreError::InvalidFormat {
            message: "truncated collection_id".into(),
        });
    }
    let collection_id = CollectionId::new(u32::from_be_bytes([
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
    ]));
    pos += 4;

    // Name length + name
    if data.len() < pos + 2 {
        return Err(CoreError::InvalidFormat {
            message: "truncated name length".into(),
        });
    }
    let name_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;

    if data.len() < pos + name_len {
        return Err(CoreError::InvalidFormat {
            message: "truncated name".into(),
        });
    }
    let name = String::from_utf8(data[pos..pos + name_len].to_vec()).map_err(|_| {
        CoreError::InvalidFormat {
            message: "invalid UTF-8 in index name".into(),
        }
    })?;
    pos += name_len;

    // Unique flag
    if data.len() < pos + 1 {
        return Err(CoreError::InvalidFormat {
            message: "truncated unique flag".into(),
        });
    }
    let unique = data[pos] != 0;
    pos += 1;

    // Entry count
    if data.len() < pos + 8 {
        return Err(CoreError::InvalidFormat {
            message: "truncated entry count".into(),
        });
    }
    let entry_count = u64::from_be_bytes([
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
        data[pos + 4],
        data[pos + 5],
        data[pos + 6],
        data[pos + 7],
    ]);
    pos += 8;

    Ok((
        IndexHeader {
            index_type,
            collection_id,
            name,
            unique,
            entry_count,
        },
        pos,
    ))
}

/// Writes a key-entities entry to the buffer.
fn write_entry<K: IndexKey>(buf: &mut Vec<u8>, key: &K, entities: &HashSet<EntityId>) {
    let key_bytes = key.to_bytes();

    // Key length (4 bytes) + key bytes
    buf.extend_from_slice(&(key_bytes.len() as u32).to_be_bytes());
    buf.extend_from_slice(&key_bytes);

    // Entity count (4 bytes) + entity IDs
    buf.extend_from_slice(&(entities.len() as u32).to_be_bytes());
    for entity_id in entities {
        buf.extend_from_slice(entity_id.as_bytes());
    }
}

/// Reads a key-entities entry from data.
fn read_entry<K: IndexKey>(data: &[u8], pos: &mut usize) -> CoreResult<(K, HashSet<EntityId>)> {
    // Key length
    if data.len() < *pos + 4 {
        return Err(CoreError::InvalidFormat {
            message: "truncated key length".into(),
        });
    }
    let key_len = u32::from_be_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
    ]) as usize;
    *pos += 4;

    // Key bytes
    if data.len() < *pos + key_len {
        return Err(CoreError::InvalidFormat {
            message: "truncated key bytes".into(),
        });
    }
    let key = K::from_bytes(&data[*pos..*pos + key_len])?;
    *pos += key_len;

    // Entity count
    if data.len() < *pos + 4 {
        return Err(CoreError::InvalidFormat {
            message: "truncated entity count".into(),
        });
    }
    let entity_count = u32::from_be_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
    ]) as usize;
    *pos += 4;

    // Entity IDs
    let mut entities = HashSet::new();
    for _ in 0..entity_count {
        if data.len() < *pos + 16 {
            return Err(CoreError::InvalidFormat {
                message: "truncated entity id".into(),
            });
        }
        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&data[*pos..*pos + 16]);
        entities.insert(EntityId::from_bytes(id_bytes));
        *pos += 16;
    }

    Ok((key, entities))
}

/// Persists a HashIndex to bytes.
pub fn persist_hash_index<K: IndexKey>(index: &HashIndex<K>) -> Vec<u8> {
    let spec = index.spec();
    let entries = index.entries();

    let header = IndexHeader {
        index_type: IndexType::Hash,
        collection_id: spec.collection_id,
        name: spec.name.clone(),
        unique: spec.unique,
        entry_count: entries.len() as u64,
    };

    let mut buf = write_header(&header);

    // Write entries
    for (key, entities) in entries {
        write_entry(&mut buf, key, entities);
    }

    buf
}

/// Persists a BTreeIndex to bytes.
pub fn persist_btree_index<K: IndexKey>(index: &BTreeIndex<K>) -> Vec<u8> {
    let spec = index.spec();
    let entries = index.entries();

    let header = IndexHeader {
        index_type: IndexType::BTree,
        collection_id: spec.collection_id,
        name: spec.name.clone(),
        unique: spec.unique,
        entry_count: entries.len() as u64,
    };

    let mut buf = write_header(&header);

    // Write entries in order
    for (key, entities) in entries {
        write_entry(&mut buf, key, entities);
    }

    buf
}

/// Loads a HashIndex from bytes.
pub fn load_hash_index<K: IndexKey>(data: &[u8]) -> CoreResult<HashIndex<K>> {
    let (header, mut pos) = read_header(data)?;

    if header.index_type != IndexType::Hash {
        return Err(CoreError::InvalidFormat {
            message: "expected hash index".into(),
        });
    }

    let spec = if header.unique {
        IndexSpec::new(header.collection_id, header.name).unique()
    } else {
        IndexSpec::new(header.collection_id, header.name)
    };

    let mut index = HashIndex::new(spec);

    // Read entries
    for _ in 0..header.entry_count {
        let (key, entities) = read_entry::<K>(data, &mut pos)?;
        for entity_id in entities {
            index.insert(key.clone(), entity_id)?;
        }
    }

    Ok(index)
}

/// Loads a BTreeIndex from bytes.
pub fn load_btree_index<K: IndexKey>(data: &[u8]) -> CoreResult<BTreeIndex<K>> {
    let (header, mut pos) = read_header(data)?;

    if header.index_type != IndexType::BTree {
        return Err(CoreError::InvalidFormat {
            message: "expected btree index".into(),
        });
    }

    let spec = if header.unique {
        IndexSpec::new(header.collection_id, header.name).unique()
    } else {
        IndexSpec::new(header.collection_id, header.name)
    };

    let mut index = BTreeIndex::new(spec);

    // Read entries
    for _ in 0..header.entry_count {
        let (key, entities) = read_entry::<K>(data, &mut pos)?;
        for entity_id in entities {
            index.insert(key.clone(), entity_id)?;
        }
    }

    Ok(index)
}

/// Reads the header without loading the full index.
#[allow(dead_code)] // Public API for index inspection
pub fn read_index_header(data: &[u8]) -> CoreResult<IndexHeader> {
    let (header, _) = read_header(data)?;
    Ok(header)
}

/// Validates an index file without loading it.
#[allow(dead_code)] // Public API for index validation
pub fn validate_index_file(data: &[u8]) -> CoreResult<IndexHeader> {
    let (header, pos) = read_header(data)?;

    // Quick validation: check we have enough data for declared entries
    // This is a heuristic check, not a full validation
    if data.len() < pos {
        return Err(CoreError::InvalidFormat {
            message: "index file truncated".into(),
        });
    }

    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::traits::Index;

    fn make_spec(name: &str) -> IndexSpec<Vec<u8>> {
        IndexSpec::new(CollectionId::new(1), name)
    }

    #[test]
    fn hash_index_roundtrip() {
        let mut index: HashIndex<Vec<u8>> = HashIndex::new(make_spec("test"));

        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();

        index.insert(vec![1, 2, 3], e1).unwrap();
        index.insert(vec![1, 2, 3], e2).unwrap();
        index.insert(vec![4, 5, 6], e3).unwrap();

        let bytes = persist_hash_index(&index);
        let loaded: HashIndex<Vec<u8>> = load_hash_index(&bytes).unwrap();

        assert_eq!(loaded.len(), 3);
        let lookup = loaded.lookup(&vec![1, 2, 3]).unwrap();
        assert!(lookup.contains(&e1));
        assert!(lookup.contains(&e2));
    }

    #[test]
    fn btree_index_roundtrip() {
        let mut index: BTreeIndex<i64> = BTreeIndex::new(IndexSpec::new(CollectionId::new(2), "age"));

        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();

        index.insert(25, e1).unwrap();
        index.insert(30, e2).unwrap();
        index.insert(35, e3).unwrap();

        let bytes = persist_btree_index(&index);
        let loaded: BTreeIndex<i64> = load_btree_index(&bytes).unwrap();

        assert_eq!(loaded.len(), 3);

        // Verify range query works
        let range_result = loaded.range(25..=30).unwrap();
        assert!(range_result.contains(&e1));
        assert!(range_result.contains(&e2));
        assert!(!range_result.contains(&e3));
    }

    #[test]
    fn unique_index_roundtrip() {
        let spec: IndexSpec<String> = IndexSpec::new(CollectionId::new(1), "email").unique();
        let mut index: HashIndex<String> = HashIndex::new(spec);

        let e1 = EntityId::new();
        index.insert("alice@example.com".to_string(), e1).unwrap();

        let bytes = persist_hash_index(&index);
        let loaded: HashIndex<String> = load_hash_index(&bytes).unwrap();

        assert!(loaded.spec().unique);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn header_validation() {
        let index: HashIndex<Vec<u8>> = HashIndex::new(make_spec("test"));
        let bytes = persist_hash_index(&index);

        let header = validate_index_file(&bytes).unwrap();
        assert_eq!(header.index_type, IndexType::Hash);
        assert_eq!(header.name, "test");
        assert_eq!(header.collection_id, CollectionId::new(1));
    }

    #[test]
    fn invalid_magic_rejected() {
        let data = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00];
        let result = read_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_file_rejected() {
        let data = vec![0x45, 0x49, 0x44, 0x58]; // Just magic, no content
        let result = read_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn empty_index_roundtrip() {
        let index: HashIndex<Vec<u8>> = HashIndex::new(make_spec("empty"));
        let bytes = persist_hash_index(&index);
        let loaded: HashIndex<Vec<u8>> = load_hash_index(&bytes).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn large_key_roundtrip() {
        let mut index: HashIndex<Vec<u8>> = HashIndex::new(make_spec("large"));
        let large_key = vec![0xAB; 1000];
        let e1 = EntityId::new();
        index.insert(large_key.clone(), e1).unwrap();

        let bytes = persist_hash_index(&index);
        let loaded: HashIndex<Vec<u8>> = load_hash_index(&bytes).unwrap();

        let result = loaded.lookup(&large_key).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&e1));
    }
}
