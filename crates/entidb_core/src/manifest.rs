//! Database manifest for metadata storage.

use crate::error::{CoreError, CoreResult};
use crate::index::{IndexDefinition, IndexKind};
use crate::types::{CollectionId, SequenceNumber};
use std::collections::BTreeMap;

/// Magic bytes for manifest file.
pub const MANIFEST_MAGIC: [u8; 4] = *b"EMFN";

/// Current manifest version.
/// Version 2 adds index definitions.
pub const MANIFEST_VERSION: u16 = 2;

/// Database manifest containing metadata.
///
/// The manifest stores:
/// - Format version
/// - Collection registry
/// - Index registry (NEW)
/// - Last checkpoint sequence
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Format version (major, minor).
    pub format_version: (u16, u16),
    /// Collection name to ID mapping (BTreeMap for deterministic serialization).
    pub collections: BTreeMap<String, u32>,
    /// Next collection ID to assign.
    pub next_collection_id: u32,
    /// Index definitions (stored for persistence and rebuild on open).
    pub indexes: Vec<IndexDefinition>,
    /// Next index ID to assign.
    pub next_index_id: u64,
    /// Last checkpoint sequence number.
    pub last_checkpoint: Option<SequenceNumber>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new((1, 0))
    }
}

impl Manifest {
    /// Creates a new empty manifest.
    #[must_use]
    pub fn new(format_version: (u16, u16)) -> Self {
        Self {
            format_version,
            collections: BTreeMap::new(),
            next_collection_id: 1,
            indexes: Vec::new(),
            next_index_id: 1,
            last_checkpoint: None,
        }
    }

    /// Gets or creates a collection ID for a name.
    pub fn get_or_create_collection(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.collections.get(name) {
            return id;
        }

        let id = self.next_collection_id;
        self.next_collection_id += 1;
        self.collections.insert(name.to_string(), id);
        id
    }

    /// Gets a collection ID by name.
    #[must_use]
    pub fn get_collection(&self, name: &str) -> Option<u32> {
        self.collections.get(name).copied()
    }

    /// Adds an index definition and returns its ID.
    pub fn add_index(&mut self, mut def: IndexDefinition) -> u64 {
        // Check if already exists
        for existing in &self.indexes {
            if existing.collection_id == def.collection_id
                && existing.field_path == def.field_path
                && existing.kind == def.kind
                && existing.unique == def.unique
            {
                return existing.id;
            }
        }

        def.id = self.next_index_id;
        self.next_index_id += 1;
        let id = def.id;
        self.indexes.push(def);
        id
    }

    /// Removes an index definition by ID.
    pub fn remove_index(&mut self, id: u64) -> bool {
        if let Some(pos) = self.indexes.iter().position(|d| d.id == id) {
            self.indexes.remove(pos);
            true
        } else {
            false
        }
    }

    /// Gets all index definitions.
    #[must_use]
    pub fn get_indexes(&self) -> &[IndexDefinition] {
        &self.indexes
    }

    /// Encodes the manifest to bytes (deterministic).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Magic
        buf.extend_from_slice(&MANIFEST_MAGIC);

        // Version
        buf.extend_from_slice(&MANIFEST_VERSION.to_le_bytes());

        // Format version
        buf.extend_from_slice(&self.format_version.0.to_le_bytes());
        buf.extend_from_slice(&self.format_version.1.to_le_bytes());

        // Next collection ID
        buf.extend_from_slice(&self.next_collection_id.to_le_bytes());

        // Collection count
        let count = u32::try_from(self.collections.len()).unwrap_or(u32::MAX);
        buf.extend_from_slice(&count.to_le_bytes());

        // Collections (BTreeMap ensures deterministic order)
        for (name, &id) in &self.collections {
            let name_bytes = name.as_bytes();
            let name_len = u16::try_from(name_bytes.len()).unwrap_or(u16::MAX);
            buf.extend_from_slice(&name_len.to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&id.to_le_bytes());
        }

        // Next index ID
        buf.extend_from_slice(&self.next_index_id.to_le_bytes());

        // Index count
        let idx_count = u32::try_from(self.indexes.len()).unwrap_or(u32::MAX);
        buf.extend_from_slice(&idx_count.to_le_bytes());

        // Indexes (sorted by ID for determinism)
        let mut sorted_indexes: Vec<_> = self.indexes.iter().collect();
        sorted_indexes.sort_by_key(|d| d.id);

        for def in sorted_indexes {
            // Index ID (8 bytes)
            buf.extend_from_slice(&def.id.to_le_bytes());
            // Collection ID (4 bytes)
            buf.extend_from_slice(&def.collection_id.as_u32().to_le_bytes());
            // Field path count (2 bytes)
            let path_count = u16::try_from(def.field_path.len()).unwrap_or(u16::MAX);
            buf.extend_from_slice(&path_count.to_le_bytes());
            // Field path elements
            for field in &def.field_path {
                let field_bytes = field.as_bytes();
                let field_len = u16::try_from(field_bytes.len()).unwrap_or(u16::MAX);
                buf.extend_from_slice(&field_len.to_le_bytes());
                buf.extend_from_slice(field_bytes);
            }
            // Kind (1 byte)
            buf.push(def.kind as u8);
            // Unique (1 byte)
            buf.push(if def.unique { 1 } else { 0 });
            // Created at sequence (8 bytes)
            buf.extend_from_slice(&def.created_at_seq.as_u64().to_le_bytes());
        }

        // Last checkpoint
        if let Some(seq) = self.last_checkpoint {
            buf.push(1);
            buf.extend_from_slice(&seq.as_u64().to_le_bytes());
        } else {
            buf.push(0);
        }

        buf
    }

    /// Decodes a manifest from bytes.
    pub fn decode(data: &[u8]) -> CoreResult<Self> {
        let mut cursor = 0;

        // Magic
        if data.len() < 4 || data[0..4] != MANIFEST_MAGIC {
            return Err(CoreError::invalid_format("invalid manifest magic"));
        }
        cursor += 4;

        // Version
        if cursor + 2 > data.len() {
            return Err(CoreError::invalid_format("manifest too short"));
        }
        let version = u16::from_le_bytes([data[cursor], data[cursor + 1]]);
        cursor += 2;
        if version > MANIFEST_VERSION {
            return Err(CoreError::invalid_format(format!(
                "unsupported manifest version: {version}"
            )));
        }

        // Format version
        if cursor + 4 > data.len() {
            return Err(CoreError::invalid_format("manifest too short"));
        }
        let format_major = u16::from_le_bytes([data[cursor], data[cursor + 1]]);
        cursor += 2;
        let format_minor = u16::from_le_bytes([data[cursor], data[cursor + 1]]);
        cursor += 2;

        // Next collection ID
        if cursor + 4 > data.len() {
            return Err(CoreError::invalid_format("manifest too short"));
        }
        let next_collection_id = u32::from_le_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]);
        cursor += 4;

        // Collection count
        if cursor + 4 > data.len() {
            return Err(CoreError::invalid_format("manifest too short"));
        }
        let collection_count = u32::from_le_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        // Collections
        let mut collections = BTreeMap::new();
        for _ in 0..collection_count {
            if cursor + 2 > data.len() {
                return Err(CoreError::invalid_format("manifest too short"));
            }
            let name_len = u16::from_le_bytes([data[cursor], data[cursor + 1]]) as usize;
            cursor += 2;

            if cursor + name_len + 4 > data.len() {
                return Err(CoreError::invalid_format("manifest too short"));
            }
            let name = std::str::from_utf8(&data[cursor..cursor + name_len])
                .map_err(|_| CoreError::invalid_format("invalid collection name"))?
                .to_string();
            cursor += name_len;

            let id = u32::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]);
            cursor += 4;

            collections.insert(name, id);
        }

        // Version 2+ has index definitions
        let (next_index_id, indexes) = if version >= 2 {
            // Next index ID
            if cursor + 8 > data.len() {
                return Err(CoreError::invalid_format("manifest too short"));
            }
            let next_idx = u64::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
                data[cursor + 4],
                data[cursor + 5],
                data[cursor + 6],
                data[cursor + 7],
            ]);
            cursor += 8;

            // Index count
            if cursor + 4 > data.len() {
                return Err(CoreError::invalid_format("manifest too short"));
            }
            let idx_count = u32::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]) as usize;
            cursor += 4;

            // Indexes
            let mut indexes = Vec::with_capacity(idx_count);
            for _ in 0..idx_count {
                // Index ID
                if cursor + 8 > data.len() {
                    return Err(CoreError::invalid_format("manifest too short"));
                }
                let id = u64::from_le_bytes([
                    data[cursor],
                    data[cursor + 1],
                    data[cursor + 2],
                    data[cursor + 3],
                    data[cursor + 4],
                    data[cursor + 5],
                    data[cursor + 6],
                    data[cursor + 7],
                ]);
                cursor += 8;

                // Collection ID
                if cursor + 4 > data.len() {
                    return Err(CoreError::invalid_format("manifest too short"));
                }
                let coll_id = u32::from_le_bytes([
                    data[cursor],
                    data[cursor + 1],
                    data[cursor + 2],
                    data[cursor + 3],
                ]);
                cursor += 4;

                // Field path count
                if cursor + 2 > data.len() {
                    return Err(CoreError::invalid_format("manifest too short"));
                }
                let path_count = u16::from_le_bytes([data[cursor], data[cursor + 1]]) as usize;
                cursor += 2;

                // Field path elements
                let mut field_path = Vec::with_capacity(path_count);
                for _ in 0..path_count {
                    if cursor + 2 > data.len() {
                        return Err(CoreError::invalid_format("manifest too short"));
                    }
                    let field_len =
                        u16::from_le_bytes([data[cursor], data[cursor + 1]]) as usize;
                    cursor += 2;

                    if cursor + field_len > data.len() {
                        return Err(CoreError::invalid_format("manifest too short"));
                    }
                    let field = std::str::from_utf8(&data[cursor..cursor + field_len])
                        .map_err(|_| CoreError::invalid_format("invalid field path"))?
                        .to_string();
                    cursor += field_len;
                    field_path.push(field);
                }

                // Kind
                if cursor + 1 > data.len() {
                    return Err(CoreError::invalid_format("manifest too short"));
                }
                let kind = IndexKind::try_from(data[cursor])?;
                cursor += 1;

                // Unique
                if cursor + 1 > data.len() {
                    return Err(CoreError::invalid_format("manifest too short"));
                }
                let unique = data[cursor] != 0;
                cursor += 1;

                // Created at sequence
                if cursor + 8 > data.len() {
                    return Err(CoreError::invalid_format("manifest too short"));
                }
                let created_at = u64::from_le_bytes([
                    data[cursor],
                    data[cursor + 1],
                    data[cursor + 2],
                    data[cursor + 3],
                    data[cursor + 4],
                    data[cursor + 5],
                    data[cursor + 6],
                    data[cursor + 7],
                ]);
                cursor += 8;

                indexes.push(IndexDefinition {
                    id,
                    collection_id: CollectionId::new(coll_id),
                    field_path,
                    kind,
                    unique,
                    created_at_seq: SequenceNumber::new(created_at),
                });
            }

            (next_idx, indexes)
        } else {
            (1, Vec::new())
        };

        // Last checkpoint
        let last_checkpoint = if cursor < data.len() && data[cursor] != 0 {
            cursor += 1;
            if cursor + 8 > data.len() {
                return Err(CoreError::invalid_format("manifest too short"));
            }
            let seq = u64::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
                data[cursor + 4],
                data[cursor + 5],
                data[cursor + 6],
                data[cursor + 7],
            ]);
            Some(SequenceNumber::new(seq))
        } else {
            None
        };

        Ok(Self {
            format_version: (format_major, format_minor),
            collections,
            next_collection_id,
            indexes,
            next_index_id,
            last_checkpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manifest() {
        let manifest = Manifest::new((1, 0));
        assert_eq!(manifest.format_version, (1, 0));
        assert!(manifest.collections.is_empty());
        assert_eq!(manifest.next_collection_id, 1);
        assert!(manifest.indexes.is_empty());
        assert_eq!(manifest.next_index_id, 1);
    }

    #[test]
    fn get_or_create_collection() {
        let mut manifest = Manifest::default();

        let id1 = manifest.get_or_create_collection("users");
        let id2 = manifest.get_or_create_collection("posts");
        let id1_again = manifest.get_or_create_collection("users");

        assert_eq!(id1, id1_again);
        assert_ne!(id1, id2);
        assert_eq!(manifest.collections.len(), 2);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let mut manifest = Manifest::new((1, 2));
        manifest.get_or_create_collection("users");
        manifest.get_or_create_collection("products");
        manifest.last_checkpoint = Some(SequenceNumber::new(42));

        let encoded = manifest.encode();
        let decoded = Manifest::decode(&encoded).unwrap();

        assert_eq!(decoded.format_version, manifest.format_version);
        assert_eq!(decoded.collections, manifest.collections);
        assert_eq!(decoded.next_collection_id, manifest.next_collection_id);
        assert_eq!(decoded.last_checkpoint, manifest.last_checkpoint);
    }

    #[test]
    fn encode_decode_with_indexes() {
        let mut manifest = Manifest::new((1, 0));
        manifest.get_or_create_collection("users");
        
        let def = IndexDefinition {
            id: 0, // Will be assigned
            collection_id: CollectionId::new(1),
            field_path: vec!["email".to_string()],
            kind: IndexKind::Hash,
            unique: true,
            created_at_seq: SequenceNumber::new(5),
        };
        manifest.add_index(def);

        let def2 = IndexDefinition {
            id: 0,
            collection_id: CollectionId::new(1),
            field_path: vec!["profile".to_string(), "age".to_string()],
            kind: IndexKind::BTree,
            unique: false,
            created_at_seq: SequenceNumber::new(10),
        };
        manifest.add_index(def2);

        let encoded = manifest.encode();
        let decoded = Manifest::decode(&encoded).unwrap();

        assert_eq!(decoded.indexes.len(), 2);
        assert_eq!(decoded.indexes[0].field_path, vec!["email"]);
        assert_eq!(decoded.indexes[0].kind, IndexKind::Hash);
        assert!(decoded.indexes[0].unique);
        assert_eq!(decoded.indexes[1].field_path, vec!["profile", "age"]);
        assert_eq!(decoded.indexes[1].kind, IndexKind::BTree);
        assert!(!decoded.indexes[1].unique);
    }

    #[test]
    fn decode_empty_manifest() {
        let manifest = Manifest::default();
        let encoded = manifest.encode();
        let decoded = Manifest::decode(&encoded).unwrap();
        assert!(decoded.collections.is_empty());
        assert!(decoded.last_checkpoint.is_none());
        assert!(decoded.indexes.is_empty());
    }

    #[test]
    fn invalid_magic_rejected() {
        let result = Manifest::decode(b"XXXX");
        assert!(result.is_err());
    }

    #[test]
    fn get_collection_not_found() {
        let manifest = Manifest::default();
        assert!(manifest.get_collection("nonexistent").is_none());
    }

    #[test]
    fn add_duplicate_index_returns_same_id() {
        let mut manifest = Manifest::default();
        
        let def1 = IndexDefinition {
            id: 0,
            collection_id: CollectionId::new(1),
            field_path: vec!["email".to_string()],
            kind: IndexKind::Hash,
            unique: true,
            created_at_seq: SequenceNumber::new(0),
        };
        let id1 = manifest.add_index(def1);

        let def2 = IndexDefinition {
            id: 0,
            collection_id: CollectionId::new(1),
            field_path: vec!["email".to_string()],
            kind: IndexKind::Hash,
            unique: true,
            created_at_seq: SequenceNumber::new(0),
        };
        let id2 = manifest.add_index(def2);

        assert_eq!(id1, id2);
        assert_eq!(manifest.indexes.len(), 1);
    }

    #[test]
    fn remove_index() {
        let mut manifest = Manifest::default();
        
        let def = IndexDefinition {
            id: 0,
            collection_id: CollectionId::new(1),
            field_path: vec!["email".to_string()],
            kind: IndexKind::Hash,
            unique: true,
            created_at_seq: SequenceNumber::new(0),
        };
        let id = manifest.add_index(def);
        assert_eq!(manifest.indexes.len(), 1);

        assert!(manifest.remove_index(id));
        assert!(manifest.indexes.is_empty());

        // Removing again returns false
        assert!(!manifest.remove_index(id));
    }

    #[test]
    fn deterministic_encoding() {
        // Test 1: Same manifest encodes identically on repeated calls
        let mut m1 = Manifest::default();
        m1.get_or_create_collection("users");
        m1.get_or_create_collection("posts");
        m1.add_index(IndexDefinition {
            id: 0,
            collection_id: CollectionId::new(1),
            field_path: vec!["email".to_string()],
            kind: IndexKind::Hash,
            unique: true,
            created_at_seq: SequenceNumber::new(0),
        });

        let enc1 = m1.encode();
        let enc2 = m1.encode();
        assert_eq!(enc1, enc2, "same manifest should encode identically");

        // Test 2: Two manifests with identical logical state encode identically
        // We manually construct them to have the exact same state
        let mut m3 = Manifest::new((1, 0));
        m3.collections.insert("alpha".to_string(), 10);
        m3.collections.insert("beta".to_string(), 20);
        m3.collections.insert("gamma".to_string(), 30);
        m3.next_collection_id = 31;
        m3.indexes.push(IndexDefinition {
            id: 5,
            collection_id: CollectionId::new(10),
            field_path: vec!["name".to_string()],
            kind: IndexKind::Hash,
            unique: false,
            created_at_seq: SequenceNumber::new(1),
        });
        m3.indexes.push(IndexDefinition {
            id: 3,
            collection_id: CollectionId::new(20),
            field_path: vec!["age".to_string()],
            kind: IndexKind::BTree,
            unique: true,
            created_at_seq: SequenceNumber::new(2),
        });
        m3.next_index_id = 6;
        m3.last_checkpoint = Some(SequenceNumber::new(100));

        let mut m4 = Manifest::new((1, 0));
        // Insert collections in DIFFERENT order - BTreeMap will sort them
        m4.collections.insert("gamma".to_string(), 30);
        m4.collections.insert("alpha".to_string(), 10);
        m4.collections.insert("beta".to_string(), 20);
        m4.next_collection_id = 31;
        // Insert indexes in DIFFERENT order - encode() sorts by ID
        m4.indexes.push(IndexDefinition {
            id: 3,
            collection_id: CollectionId::new(20),
            field_path: vec!["age".to_string()],
            kind: IndexKind::BTree,
            unique: true,
            created_at_seq: SequenceNumber::new(2),
        });
        m4.indexes.push(IndexDefinition {
            id: 5,
            collection_id: CollectionId::new(10),
            field_path: vec!["name".to_string()],
            kind: IndexKind::Hash,
            unique: false,
            created_at_seq: SequenceNumber::new(1),
        });
        m4.next_index_id = 6;
        m4.last_checkpoint = Some(SequenceNumber::new(100));

        let enc3 = m3.encode();
        let enc4 = m4.encode();
        assert_eq!(
            enc3, enc4,
            "manifests with same logical state must produce identical bytes"
        );

        // Test 3: Verify the encoding is stable across decode/re-encode
        let decoded = Manifest::decode(&enc3).unwrap();
        let enc5 = decoded.encode();
        assert_eq!(
            enc3, enc5,
            "decode then re-encode must produce identical bytes"
        );
    }
}
