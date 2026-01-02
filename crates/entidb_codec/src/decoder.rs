//! Canonical CBOR decoder.

use crate::error::{CodecError, CodecResult};
use crate::value::Value;

/// Decode a value from CBOR bytes.
///
/// # Errors
///
/// Returns an error if the bytes are not valid CBOR or contain
/// forbidden constructs (floats, NaN, indefinite-length).
pub fn from_cbor(bytes: &[u8]) -> CodecResult<Value> {
    let mut decoder = CanonicalDecoder::new(bytes);
    decoder.decode()
}

/// A canonical CBOR decoder.
///
/// This decoder validates that input follows canonical CBOR rules
/// and rejects forbidden constructs.
pub struct CanonicalDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

/// Maximum allowed element count for arrays and maps.
/// This prevents allocation-based DoS from untrusted input.
/// 16 million elements is generous for legitimate use cases.
const MAX_CONTAINER_ELEMENTS: u64 = 16 * 1024 * 1024;

/// Maximum allowed byte/string length.
/// This prevents allocation-based DoS from untrusted input.
/// 256 MB should cover any legitimate payload.
const MAX_BYTES_LENGTH: u64 = 256 * 1024 * 1024;

impl<'a> CanonicalDecoder<'a> {
    /// Create a new decoder for the given bytes.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Decode the next value.
    #[allow(clippy::cast_possible_wrap)]
    pub fn decode(&mut self) -> CodecResult<Value> {
        let initial_byte = self.read_byte()?;
        let major_type = initial_byte >> 5;
        let additional_info = initial_byte & 0x1f;

        match major_type {
            0 => self
                .decode_unsigned(additional_info)
                .map(|n| Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))),
            1 => self.decode_unsigned(additional_info).map(|n| {
                // Negative integer: value is -(n+1)
                // Safe cast: we check the range first
                if i64::try_from(n).is_ok() {
                    Value::Integer(-(n as i64) - 1)
                } else {
                    // Handle overflow for very large negative numbers
                    Value::Integer(i64::MIN)
                }
            }),
            2 => self.decode_bytes(additional_info),
            3 => self.decode_text(additional_info),
            4 => self.decode_array(additional_info),
            5 => self.decode_map(additional_info),
            6 => {
                // Tagged value - skip the tag and decode the value
                let _tag = self.decode_unsigned(additional_info)?;
                self.decode()
            }
            7 => self.decode_simple(additional_info),
            _ => Err(CodecError::invalid_structure("invalid major type")),
        }
    }

    /// Check if all bytes have been consumed.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Get remaining bytes.
    pub fn remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }

    #[inline]
    fn read_byte(&mut self) -> CodecResult<u8> {
        if self.pos >= self.data.len() {
            return Err(CodecError::UnexpectedEof);
        }
        let byte = self.data[self.pos];
        self.pos += 1;
        Ok(byte)
    }

    #[inline]
    fn read_bytes(&mut self, len: usize) -> CodecResult<&'a [u8]> {
        if self.pos + len > self.data.len() {
            return Err(CodecError::UnexpectedEof);
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    #[inline]
    fn decode_unsigned(&mut self, additional_info: u8) -> CodecResult<u64> {
        match additional_info {
            0..=23 => Ok(u64::from(additional_info)),
            24 => {
                let byte = self.read_byte()?;
                // Validate shortest encoding
                if byte < 24 {
                    return Err(CodecError::invalid_structure(
                        "non-canonical: value could be encoded in fewer bytes",
                    ));
                }
                Ok(u64::from(byte))
            }
            25 => {
                let bytes = self.read_bytes(2)?;
                let value = u16::from_be_bytes([bytes[0], bytes[1]]);
                // Validate shortest encoding
                if u8::try_from(value).is_ok() {
                    return Err(CodecError::invalid_structure(
                        "non-canonical: value could be encoded in fewer bytes",
                    ));
                }
                Ok(u64::from(value))
            }
            26 => {
                let bytes = self.read_bytes(4)?;
                let value = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                // Validate shortest encoding
                if u16::try_from(value).is_ok() {
                    return Err(CodecError::invalid_structure(
                        "non-canonical: value could be encoded in fewer bytes",
                    ));
                }
                Ok(u64::from(value))
            }
            27 => {
                let bytes = self.read_bytes(8)?;
                let value = u64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                // Validate shortest encoding
                if u32::try_from(value).is_ok() {
                    return Err(CodecError::invalid_structure(
                        "non-canonical: value could be encoded in fewer bytes",
                    ));
                }
                Ok(value)
            }
            28..=30 => Err(CodecError::invalid_structure("reserved additional info")),
            31 => Err(CodecError::IndefiniteLengthForbidden),
            _ => unreachable!(),
        }
    }

    fn decode_bytes(&mut self, additional_info: u8) -> CodecResult<Value> {
        if additional_info == 31 {
            return Err(CodecError::IndefiniteLengthForbidden);
        }
        let len_u64 = self.decode_unsigned(additional_info)?;
        if len_u64 > MAX_BYTES_LENGTH {
            return Err(CodecError::SizeLimitExceeded {
                claimed: len_u64,
                max_allowed: MAX_BYTES_LENGTH,
            });
        }
        let len = len_u64 as usize;
        let bytes = self.read_bytes(len)?;
        Ok(Value::Bytes(bytes.to_vec()))
    }

    fn decode_text(&mut self, additional_info: u8) -> CodecResult<Value> {
        if additional_info == 31 {
            return Err(CodecError::IndefiniteLengthForbidden);
        }
        let len_u64 = self.decode_unsigned(additional_info)?;
        if len_u64 > MAX_BYTES_LENGTH {
            return Err(CodecError::SizeLimitExceeded {
                claimed: len_u64,
                max_allowed: MAX_BYTES_LENGTH,
            });
        }
        let len = len_u64 as usize;
        let bytes = self.read_bytes(len)?;
        let text = std::str::from_utf8(bytes).map_err(|_| CodecError::InvalidUtf8)?;
        Ok(Value::Text(text.to_string()))
    }

    fn decode_array(&mut self, additional_info: u8) -> CodecResult<Value> {
        if additional_info == 31 {
            return Err(CodecError::IndefiniteLengthForbidden);
        }
        let len_u64 = self.decode_unsigned(additional_info)?;
        if len_u64 > MAX_CONTAINER_ELEMENTS {
            return Err(CodecError::SizeLimitExceeded {
                claimed: len_u64,
                max_allowed: MAX_CONTAINER_ELEMENTS,
            });
        }
        let len = len_u64 as usize;
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(self.decode()?);
        }
        Ok(Value::Array(items))
    }

    fn decode_map(&mut self, additional_info: u8) -> CodecResult<Value> {
        if additional_info == 31 {
            return Err(CodecError::IndefiniteLengthForbidden);
        }
        let len_u64 = self.decode_unsigned(additional_info)?;
        if len_u64 > MAX_CONTAINER_ELEMENTS {
            return Err(CodecError::SizeLimitExceeded {
                claimed: len_u64,
                max_allowed: MAX_CONTAINER_ELEMENTS,
            });
        }
        let len = len_u64 as usize;
        let mut pairs = Vec::with_capacity(len);

        let mut prev_key_bytes: Option<Vec<u8>> = None;

        for _ in 0..len {
            // Remember position before decoding key
            let key_start = self.pos;
            let key = self.decode()?;
            let key_end = self.pos;
            let key_bytes = self.data[key_start..key_end].to_vec();

            // Validate key ordering (must be strictly increasing)
            if let Some(ref prev) = prev_key_bytes {
                let ordering = compare_cbor_bytes(prev, &key_bytes);
                if ordering != std::cmp::Ordering::Less {
                    return Err(CodecError::invalid_structure(
                        "non-canonical: map keys not in sorted order",
                    ));
                }
            }
            prev_key_bytes = Some(key_bytes);

            let value = self.decode()?;
            pairs.push((key, value));
        }

        Ok(Value::Map(pairs))
    }

    fn decode_simple(&mut self, additional_info: u8) -> CodecResult<Value> {
        match additional_info {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 => Ok(Value::Null),
            23 => {
                // undefined - treat as null
                Ok(Value::Null)
            }
            24 => {
                let simple = self.read_byte()?;
                match simple {
                    0..=31 => Err(CodecError::invalid_structure(
                        "non-canonical: simple value should use direct encoding",
                    )),
                    _ => Err(CodecError::unsupported_type(format!(
                        "simple value {simple}"
                    ))),
                }
            }
            25..=27 => {
                // Half, single, or double-precision float
                Err(CodecError::FloatForbidden)
            }
            28..=30 => Err(CodecError::invalid_structure("reserved additional info")),
            31 => Err(CodecError::invalid_structure("break without indefinite")),
            _ => Err(CodecError::unsupported_type(format!(
                "simple value {additional_info}"
            ))),
        }
    }
}

/// Compare two CBOR byte sequences for canonical ordering.
/// Uses length-first, then bytewise comparison.
fn compare_cbor_bytes(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => a.cmp(b),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_null() {
        let value = from_cbor(&[0xf6]).unwrap();
        assert_eq!(value, Value::Null);
    }

    #[test]
    fn decode_bool() {
        assert_eq!(from_cbor(&[0xf4]).unwrap(), Value::Bool(false));
        assert_eq!(from_cbor(&[0xf5]).unwrap(), Value::Bool(true));
    }

    #[test]
    fn decode_small_positive_integers() {
        assert_eq!(from_cbor(&[0x00]).unwrap(), Value::Integer(0));
        assert_eq!(from_cbor(&[0x01]).unwrap(), Value::Integer(1));
        assert_eq!(from_cbor(&[0x17]).unwrap(), Value::Integer(23));
    }

    #[test]
    fn decode_one_byte_integers() {
        assert_eq!(from_cbor(&[0x18, 24]).unwrap(), Value::Integer(24));
        assert_eq!(from_cbor(&[0x18, 255]).unwrap(), Value::Integer(255));
    }

    #[test]
    fn decode_two_byte_integers() {
        assert_eq!(from_cbor(&[0x19, 0x01, 0x00]).unwrap(), Value::Integer(256));
        assert_eq!(
            from_cbor(&[0x19, 0xff, 0xff]).unwrap(),
            Value::Integer(65535)
        );
    }

    #[test]
    fn decode_negative_integers() {
        assert_eq!(from_cbor(&[0x20]).unwrap(), Value::Integer(-1));
        assert_eq!(from_cbor(&[0x37]).unwrap(), Value::Integer(-24));
        assert_eq!(from_cbor(&[0x38, 24]).unwrap(), Value::Integer(-25));
        assert_eq!(from_cbor(&[0x38, 99]).unwrap(), Value::Integer(-100));
    }

    #[test]
    fn decode_bytes() {
        assert_eq!(from_cbor(&[0x40]).unwrap(), Value::Bytes(vec![]));
        assert_eq!(
            from_cbor(&[0x43, 1, 2, 3]).unwrap(),
            Value::Bytes(vec![1, 2, 3])
        );
    }

    #[test]
    fn decode_text() {
        assert_eq!(from_cbor(&[0x60]).unwrap(), Value::Text(String::new()));
        assert_eq!(
            from_cbor(&[0x61, b'a']).unwrap(),
            Value::Text("a".to_string())
        );
        assert_eq!(
            from_cbor(&[0x65, b'h', b'e', b'l', b'l', b'o']).unwrap(),
            Value::Text("hello".to_string())
        );
    }

    #[test]
    fn decode_array() {
        assert_eq!(from_cbor(&[0x80]).unwrap(), Value::Array(vec![]));
        assert_eq!(
            from_cbor(&[0x82, 0x01, 0x02]).unwrap(),
            Value::Array(vec![Value::Integer(1), Value::Integer(2)])
        );
    }

    #[test]
    fn decode_map() {
        assert_eq!(from_cbor(&[0xa0]).unwrap(), Value::Map(vec![]));
        // Map with keys "a" -> 1
        assert_eq!(
            from_cbor(&[0xa1, 0x61, b'a', 0x01]).unwrap(),
            Value::Map(vec![(Value::Text("a".to_string()), Value::Integer(1))])
        );
    }

    #[test]
    fn reject_float() {
        // Half float
        assert!(matches!(
            from_cbor(&[0xf9, 0x00, 0x00]),
            Err(CodecError::FloatForbidden)
        ));
        // Single float
        assert!(matches!(
            from_cbor(&[0xfa, 0x00, 0x00, 0x00, 0x00]),
            Err(CodecError::FloatForbidden)
        ));
        // Double float
        assert!(matches!(
            from_cbor(&[0xfb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            Err(CodecError::FloatForbidden)
        ));
    }

    #[test]
    fn reject_indefinite_length() {
        // Indefinite byte string
        assert!(matches!(
            from_cbor(&[0x5f, 0x41, b'a', 0xff]),
            Err(CodecError::IndefiniteLengthForbidden)
        ));
        // Indefinite text string
        assert!(matches!(
            from_cbor(&[0x7f, 0x61, b'a', 0xff]),
            Err(CodecError::IndefiniteLengthForbidden)
        ));
        // Indefinite array
        assert!(matches!(
            from_cbor(&[0x9f, 0x01, 0xff]),
            Err(CodecError::IndefiniteLengthForbidden)
        ));
        // Indefinite map
        assert!(matches!(
            from_cbor(&[0xbf, 0x61, b'a', 0x01, 0xff]),
            Err(CodecError::IndefiniteLengthForbidden)
        ));
    }

    #[test]
    fn reject_non_shortest_encoding() {
        // 23 encoded with extra byte (should be 0x17)
        assert!(matches!(
            from_cbor(&[0x18, 23]),
            Err(CodecError::InvalidStructure { .. })
        ));
        // 255 encoded with two bytes (should be 0x18, 0xff)
        assert!(matches!(
            from_cbor(&[0x19, 0x00, 0xff]),
            Err(CodecError::InvalidStructure { .. })
        ));
    }

    #[test]
    fn reject_unsorted_map_keys() {
        // Map with keys "b", "a" (wrong order - should be "a", "b")
        assert!(matches!(
            from_cbor(&[0xa2, 0x61, b'b', 0x01, 0x61, b'a', 0x02]),
            Err(CodecError::InvalidStructure { .. })
        ));
    }

    #[test]
    fn unexpected_eof() {
        assert!(matches!(from_cbor(&[]), Err(CodecError::UnexpectedEof)));
        assert!(matches!(from_cbor(&[0x18]), Err(CodecError::UnexpectedEof)));
        assert!(matches!(
            from_cbor(&[0x19, 0x01]),
            Err(CodecError::UnexpectedEof)
        ));
    }

    #[test]
    fn invalid_utf8_rejected() {
        // Text string with invalid UTF-8
        assert!(matches!(
            from_cbor(&[0x62, 0xff, 0xfe]),
            Err(CodecError::InvalidUtf8)
        ));
    }
}
