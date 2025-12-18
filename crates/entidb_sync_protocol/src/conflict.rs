//! Conflict detection and resolution.

use entidb_codec::{from_cbor, to_canonical_cbor, CodecResult, Value};

/// A conflict between local and remote operations.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// Collection ID.
    pub collection_id: u32,
    /// Entity ID.
    pub entity_id: [u8; 16],
    /// Local version hash.
    pub local_hash: Option<[u8; 32]>,
    /// Remote version hash.
    pub remote_hash: Option<[u8; 32]>,
    /// Local payload.
    pub local_payload: Option<Vec<u8>>,
    /// Remote payload.
    pub remote_payload: Option<Vec<u8>>,
    /// Resolution (if any).
    pub resolution: Option<ConflictResolution>,
}

impl Conflict {
    /// Creates a new conflict.
    pub fn new(
        collection_id: u32,
        entity_id: [u8; 16],
        local_hash: Option<[u8; 32]>,
        remote_hash: Option<[u8; 32]>,
        local_payload: Option<Vec<u8>>,
        remote_payload: Option<Vec<u8>>,
    ) -> Self {
        Self {
            collection_id,
            entity_id,
            local_hash,
            remote_hash,
            local_payload,
            remote_payload,
            resolution: None,
        }
    }

    /// Returns true if this is a create-create conflict.
    pub fn is_create_conflict(&self) -> bool {
        self.local_hash.is_none() && self.remote_hash.is_none()
    }

    /// Returns true if this is an update-update conflict.
    pub fn is_update_conflict(&self) -> bool {
        self.local_hash.is_some()
            && self.remote_hash.is_some()
            && self.local_payload.is_some()
            && self.remote_payload.is_some()
    }

    /// Returns true if this is an update-delete conflict.
    pub fn is_update_delete_conflict(&self) -> bool {
        (self.local_payload.is_some() && self.remote_payload.is_none())
            || (self.local_payload.is_none() && self.remote_payload.is_some())
    }

    /// Resolves the conflict with the given resolution.
    pub fn resolve(&mut self, resolution: ConflictResolution) {
        self.resolution = Some(resolution);
    }

    /// Returns true if the conflict has been resolved.
    pub fn is_resolved(&self) -> bool {
        self.resolution.is_some()
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let mut pairs = vec![
            (
                Value::Text("collection_id".into()),
                Value::Integer(i64::from(self.collection_id)),
            ),
            (
                Value::Text("entity_id".into()),
                Value::Bytes(self.entity_id.to_vec()),
            ),
        ];

        if let Some(hash) = &self.local_hash {
            pairs.push((
                Value::Text("local_hash".into()),
                Value::Bytes(hash.to_vec()),
            ));
        }

        if let Some(hash) = &self.remote_hash {
            pairs.push((
                Value::Text("remote_hash".into()),
                Value::Bytes(hash.to_vec()),
            ));
        }

        if let Some(payload) = &self.local_payload {
            pairs.push((
                Value::Text("local_payload".into()),
                Value::Bytes(payload.clone()),
            ));
        }

        if let Some(payload) = &self.remote_payload {
            pairs.push((
                Value::Text("remote_payload".into()),
                Value::Bytes(payload.clone()),
            ));
        }

        if let Some(resolution) = &self.resolution {
            pairs.push((
                Value::Text("resolution".into()),
                Value::Integer(i64::from(resolution.to_code())),
            ));
        }

        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value.as_map().ok_or_else(|| {
            entidb_codec::CodecError::invalid_structure("expected map for Conflict")
        })?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let collection_id = get_field("collection_id")
            .and_then(|v: &Value| v.as_integer())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing collection_id"))?
            as u32;

        let entity_id_bytes = get_field("entity_id")
            .and_then(|v: &Value| v.as_bytes())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing entity_id"))?;

        let entity_id: [u8; 16] = entity_id_bytes.try_into().map_err(|_| {
            entidb_codec::CodecError::invalid_structure("entity_id must be 16 bytes")
        })?;

        let local_hash = get_field("local_hash")
            .and_then(|v: &Value| v.as_bytes())
            .and_then(|b| <[u8; 32]>::try_from(b).ok());

        let remote_hash = get_field("remote_hash")
            .and_then(|v: &Value| v.as_bytes())
            .and_then(|b| <[u8; 32]>::try_from(b).ok());

        let local_payload = get_field("local_payload")
            .and_then(|v: &Value| v.as_bytes())
            .map(|b| b.to_vec());

        let remote_payload = get_field("remote_payload")
            .and_then(|v: &Value| v.as_bytes())
            .map(|b| b.to_vec());

        let resolution = get_field("resolution")
            .and_then(|v: &Value| v.as_integer())
            .and_then(|code| ConflictResolution::from_code(code as u8));

        Ok(Self {
            collection_id,
            entity_id,
            local_hash,
            remote_hash,
            local_payload,
            remote_payload,
            resolution,
        })
    }
}

/// Resolution for a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Keep local version.
    KeepLocal,
    /// Accept remote version.
    AcceptRemote,
    /// Merge (requires custom merge function).
    Merge,
    /// Skip (leave unresolved).
    Skip,
}

impl ConflictResolution {
    /// Converts to a code.
    pub fn to_code(&self) -> u8 {
        match self {
            ConflictResolution::KeepLocal => 1,
            ConflictResolution::AcceptRemote => 2,
            ConflictResolution::Merge => 3,
            ConflictResolution::Skip => 4,
        }
    }

    /// Converts from a code.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(ConflictResolution::KeepLocal),
            2 => Some(ConflictResolution::AcceptRemote),
            3 => Some(ConflictResolution::Merge),
            4 => Some(ConflictResolution::Skip),
            _ => None,
        }
    }
}

/// Policy for automatic conflict resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Server always wins.
    ServerWins,
    /// Client always wins.
    ClientWins,
    /// Last write wins (by timestamp).
    LastWriteWins,
    /// Manual resolution required.
    Manual,
}

impl ConflictPolicy {
    /// Returns true if this policy automatically resolves conflicts.
    pub fn auto_resolves(&self) -> bool {
        !matches!(self, ConflictPolicy::Manual)
    }

    /// Resolves a conflict according to this policy.
    pub fn resolve(&self, conflict: &mut Conflict) {
        let resolution = match self {
            ConflictPolicy::ServerWins => ConflictResolution::AcceptRemote,
            ConflictPolicy::ClientWins => ConflictResolution::KeepLocal,
            ConflictPolicy::LastWriteWins => {
                // In absence of timestamps, prefer remote (server authoritative)
                ConflictResolution::AcceptRemote
            }
            ConflictPolicy::Manual => ConflictResolution::Skip,
        };
        conflict.resolve(resolution);
    }

    /// Converts to a code.
    pub fn to_code(&self) -> u8 {
        match self {
            ConflictPolicy::ServerWins => 1,
            ConflictPolicy::ClientWins => 2,
            ConflictPolicy::LastWriteWins => 3,
            ConflictPolicy::Manual => 4,
        }
    }

    /// Converts from a code.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(ConflictPolicy::ServerWins),
            2 => Some(ConflictPolicy::ClientWins),
            3 => Some(ConflictPolicy::LastWriteWins),
            4 => Some(ConflictPolicy::Manual),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_roundtrip() {
        let conflict = Conflict::new(
            1,
            [5u8; 16],
            Some([1u8; 32]),
            Some([2u8; 32]),
            Some(vec![1, 2, 3]),
            Some(vec![4, 5, 6]),
        );

        let bytes = conflict.encode().unwrap();
        let decoded = Conflict::decode(&bytes).unwrap();

        assert_eq!(decoded.collection_id, 1);
        assert_eq!(decoded.entity_id, [5u8; 16]);
        assert_eq!(decoded.local_hash, Some([1u8; 32]));
        assert_eq!(decoded.remote_hash, Some([2u8; 32]));
        assert_eq!(decoded.local_payload, Some(vec![1, 2, 3]));
        assert_eq!(decoded.remote_payload, Some(vec![4, 5, 6]));
    }

    #[test]
    fn conflict_types() {
        // Create-create conflict
        let cc = Conflict::new(1, [0u8; 16], None, None, Some(vec![1]), Some(vec![2]));
        assert!(cc.is_create_conflict());

        // Update-update conflict
        let uu = Conflict::new(
            1,
            [0u8; 16],
            Some([1u8; 32]),
            Some([2u8; 32]),
            Some(vec![1]),
            Some(vec![2]),
        );
        assert!(uu.is_update_conflict());

        // Update-delete conflict
        let ud = Conflict::new(1, [0u8; 16], Some([1u8; 32]), Some([2u8; 32]), Some(vec![1]), None);
        assert!(ud.is_update_delete_conflict());
    }

    #[test]
    fn resolution_codes() {
        assert_eq!(
            ConflictResolution::from_code(1),
            Some(ConflictResolution::KeepLocal)
        );
        assert_eq!(
            ConflictResolution::from_code(2),
            Some(ConflictResolution::AcceptRemote)
        );
        assert_eq!(
            ConflictResolution::from_code(3),
            Some(ConflictResolution::Merge)
        );
        assert_eq!(
            ConflictResolution::from_code(4),
            Some(ConflictResolution::Skip)
        );
        assert_eq!(ConflictResolution::from_code(0), None);
    }

    #[test]
    fn policy_resolution() {
        let mut conflict = Conflict::new(1, [0u8; 16], None, None, Some(vec![1]), Some(vec![2]));

        ConflictPolicy::ServerWins.resolve(&mut conflict);
        assert_eq!(conflict.resolution, Some(ConflictResolution::AcceptRemote));

        conflict.resolution = None;
        ConflictPolicy::ClientWins.resolve(&mut conflict);
        assert_eq!(conflict.resolution, Some(ConflictResolution::KeepLocal));

        conflict.resolution = None;
        ConflictPolicy::Manual.resolve(&mut conflict);
        assert_eq!(conflict.resolution, Some(ConflictResolution::Skip));
    }
}
