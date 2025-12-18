//! Sync operations.

use entidb_codec::{from_cbor, to_canonical_cbor, CodecResult, Value};

/// Type of sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    /// Entity was created or updated.
    Put,
    /// Entity was deleted.
    Delete,
}

impl OperationType {
    /// Converts to a numeric code for CBOR encoding.
    pub fn to_code(&self) -> u8 {
        match self {
            OperationType::Put => 1,
            OperationType::Delete => 2,
        }
    }

    /// Converts from a numeric code.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(OperationType::Put),
            2 => Some(OperationType::Delete),
            _ => None,
        }
    }
}

/// A sync operation that can be replicated.
///
/// `SyncOperation` represents a single committed change that can be
/// transmitted between databases for synchronization.
///
/// # Fields
///
/// - `op_id`: Unique operation identifier (monotonically increasing)
/// - `collection_id`: The collection this operation affects
/// - `entity_id`: The entity being modified
/// - `op_type`: Put or Delete
/// - `payload`: For Put operations, the entity's CBOR bytes
/// - `sequence`: The commit sequence number
#[derive(Debug, Clone, PartialEq)]
pub struct SyncOperation {
    /// Unique operation ID.
    pub op_id: u64,
    /// Collection ID.
    pub collection_id: u32,
    /// Entity ID (16 bytes).
    pub entity_id: [u8; 16],
    /// Operation type.
    pub op_type: OperationType,
    /// Entity payload (for Put operations).
    pub payload: Option<Vec<u8>>,
    /// Commit sequence number.
    pub sequence: u64,
}

impl SyncOperation {
    /// Creates a new Put operation.
    pub fn put(
        op_id: u64,
        collection_id: u32,
        entity_id: [u8; 16],
        payload: Vec<u8>,
        sequence: u64,
    ) -> Self {
        Self {
            op_id,
            collection_id,
            entity_id,
            op_type: OperationType::Put,
            payload: Some(payload),
            sequence,
        }
    }

    /// Creates a new Delete operation.
    pub fn delete(op_id: u64, collection_id: u32, entity_id: [u8; 16], sequence: u64) -> Self {
        Self {
            op_id,
            collection_id,
            entity_id,
            op_type: OperationType::Delete,
            payload: None,
            sequence,
        }
    }

    /// Encodes to canonical CBOR bytes.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let mut pairs = vec![
            (
                Value::Text("op_id".into()),
                Value::Integer(self.op_id as i64),
            ),
            (
                Value::Text("collection_id".into()),
                Value::Integer(i64::from(self.collection_id)),
            ),
            (
                Value::Text("entity_id".into()),
                Value::Bytes(self.entity_id.to_vec()),
            ),
            (
                Value::Text("op_type".into()),
                Value::Integer(i64::from(self.op_type.to_code())),
            ),
            (
                Value::Text("sequence".into()),
                Value::Integer(self.sequence as i64),
            ),
        ];

        if let Some(ref payload) = self.payload {
            pairs.push((Value::Text("payload".into()), Value::Bytes(payload.clone())));
        }

        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR bytes.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value.as_map().ok_or_else(|| {
            entidb_codec::CodecError::invalid_structure("expected map for SyncOperation")
        })?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let op_id = get_field("op_id")
            .and_then(|v: &Value| v.as_integer())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing op_id"))?
            as u64;

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

        let op_type_code = get_field("op_type")
            .and_then(|v: &Value| v.as_integer())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing op_type"))?
            as u8;

        let op_type = OperationType::from_code(op_type_code)
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("invalid op_type"))?;

        let sequence = get_field("sequence")
            .and_then(|v: &Value| v.as_integer())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing sequence"))?
            as u64;

        let payload = get_field("payload")
            .and_then(|v: &Value| v.as_bytes())
            .map(|b| b.to_vec());

        Ok(Self {
            op_id,
            collection_id,
            entity_id,
            op_type,
            payload,
            sequence,
        })
    }

    /// Returns the size of the payload in bytes.
    pub fn payload_size(&self) -> usize {
        self.payload.as_ref().map(|p| p.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_type_codes() {
        assert_eq!(OperationType::Put.to_code(), 1);
        assert_eq!(OperationType::Delete.to_code(), 2);

        assert_eq!(OperationType::from_code(1), Some(OperationType::Put));
        assert_eq!(OperationType::from_code(2), Some(OperationType::Delete));
        assert_eq!(OperationType::from_code(0), None);
    }

    #[test]
    fn put_operation_roundtrip() {
        let entity_id = [1u8; 16];
        let payload = vec![0xA1, 0x64, 0x74, 0x65, 0x73, 0x74]; // {"test"}

        let op = SyncOperation::put(1, 100, entity_id, payload.clone(), 42);

        let bytes = op.encode().unwrap();
        let decoded = SyncOperation::decode(&bytes).unwrap();

        assert_eq!(decoded.op_id, 1);
        assert_eq!(decoded.collection_id, 100);
        assert_eq!(decoded.entity_id, entity_id);
        assert_eq!(decoded.op_type, OperationType::Put);
        assert_eq!(decoded.payload, Some(payload));
        assert_eq!(decoded.sequence, 42);
    }

    #[test]
    fn delete_operation_roundtrip() {
        let entity_id = [2u8; 16];

        let op = SyncOperation::delete(5, 200, entity_id, 99);

        let bytes = op.encode().unwrap();
        let decoded = SyncOperation::decode(&bytes).unwrap();

        assert_eq!(decoded.op_id, 5);
        assert_eq!(decoded.collection_id, 200);
        assert_eq!(decoded.entity_id, entity_id);
        assert_eq!(decoded.op_type, OperationType::Delete);
        assert_eq!(decoded.payload, None);
        assert_eq!(decoded.sequence, 99);
    }

    #[test]
    fn payload_size() {
        let put = SyncOperation::put(1, 1, [0u8; 16], vec![1, 2, 3, 4, 5], 1);
        assert_eq!(put.payload_size(), 5);

        let delete = SyncOperation::delete(2, 1, [0u8; 16], 2);
        assert_eq!(delete.payload_size(), 0);
    }
}
