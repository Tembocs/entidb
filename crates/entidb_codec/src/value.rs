//! Dynamic CBOR value type.

use std::cmp::Ordering;

/// A dynamic CBOR value.
///
/// This type represents any valid CBOR value that EntiDB supports.
/// Note that floats are intentionally not supported per the canonical
/// CBOR specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Signed integer (supports full i64 range).
    Integer(i64),
    /// Byte string.
    Bytes(Vec<u8>),
    /// Text string (UTF-8).
    Text(String),
    /// Array of values.
    Array(Vec<Value>),
    /// Map of key-value pairs (keys are sorted for canonical encoding).
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// Create a map value with sorted keys.
    ///
    /// Keys are sorted by their canonical CBOR encoding (bytewise comparison).
    pub fn map(mut pairs: Vec<(Value, Value)>) -> Self {
        pairs.sort_by(|a, b| a.0.cmp_canonical(&b.0));
        Value::Map(pairs)
    }

    /// Compare two values for canonical ordering.
    ///
    /// This implements the bytewise comparison of canonical CBOR encodings,
    /// which is required for map key sorting.
    #[allow(clippy::match_same_arms)]
    pub fn cmp_canonical(&self, other: &Self) -> Ordering {
        // First compare by major type
        let self_type = self.major_type();
        let other_type = other.major_type();

        if self_type != other_type {
            return self_type.cmp(&other_type);
        }

        // Same major type, compare by content
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Integer(a), Value::Integer(b)) => {
                // For integers, we need to compare by their canonical CBOR encoded form.
                // In canonical CBOR, ordering is length-first, then lexicographic.
                //
                // For positive integers (major type 0): argument is the value itself.
                // For negative integers (major type 1): argument is -1 - n (e.g., -1 -> 0, -2 -> 1).
                //
                // Since positive (type 0) and negative (type 1) have different major types,
                // they are already separated by the major_type() comparison above.
                // Here we only compare within the same sign.
                #[allow(clippy::cast_sign_loss)]
                match (a.signum(), b.signum()) {
                    (s1, s2) if s1 >= 0 && s2 >= 0 => {
                        // Both non-negative: compare by encoded length, then value
                        // Safety: signum() >= 0 guarantees non-negative value
                        Self::cmp_unsigned_canonical(*a as u64, *b as u64)
                    }
                    (s1, s2) if s1 < 0 && s2 < 0 => {
                        // Both negative: CBOR encodes -1-n as argument
                        // -1 -> arg 0, -2 -> arg 1, -10 -> arg 9, etc.
                        // Compare by encoded argument length, then lexicographic (ascending argument)
                        // Safety: for negative a, (-1 - a) is always non-negative
                        let arg_a = (-1 - *a) as u64;
                        let arg_b = (-1 - *b) as u64;
                        Self::cmp_unsigned_canonical(arg_a, arg_b)
                    }
                    (s1, _) if s1 >= 0 => Ordering::Less, // positive (type 0) before negative (type 1)
                    _ => Ordering::Greater,
                }
            }
            (Value::Bytes(a), Value::Bytes(b)) => {
                // Length-first, then lexicographic
                match a.len().cmp(&b.len()) {
                    Ordering::Equal => a.cmp(b),
                    ord => ord,
                }
            }
            (Value::Text(a), Value::Text(b)) => {
                // Length-first (by UTF-8 bytes), then lexicographic
                match a.len().cmp(&b.len()) {
                    Ordering::Equal => a.cmp(b),
                    ord => ord,
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                // Length-first, then element-by-element
                match a.len().cmp(&b.len()) {
                    Ordering::Equal => {
                        for (av, bv) in a.iter().zip(b.iter()) {
                            let ord = av.cmp_canonical(bv);
                            if ord != Ordering::Equal {
                                return ord;
                            }
                        }
                        Ordering::Equal
                    }
                    ord => ord,
                }
            }
            (Value::Map(a), Value::Map(b)) => {
                // Length-first, then entry-by-entry
                match a.len().cmp(&b.len()) {
                    Ordering::Equal => {
                        for ((ak, av), (bk, bv)) in a.iter().zip(b.iter()) {
                            let key_ord = ak.cmp_canonical(bk);
                            if key_ord != Ordering::Equal {
                                return key_ord;
                            }
                            let val_ord = av.cmp_canonical(bv);
                            if val_ord != Ordering::Equal {
                                return val_ord;
                            }
                        }
                        Ordering::Equal
                    }
                    ord => ord,
                }
            }
            _ => Ordering::Equal, // Should not happen with same major type
        }
    }

    /// Compare two unsigned integers by their canonical CBOR encoding.
    ///
    /// Canonical CBOR uses shortest encoding, so values 0-23 encode in 1 byte,
    /// 24-255 in 2 bytes, 256-65535 in 3 bytes, etc.
    /// Comparison is length-first, then lexicographic (which equals numeric for same length).
    fn cmp_unsigned_canonical(a: u64, b: u64) -> Ordering {
        // Determine encoding length for each value
        let len_a = Self::cbor_uint_encoded_len(a);
        let len_b = Self::cbor_uint_encoded_len(b);

        match len_a.cmp(&len_b) {
            Ordering::Equal => a.cmp(&b), // Same length, compare numerically (= lexicographic for big-endian)
            ord => ord,
        }
    }

    /// Returns the encoded length (in bytes) of an unsigned integer in CBOR.
    fn cbor_uint_encoded_len(n: u64) -> usize {
        if n <= 23 {
            1 // Argument fits in initial byte
        } else if n <= 0xFF {
            2 // 1 byte header + 1 byte argument
        } else if n <= 0xFFFF {
            3 // 1 byte header + 2 bytes argument
        } else if n <= 0xFFFF_FFFF {
            5 // 1 byte header + 4 bytes argument
        } else {
            9 // 1 byte header + 8 bytes argument
        }
    }

    /// Get the CBOR major type for this value.
    fn major_type(&self) -> u8 {
        match self {
            Value::Integer(n) if *n >= 0 => 0, // Unsigned integer
            Value::Integer(_) => 1,            // Negative integer
            Value::Bytes(_) => 2,              // Byte string
            Value::Text(_) => 3,               // Text string
            Value::Array(_) => 4,              // Array
            Value::Map(_) => 5,                // Map
            Value::Bool(_) | Value::Null => 7, // Simple values
        }
    }

    /// Check if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Get this value as a boolean, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get this value as an integer, if it is one.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }

    /// Get this value as bytes, if it is a byte string.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Get this value as a string, if it is a text string.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get this value as an array, if it is one.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Get this value as a map, if it is one.
    pub fn as_map(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Look up a key in this map value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(pairs) => {
                let key_value = Value::Text(key.to_string());
                pairs.iter().find(|(k, _)| k == &key_value).map(|(_, v)| v)
            }
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Integer(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Integer(i64::from(n))
    }
}

impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Value::Integer(i64::from(n))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}

impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Self {
        Value::Bytes(b)
    }
}

impl From<&[u8]> for Value {
    fn from(b: &[u8]) -> Self {
        Value::Bytes(b.to_vec())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::Array(v.into_iter().map(Into::into).collect())
    }
}

impl From<()> for Value {
    fn from((): ()) -> Self {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_keys_are_sorted() {
        let map = Value::map(vec![
            (Value::Text("z".to_string()), Value::Integer(1)),
            (Value::Text("a".to_string()), Value::Integer(2)),
            (Value::Text("m".to_string()), Value::Integer(3)),
        ]);

        if let Value::Map(pairs) = map {
            assert_eq!(pairs[0].0, Value::Text("a".to_string()));
            assert_eq!(pairs[1].0, Value::Text("m".to_string()));
            assert_eq!(pairs[2].0, Value::Text("z".to_string()));
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn map_key_length_ordering() {
        // Shorter keys come first in canonical CBOR
        let map = Value::map(vec![
            (Value::Text("abc".to_string()), Value::Integer(1)),
            (Value::Text("a".to_string()), Value::Integer(2)),
            (Value::Text("ab".to_string()), Value::Integer(3)),
        ]);

        if let Value::Map(pairs) = map {
            assert_eq!(pairs[0].0, Value::Text("a".to_string()));
            assert_eq!(pairs[1].0, Value::Text("ab".to_string()));
            assert_eq!(pairs[2].0, Value::Text("abc".to_string()));
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn integer_ordering() {
        // Positive before negative, then by value
        let values = vec![
            Value::Integer(-1),
            Value::Integer(0),
            Value::Integer(1),
            Value::Integer(-2),
            Value::Integer(2),
        ];

        let mut sorted = values.clone();
        sorted.sort_by(Value::cmp_canonical);

        assert_eq!(sorted[0], Value::Integer(0));
        assert_eq!(sorted[1], Value::Integer(1));
        assert_eq!(sorted[2], Value::Integer(2));
        assert_eq!(sorted[3], Value::Integer(-1));
        assert_eq!(sorted[4], Value::Integer(-2));
    }

    #[test]
    fn value_accessors() {
        assert!(Value::Null.is_null());
        assert!(!Value::Bool(true).is_null());

        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Integer(42).as_bool(), None);

        assert_eq!(Value::Integer(42).as_integer(), Some(42));
        assert_eq!(Value::Text("42".to_string()).as_integer(), None);

        assert_eq!(Value::Text("hello".to_string()).as_text(), Some("hello"));
        assert_eq!(Value::Bytes(vec![1, 2, 3]).as_bytes(), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn map_get() {
        let map = Value::map(vec![
            (
                Value::Text("name".to_string()),
                Value::Text("Alice".to_string()),
            ),
            (Value::Text("age".to_string()), Value::Integer(30)),
        ]);

        assert_eq!(map.get("name"), Some(&Value::Text("Alice".to_string())));
        assert_eq!(map.get("age"), Some(&Value::Integer(30)));
        assert_eq!(map.get("missing"), None);
    }

    #[test]
    fn from_impls() {
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from(42i64), Value::Integer(42));
        assert_eq!(Value::from(42i32), Value::Integer(42));
        assert_eq!(Value::from(42u32), Value::Integer(42));
        assert_eq!(Value::from("hello"), Value::Text("hello".to_string()));
        assert_eq!(
            Value::from("hello".to_string()),
            Value::Text("hello".to_string())
        );
        assert_eq!(Value::from(vec![1u8, 2, 3]), Value::Bytes(vec![1, 2, 3]));
        assert_eq!(Value::from(()), Value::Null);
    }
}
