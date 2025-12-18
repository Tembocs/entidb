//! Database manifest for metadata storage.

use crate::error::{CoreError, CoreResult};
use crate::types::SequenceNumber;
use std::collections::HashMap;

/// Magic bytes for manifest file.
pub const MANIFEST_MAGIC: [u8; 4] = *b"EMFN";

/// Current manifest version.
pub const MANIFEST_VERSION: u16 = 1;

/// Database manifest containing metadata.
///
/// The manifest stores:
/// - Format version
/// - Collection registry
/// - Last checkpoint sequence
#[derive(Debug, Clone)]
pub struct Manifest {
    /// Format version (major, minor).
    pub format_version: (u16, u16),
    /// Collection name to ID mapping.
    pub collections: HashMap<String, u32>,
    /// Next collection ID to assign.
    pub next_collection_id: u32,
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
            collections: HashMap::new(),
            next_collection_id: 1,
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

    /// Encodes the manifest to bytes.
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

        // Collections
        for (name, &id) in &self.collections {
            let name_bytes = name.as_bytes();
            let name_len = u16::try_from(name_bytes.len()).unwrap_or(u16::MAX);
            buf.extend_from_slice(&name_len.to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&id.to_le_bytes());
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
        let mut collections = HashMap::new();
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
    fn decode_empty_manifest() {
        let manifest = Manifest::default();
        let encoded = manifest.encode();
        let decoded = Manifest::decode(&encoded).unwrap();
        assert!(decoded.collections.is_empty());
        assert!(decoded.last_checkpoint.is_none());
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
}
