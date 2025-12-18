//! Canonical CBOR encoder.

use crate::error::CodecResult;
use crate::value::Value;

/// Encode a value to canonical CBOR bytes.
///
/// This function produces deterministic output following the canonical
/// CBOR rules specified in RFC 8949 Section 4.2.1:
/// - Map keys are sorted by their encoded form (length-first, then bytewise)
/// - Integers use the shortest possible encoding
/// - No indefinite-length encoding
///
/// # Errors
///
/// Returns an error if the value cannot be encoded (e.g., contains floats).
pub fn to_canonical_cbor(value: &Value) -> CodecResult<Vec<u8>> {
    let mut encoder = CanonicalEncoder::new();
    encoder.encode(value)?;
    Ok(encoder.into_bytes())
}

/// A canonical CBOR encoder.
///
/// This encoder produces deterministic CBOR output suitable for
/// hashing and storage in EntiDB.
pub struct CanonicalEncoder {
    buffer: Vec<u8>,
}

impl CanonicalEncoder {
    /// Create a new encoder.
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Create a new encoder with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Encode a value.
    pub fn encode(&mut self, value: &Value) -> CodecResult<()> {
        match value {
            Value::Null => {
                self.encode_null();
                Ok(())
            }
            Value::Bool(b) => {
                self.encode_bool(*b);
                Ok(())
            }
            Value::Integer(n) => {
                self.encode_integer(*n);
                Ok(())
            }
            Value::Bytes(b) => {
                self.encode_bytes(b);
                Ok(())
            }
            Value::Text(s) => {
                self.encode_text(s);
                Ok(())
            }
            Value::Array(arr) => self.encode_array(arr),
            Value::Map(pairs) => self.encode_map(pairs),
        }
    }

    /// Consume this encoder and return the encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// Get a reference to the encoded bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }

    fn encode_null(&mut self) {
        // CBOR null is simple value 22 (0xf6)
        self.buffer.push(0xf6);
    }

    fn encode_bool(&mut self, b: bool) {
        // CBOR false is 0xf4, true is 0xf5
        self.buffer.push(if b { 0xf5 } else { 0xf4 });
    }

    #[allow(clippy::cast_sign_loss)]
    fn encode_integer(&mut self, n: i64) {
        if n >= 0 {
            self.encode_unsigned(0, n as u64);
        } else {
            // CBOR negative integers encode -(n+1)
            // So -1 encodes as 0, -2 encodes as 1, etc.
            // This is safe: for n in [-2^63, -1], -(n+1) is in [0, 2^63-1]
            let abs_minus_one = (-(n + 1)) as u64;
            self.encode_unsigned(1, abs_minus_one);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn encode_unsigned(&mut self, major_type: u8, value: u64) {
        let mt = major_type << 5;

        if value < 24 {
            self.buffer.push(mt | (value as u8));
        } else if u8::try_from(value).is_ok() {
            self.buffer.push(mt | 24);
            self.buffer.push(value as u8);
        } else if u16::try_from(value).is_ok() {
            self.buffer.push(mt | 25);
            self.buffer.extend_from_slice(&(value as u16).to_be_bytes());
        } else if u32::try_from(value).is_ok() {
            self.buffer.push(mt | 26);
            self.buffer.extend_from_slice(&(value as u32).to_be_bytes());
        } else {
            self.buffer.push(mt | 27);
            self.buffer.extend_from_slice(&value.to_be_bytes());
        }
    }

    fn encode_bytes(&mut self, bytes: &[u8]) {
        self.encode_unsigned(2, bytes.len() as u64);
        self.buffer.extend_from_slice(bytes);
    }

    fn encode_text(&mut self, text: &str) {
        self.encode_unsigned(3, text.len() as u64);
        self.buffer.extend_from_slice(text.as_bytes());
    }

    fn encode_array(&mut self, arr: &[Value]) -> CodecResult<()> {
        self.encode_unsigned(4, arr.len() as u64);
        for item in arr {
            self.encode(item)?;
        }
        Ok(())
    }

    fn encode_map(&mut self, pairs: &[(Value, Value)]) -> CodecResult<()> {
        // First, encode all keys to get their canonical byte representation
        let mut encoded_pairs: Vec<(Vec<u8>, &Value, &Value)> = Vec::with_capacity(pairs.len());

        for (key, value) in pairs {
            let mut key_encoder = CanonicalEncoder::new();
            key_encoder.encode(key)?;
            encoded_pairs.push((key_encoder.into_bytes(), key, value));
        }

        // Sort by encoded key (length-first, then bytewise)
        encoded_pairs.sort_by(|a, b| match a.0.len().cmp(&b.0.len()) {
            std::cmp::Ordering::Equal => a.0.cmp(&b.0),
            other => other,
        });

        // Encode the map
        self.encode_unsigned(5, pairs.len() as u64);
        for (encoded_key, _, value) in encoded_pairs {
            self.buffer.extend_from_slice(&encoded_key);
            self.encode(value)?;
        }

        Ok(())
    }
}

impl Default for CanonicalEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_null() {
        let bytes = to_canonical_cbor(&Value::Null).unwrap();
        assert_eq!(bytes, vec![0xf6]);
    }

    #[test]
    fn encode_bool() {
        assert_eq!(to_canonical_cbor(&Value::Bool(false)).unwrap(), vec![0xf4]);
        assert_eq!(to_canonical_cbor(&Value::Bool(true)).unwrap(), vec![0xf5]);
    }

    #[test]
    fn encode_small_positive_integers() {
        // 0-23 encode in one byte
        assert_eq!(to_canonical_cbor(&Value::Integer(0)).unwrap(), vec![0x00]);
        assert_eq!(to_canonical_cbor(&Value::Integer(1)).unwrap(), vec![0x01]);
        assert_eq!(to_canonical_cbor(&Value::Integer(23)).unwrap(), vec![0x17]);
    }

    #[test]
    fn encode_one_byte_integers() {
        // 24-255 encode with additional byte
        assert_eq!(
            to_canonical_cbor(&Value::Integer(24)).unwrap(),
            vec![0x18, 24]
        );
        assert_eq!(
            to_canonical_cbor(&Value::Integer(255)).unwrap(),
            vec![0x18, 255]
        );
    }

    #[test]
    fn encode_two_byte_integers() {
        // 256-65535 encode with two additional bytes
        assert_eq!(
            to_canonical_cbor(&Value::Integer(256)).unwrap(),
            vec![0x19, 0x01, 0x00]
        );
        assert_eq!(
            to_canonical_cbor(&Value::Integer(65535)).unwrap(),
            vec![0x19, 0xff, 0xff]
        );
    }

    #[test]
    fn encode_four_byte_integers() {
        assert_eq!(
            to_canonical_cbor(&Value::Integer(65536)).unwrap(),
            vec![0x1a, 0x00, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn encode_negative_integers() {
        // -1 encodes as 0x20 (major type 1, value 0)
        assert_eq!(to_canonical_cbor(&Value::Integer(-1)).unwrap(), vec![0x20]);
        // -24 encodes as 0x37 (major type 1, value 23)
        assert_eq!(to_canonical_cbor(&Value::Integer(-24)).unwrap(), vec![0x37]);
        // -25 encodes as 0x38, 24
        assert_eq!(
            to_canonical_cbor(&Value::Integer(-25)).unwrap(),
            vec![0x38, 24]
        );
        // -100 encodes as 0x38, 99
        assert_eq!(
            to_canonical_cbor(&Value::Integer(-100)).unwrap(),
            vec![0x38, 99]
        );
    }

    #[test]
    fn encode_bytes() {
        assert_eq!(
            to_canonical_cbor(&Value::Bytes(vec![])).unwrap(),
            vec![0x40]
        );
        assert_eq!(
            to_canonical_cbor(&Value::Bytes(vec![1, 2, 3])).unwrap(),
            vec![0x43, 1, 2, 3]
        );
    }

    #[test]
    fn encode_text() {
        assert_eq!(
            to_canonical_cbor(&Value::Text(String::new())).unwrap(),
            vec![0x60]
        );
        assert_eq!(
            to_canonical_cbor(&Value::Text("a".to_string())).unwrap(),
            vec![0x61, b'a']
        );
        assert_eq!(
            to_canonical_cbor(&Value::Text("hello".to_string())).unwrap(),
            vec![0x65, b'h', b'e', b'l', b'l', b'o']
        );
    }

    #[test]
    fn encode_array() {
        assert_eq!(
            to_canonical_cbor(&Value::Array(vec![])).unwrap(),
            vec![0x80]
        );
        assert_eq!(
            to_canonical_cbor(&Value::Array(vec![Value::Integer(1), Value::Integer(2)])).unwrap(),
            vec![0x82, 0x01, 0x02]
        );
    }

    #[test]
    fn encode_map_sorted() {
        // Keys should be sorted: length first, then bytewise
        let map = Value::Map(vec![
            (Value::Text("bb".to_string()), Value::Integer(2)),
            (Value::Text("a".to_string()), Value::Integer(1)),
        ]);
        let bytes = to_canonical_cbor(&map).unwrap();

        // Expected: map(2), "a"(61 61), 1, "bb"(62 62 62), 2
        assert_eq!(bytes, vec![0xa2, 0x61, b'a', 0x01, 0x62, b'b', b'b', 0x02]);
    }

    #[test]
    fn encode_map_with_integer_keys() {
        // Integer keys should sort before text keys (lower major type)
        let map = Value::Map(vec![
            (Value::Text("a".to_string()), Value::Integer(2)),
            (Value::Integer(1), Value::Integer(1)),
        ]);
        let bytes = to_canonical_cbor(&map).unwrap();

        // Expected: map(2), 1, 1, "a", 2
        // Integer 1 encodes as 0x01 (1 byte), "a" encodes as 0x61 0x61 (2 bytes)
        // Shorter encoding comes first
        assert_eq!(bytes, vec![0xa2, 0x01, 0x01, 0x61, b'a', 0x02]);
    }

    #[test]
    fn deterministic_encoding() {
        // Same logical map with different insertion orders should produce same bytes
        let map1 = Value::Map(vec![
            (Value::Text("z".to_string()), Value::Integer(1)),
            (Value::Text("a".to_string()), Value::Integer(2)),
        ]);
        let map2 = Value::Map(vec![
            (Value::Text("a".to_string()), Value::Integer(2)),
            (Value::Text("z".to_string()), Value::Integer(1)),
        ]);

        let bytes1 = to_canonical_cbor(&map1).unwrap();
        let bytes2 = to_canonical_cbor(&map2).unwrap();

        assert_eq!(bytes1, bytes2);
    }
}
