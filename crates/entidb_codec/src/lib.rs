//! # EntiDB Codec
//!
//! Canonical CBOR encoding/decoding for EntiDB.
//!
//! This crate provides deterministic CBOR encoding that ensures:
//! - Identical inputs produce identical bytes
//! - Cross-platform consistency
//! - Stable hashing
//!
//! ## Canonical CBOR Rules
//!
//! - Maps are sorted by key (bytewise comparison of encoded keys)
//! - Integers use shortest encoding
//! - No floats unless explicitly allowed
//! - Strings must be UTF-8
//! - No indefinite-length items
//! - No NaN values
//!
//! ## Usage
//!
//! ```
//! use entidb_codec::{to_canonical_cbor, from_cbor, Value};
//!
//! // Encode a value
//! let value = Value::Integer(42);
//! let bytes = to_canonical_cbor(&value).unwrap();
//!
//! // Decode back
//! let decoded: Value = from_cbor(&bytes).unwrap();
//! assert_eq!(value, decoded);
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod decoder;
mod encoder;
mod error;
mod value;

pub use decoder::{from_cbor, CanonicalDecoder};
pub use encoder::{to_canonical_cbor, CanonicalEncoder};
pub use error::{CodecError, CodecResult};
pub use value::Value;

/// Trait for types that can be encoded to canonical CBOR.
pub trait Encode {
    /// Encode this value to canonical CBOR bytes.
    fn encode(&self) -> CodecResult<Vec<u8>>;
}

/// Trait for types that can be decoded from CBOR.
pub trait Decode: Sized {
    /// Decode this value from CBOR bytes.
    fn decode(bytes: &[u8]) -> CodecResult<Self>;
}

// Implement Encode for Value
impl Encode for Value {
    fn encode(&self) -> CodecResult<Vec<u8>> {
        to_canonical_cbor(self)
    }
}

// Implement Decode for Value
impl Decode for Value {
    fn decode(bytes: &[u8]) -> CodecResult<Self> {
        from_cbor(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_integer() {
        let value = Value::Integer(42);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_negative_integer() {
        let value = Value::Integer(-100);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_string() {
        let value = Value::Text("hello world".to_string());
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_bytes() {
        let value = Value::Bytes(vec![1, 2, 3, 4, 5]);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_array() {
        let value = Value::Array(vec![
            Value::Integer(1),
            Value::Text("two".to_string()),
            Value::Integer(3),
        ]);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_map() {
        let value = Value::map(vec![
            (Value::Text("a".to_string()), Value::Integer(1)),
            (Value::Text("b".to_string()), Value::Integer(2)),
        ]);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_bool_true() {
        let value = Value::Bool(true);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_bool_false() {
        let value = Value::Bool(false);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_null() {
        let value = Value::Null;
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn roundtrip_nested() {
        let value = Value::map(vec![
            (
                Value::Text("users".to_string()),
                Value::Array(vec![
                    Value::map(vec![
                        (
                            Value::Text("name".to_string()),
                            Value::Text("Alice".to_string()),
                        ),
                        (Value::Text("age".to_string()), Value::Integer(30)),
                    ]),
                    Value::map(vec![
                        (
                            Value::Text("name".to_string()),
                            Value::Text("Bob".to_string()),
                        ),
                        (Value::Text("age".to_string()), Value::Integer(25)),
                    ]),
                ]),
            ),
            (Value::Text("count".to_string()), Value::Integer(2)),
        ]);
        let bytes = to_canonical_cbor(&value).unwrap();
        let decoded: Value = from_cbor(&bytes).unwrap();
        assert_eq!(value, decoded);
    }
}
