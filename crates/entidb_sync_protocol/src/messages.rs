//! Protocol messages for sync.

use crate::conflict::Conflict;
use crate::operation::SyncOperation;
use entidb_codec::{from_cbor, to_canonical_cbor, CodecResult, Value};

/// A sync protocol message.
#[derive(Debug, Clone)]
pub enum SyncMessage {
    /// Handshake request.
    HandshakeRequest(HandshakeRequest),
    /// Handshake response.
    HandshakeResponse(HandshakeResponse),
    /// Pull request.
    PullRequest(PullRequest),
    /// Pull response.
    PullResponse(PullResponse),
    /// Push request.
    PushRequest(PushRequest),
    /// Push response.
    PushResponse(PushResponse),
}

impl SyncMessage {
    /// Returns the message type code.
    pub fn type_code(&self) -> u8 {
        match self {
            SyncMessage::HandshakeRequest(_) => 1,
            SyncMessage::HandshakeResponse(_) => 2,
            SyncMessage::PullRequest(_) => 3,
            SyncMessage::PullResponse(_) => 4,
            SyncMessage::PushRequest(_) => 5,
            SyncMessage::PushResponse(_) => 6,
        }
    }
}

/// Handshake request from client.
#[derive(Debug, Clone, PartialEq)]
pub struct HandshakeRequest {
    /// Database ID.
    pub db_id: [u8; 16],
    /// Device ID.
    pub device_id: [u8; 16],
    /// Protocol version.
    pub protocol_version: u16,
    /// Client's last known cursor.
    pub last_cursor: u64,
}

impl HandshakeRequest {
    /// Creates a new handshake request.
    pub fn new(db_id: [u8; 16], device_id: [u8; 16], last_cursor: u64) -> Self {
        Self {
            db_id,
            device_id,
            protocol_version: 1,
            last_cursor,
        }
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let pairs = vec![
            (
                Value::Text("db_id".into()),
                Value::Bytes(self.db_id.to_vec()),
            ),
            (
                Value::Text("device_id".into()),
                Value::Bytes(self.device_id.to_vec()),
            ),
            (
                Value::Text("protocol_version".into()),
                Value::Integer(i64::from(self.protocol_version)),
            ),
            (
                Value::Text("last_cursor".into()),
                Value::Integer(self.last_cursor as i64),
            ),
        ];
        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value
            .as_map()
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("expected map"))?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let db_id: [u8; 16] = get_field("db_id")
            .and_then(|v: &Value| v.as_bytes())
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing db_id"))?;

        let device_id: [u8; 16] = get_field("device_id")
            .and_then(|v: &Value| v.as_bytes())
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("missing device_id"))?;

        let protocol_version = get_field("protocol_version")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(1) as u16;

        let last_cursor = get_field("last_cursor")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(0) as u64;

        Ok(Self {
            db_id,
            device_id,
            protocol_version,
            last_cursor,
        })
    }
}

/// Handshake response from server.
#[derive(Debug, Clone, PartialEq)]
pub struct HandshakeResponse {
    /// Whether handshake succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
    /// Server's protocol version.
    pub protocol_version: u16,
    /// Server's current cursor.
    pub server_cursor: u64,
}

impl HandshakeResponse {
    /// Creates a successful handshake response.
    pub fn success(server_cursor: u64) -> Self {
        Self {
            success: true,
            error: None,
            protocol_version: 1,
            server_cursor,
        }
    }

    /// Creates a failed handshake response.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(message.into()),
            protocol_version: 1,
            server_cursor: 0,
        }
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let mut pairs = vec![
            (Value::Text("success".into()), Value::Bool(self.success)),
            (
                Value::Text("protocol_version".into()),
                Value::Integer(i64::from(self.protocol_version)),
            ),
            (
                Value::Text("server_cursor".into()),
                Value::Integer(self.server_cursor as i64),
            ),
        ];

        if let Some(ref error) = self.error {
            pairs.push((Value::Text("error".into()), Value::Text(error.clone())));
        }

        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value
            .as_map()
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("expected map"))?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let success = get_field("success")
            .and_then(|v: &Value| v.as_bool())
            .unwrap_or(false);

        let error = get_field("error")
            .and_then(|v: &Value| v.as_text())
            .map(|s| s.to_string());

        let protocol_version = get_field("protocol_version")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(1) as u16;

        let server_cursor = get_field("server_cursor")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(0) as u64;

        Ok(Self {
            success,
            error,
            protocol_version,
            server_cursor,
        })
    }
}

/// Pull request from client.
#[derive(Debug, Clone, PartialEq)]
pub struct PullRequest {
    /// Cursor to pull from.
    pub cursor: u64,
    /// Maximum number of operations to return.
    pub limit: u32,
}

impl PullRequest {
    /// Creates a new pull request.
    pub fn new(cursor: u64, limit: u32) -> Self {
        Self { cursor, limit }
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let pairs = vec![
            (
                Value::Text("cursor".into()),
                Value::Integer(self.cursor as i64),
            ),
            (
                Value::Text("limit".into()),
                Value::Integer(i64::from(self.limit)),
            ),
        ];
        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value
            .as_map()
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("expected map"))?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let cursor = get_field("cursor")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(0) as u64;

        let limit = get_field("limit")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(100) as u32;

        Ok(Self { cursor, limit })
    }
}

/// Pull response from server.
#[derive(Debug, Clone)]
pub struct PullResponse {
    /// Operations since cursor.
    pub operations: Vec<SyncOperation>,
    /// New cursor after these operations.
    pub new_cursor: u64,
    /// Whether there are more operations.
    pub has_more: bool,
}

impl PullResponse {
    /// Creates a new pull response.
    pub fn new(operations: Vec<SyncOperation>, new_cursor: u64, has_more: bool) -> Self {
        Self {
            operations,
            new_cursor,
            has_more,
        }
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let ops: CodecResult<Vec<Value>> = self
            .operations
            .iter()
            .map(|op| {
                let bytes = op.encode()?;
                Ok(Value::Bytes(bytes))
            })
            .collect();

        let pairs = vec![
            (Value::Text("operations".into()), Value::Array(ops?)),
            (
                Value::Text("new_cursor".into()),
                Value::Integer(self.new_cursor as i64),
            ),
            (Value::Text("has_more".into()), Value::Bool(self.has_more)),
        ];

        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value
            .as_map()
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("expected map"))?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let operations: Vec<SyncOperation> = get_field("operations")
            .and_then(|v: &Value| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v: &Value| v.as_bytes())
                    .filter_map(|b| SyncOperation::decode(b).ok())
                    .collect()
            })
            .unwrap_or_default();

        let new_cursor = get_field("new_cursor")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(0) as u64;

        let has_more = get_field("has_more")
            .and_then(|v: &Value| v.as_bool())
            .unwrap_or(false);

        Ok(Self {
            operations,
            new_cursor,
            has_more,
        })
    }
}

/// Push request from client.
#[derive(Debug, Clone)]
pub struct PushRequest {
    /// Operations to push.
    pub operations: Vec<SyncOperation>,
    /// Expected server cursor (for conflict detection).
    pub expected_cursor: u64,
}

impl PushRequest {
    /// Creates a new push request.
    pub fn new(operations: Vec<SyncOperation>, expected_cursor: u64) -> Self {
        Self {
            operations,
            expected_cursor,
        }
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let ops: CodecResult<Vec<Value>> = self
            .operations
            .iter()
            .map(|op| {
                let bytes = op.encode()?;
                Ok(Value::Bytes(bytes))
            })
            .collect();

        let pairs = vec![
            (Value::Text("operations".into()), Value::Array(ops?)),
            (
                Value::Text("expected_cursor".into()),
                Value::Integer(self.expected_cursor as i64),
            ),
        ];

        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value
            .as_map()
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("expected map"))?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let operations: Vec<SyncOperation> = get_field("operations")
            .and_then(|v: &Value| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v: &Value| v.as_bytes())
                    .filter_map(|b| SyncOperation::decode(b).ok())
                    .collect()
            })
            .unwrap_or_default();

        let expected_cursor = get_field("expected_cursor")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(0) as u64;

        Ok(Self {
            operations,
            expected_cursor,
        })
    }
}

/// Push response from server.
#[derive(Debug, Clone)]
pub struct PushResponse {
    /// Whether push succeeded.
    pub success: bool,
    /// New cursor after push.
    pub new_cursor: u64,
    /// Conflicts if any.
    pub conflicts: Vec<Conflict>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl PushResponse {
    /// Creates a successful push response.
    pub fn success(new_cursor: u64) -> Self {
        Self {
            success: true,
            new_cursor,
            conflicts: Vec::new(),
            error: None,
        }
    }

    /// Creates a push response with conflicts.
    pub fn with_conflicts(new_cursor: u64, conflicts: Vec<Conflict>) -> Self {
        Self {
            success: conflicts.is_empty(),
            new_cursor,
            conflicts,
            error: None,
        }
    }

    /// Creates a failed push response.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            new_cursor: 0,
            conflicts: Vec::new(),
            error: Some(message.into()),
        }
    }

    /// Encodes to CBOR.
    pub fn encode(&self) -> CodecResult<Vec<u8>> {
        let conflicts: CodecResult<Vec<Value>> = self
            .conflicts
            .iter()
            .map(|c| {
                let bytes = c.encode()?;
                Ok(Value::Bytes(bytes))
            })
            .collect();

        let mut pairs = vec![
            (Value::Text("success".into()), Value::Bool(self.success)),
            (
                Value::Text("new_cursor".into()),
                Value::Integer(self.new_cursor as i64),
            ),
            (Value::Text("conflicts".into()), Value::Array(conflicts?)),
        ];

        if let Some(ref error) = self.error {
            pairs.push((Value::Text("error".into()), Value::Text(error.clone())));
        }

        to_canonical_cbor(&Value::map(pairs))
    }

    /// Decodes from CBOR.
    pub fn decode(bytes: &[u8]) -> CodecResult<Self> {
        let value: Value = from_cbor(bytes)?;
        let map = value
            .as_map()
            .ok_or_else(|| entidb_codec::CodecError::invalid_structure("expected map"))?;

        let get_field = |name: &str| {
            map.iter()
                .find(|(k, _)| k.as_text() == Some(name))
                .map(|(_, v)| v)
        };

        let success = get_field("success")
            .and_then(|v: &Value| v.as_bool())
            .unwrap_or(false);

        let new_cursor = get_field("new_cursor")
            .and_then(|v: &Value| v.as_integer())
            .unwrap_or(0) as u64;

        let conflicts: Vec<Conflict> = get_field("conflicts")
            .and_then(|v: &Value| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v: &Value| v.as_bytes())
                    .filter_map(|b| Conflict::decode(b).ok())
                    .collect()
            })
            .unwrap_or_default();

        let error = get_field("error")
            .and_then(|v: &Value| v.as_text())
            .map(|s| s.to_string());

        Ok(Self {
            success,
            new_cursor,
            conflicts,
            error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_request_roundtrip() {
        let req = HandshakeRequest::new([1u8; 16], [2u8; 16], 100);
        let bytes = req.encode().unwrap();
        let decoded = HandshakeRequest::decode(&bytes).unwrap();

        assert_eq!(decoded.db_id, [1u8; 16]);
        assert_eq!(decoded.device_id, [2u8; 16]);
        assert_eq!(decoded.last_cursor, 100);
    }

    #[test]
    fn handshake_response_success() {
        let resp = HandshakeResponse::success(500);
        let bytes = resp.encode().unwrap();
        let decoded = HandshakeResponse::decode(&bytes).unwrap();

        assert!(decoded.success);
        assert_eq!(decoded.server_cursor, 500);
        assert!(decoded.error.is_none());
    }

    #[test]
    fn handshake_response_error() {
        let resp = HandshakeResponse::error("version mismatch");
        let bytes = resp.encode().unwrap();
        let decoded = HandshakeResponse::decode(&bytes).unwrap();

        assert!(!decoded.success);
        assert_eq!(decoded.error, Some("version mismatch".to_string()));
    }

    #[test]
    fn pull_request_roundtrip() {
        let req = PullRequest::new(50, 200);
        let bytes = req.encode().unwrap();
        let decoded = PullRequest::decode(&bytes).unwrap();

        assert_eq!(decoded.cursor, 50);
        assert_eq!(decoded.limit, 200);
    }

    #[test]
    fn pull_response_with_operations() {
        let ops = vec![
            SyncOperation::put(1, 100, [1u8; 16], vec![1, 2, 3], 10),
            SyncOperation::delete(2, 100, [2u8; 16], 11),
        ];

        let resp = PullResponse::new(ops, 11, true);
        let bytes = resp.encode().unwrap();
        let decoded = PullResponse::decode(&bytes).unwrap();

        assert_eq!(decoded.operations.len(), 2);
        assert_eq!(decoded.new_cursor, 11);
        assert!(decoded.has_more);
    }

    #[test]
    fn push_request_roundtrip() {
        let ops = vec![SyncOperation::put(1, 50, [3u8; 16], vec![4, 5, 6], 20)];

        let req = PushRequest::new(ops, 100);
        let bytes = req.encode().unwrap();
        let decoded = PushRequest::decode(&bytes).unwrap();

        assert_eq!(decoded.operations.len(), 1);
        assert_eq!(decoded.expected_cursor, 100);
    }

    #[test]
    fn push_response_success() {
        let resp = PushResponse::success(150);
        let bytes = resp.encode().unwrap();
        let decoded = PushResponse::decode(&bytes).unwrap();

        assert!(decoded.success);
        assert_eq!(decoded.new_cursor, 150);
        assert!(decoded.conflicts.is_empty());
    }

    #[test]
    fn sync_message_type_codes() {
        assert_eq!(
            SyncMessage::HandshakeRequest(HandshakeRequest::new([0u8; 16], [0u8; 16], 0))
                .type_code(),
            1
        );
        assert_eq!(
            SyncMessage::HandshakeResponse(HandshakeResponse::success(0)).type_code(),
            2
        );
        assert_eq!(
            SyncMessage::PullRequest(PullRequest::new(0, 0)).type_code(),
            3
        );
        assert_eq!(
            SyncMessage::PullResponse(PullResponse::new(vec![], 0, false)).type_code(),
            4
        );
        assert_eq!(
            SyncMessage::PushRequest(PushRequest::new(vec![], 0)).type_code(),
            5
        );
        assert_eq!(
            SyncMessage::PushResponse(PushResponse::success(0)).type_code(),
            6
        );
    }
}
