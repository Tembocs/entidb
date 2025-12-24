//! Composite index keys for multi-field indexing.
//!
//! This module provides composite key types that allow indexing on
//! multiple fields simultaneously. Composite keys support:
//!
//! - Two-field composite keys (most common)
//! - Three-field composite keys
//! - Generic tuple-based keys
//!
//! ## Example
//!
//! ```rust,ignore
//! use entidb_core::index::{BTreeIndex, CompositeKey2, IndexSpec};
//!
//! // Index on (last_name, first_name)
//! let spec = IndexSpec::new(collection_id, "name_idx");
//! let mut index: BTreeIndex<CompositeKey2<String, String>> = BTreeIndex::new(spec);
//!
//! // Insert
//! let key = CompositeKey2::new("Smith".to_string(), "John".to_string());
//! index.insert(key, entity_id)?;
//!
//! // Range query on prefix (all "Smith" entries)
//! let prefix_start = CompositeKey2::new("Smith".to_string(), String::new());
//! let prefix_end = CompositeKey2::new("Smith".to_string() + "\u{FFFF}", String::new());
//! let results = index.range(prefix_start..prefix_end)?;
//! ```

use crate::error::{CoreError, CoreResult};
use crate::index::IndexKey;

/// Two-field composite key.
///
/// Composite keys are ordered lexicographically by (first, second).
/// This is useful for queries like "all users with last_name = 'Smith'".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompositeKey2<A: IndexKey, B: IndexKey> {
    /// First key component.
    pub first: A,
    /// Second key component.
    pub second: B,
}

impl<A: IndexKey, B: IndexKey> CompositeKey2<A, B> {
    /// Creates a new composite key.
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }

    /// Returns a reference to the first component.
    pub fn first(&self) -> &A {
        &self.first
    }

    /// Returns a reference to the second component.
    pub fn second(&self) -> &B {
        &self.second
    }
}

impl<A: IndexKey, B: IndexKey> PartialOrd for CompositeKey2<A, B> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<A: IndexKey, B: IndexKey> Ord for CompositeKey2<A, B> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.first.cmp(&other.first) {
            std::cmp::Ordering::Equal => self.second.cmp(&other.second),
            ord => ord,
        }
    }
}

impl<A: IndexKey, B: IndexKey> IndexKey for CompositeKey2<A, B> {
    fn to_bytes(&self) -> Vec<u8> {
        let a_bytes = self.first.to_bytes();
        let b_bytes = self.second.to_bytes();

        let mut result = Vec::with_capacity(8 + a_bytes.len() + b_bytes.len());

        // Length-prefix each component for unambiguous parsing
        result.extend_from_slice(&(a_bytes.len() as u32).to_be_bytes());
        result.extend_from_slice(&a_bytes);
        result.extend_from_slice(&(b_bytes.len() as u32).to_be_bytes());
        result.extend_from_slice(&b_bytes);

        result
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() < 4 {
            return Err(CoreError::InvalidFormat {
                message: "composite key too short".into(),
            });
        }

        let mut pos = 0;

        // Read first component
        let a_len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;

        if bytes.len() < pos + a_len + 4 {
            return Err(CoreError::InvalidFormat {
                message: "composite key truncated".into(),
            });
        }

        let first = A::from_bytes(&bytes[pos..pos + a_len])?;
        pos += a_len;

        // Read second component
        let b_len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;

        if bytes.len() < pos + b_len {
            return Err(CoreError::InvalidFormat {
                message: "composite key truncated".into(),
            });
        }

        let second = B::from_bytes(&bytes[pos..pos + b_len])?;

        Ok(Self { first, second })
    }
}

/// Three-field composite key.
///
/// Useful for triple-field indexes like (year, month, day) or
/// (category, subcategory, name).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompositeKey3<A: IndexKey, B: IndexKey, C: IndexKey> {
    /// First key component.
    pub first: A,
    /// Second key component.
    pub second: B,
    /// Third key component.
    pub third: C,
}

impl<A: IndexKey, B: IndexKey, C: IndexKey> CompositeKey3<A, B, C> {
    /// Creates a new composite key.
    pub fn new(first: A, second: B, third: C) -> Self {
        Self {
            first,
            second,
            third,
        }
    }
}

impl<A: IndexKey, B: IndexKey, C: IndexKey> PartialOrd for CompositeKey3<A, B, C> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<A: IndexKey, B: IndexKey, C: IndexKey> Ord for CompositeKey3<A, B, C> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.first.cmp(&other.first) {
            std::cmp::Ordering::Equal => match self.second.cmp(&other.second) {
                std::cmp::Ordering::Equal => self.third.cmp(&other.third),
                ord => ord,
            },
            ord => ord,
        }
    }
}

impl<A: IndexKey, B: IndexKey, C: IndexKey> IndexKey for CompositeKey3<A, B, C> {
    fn to_bytes(&self) -> Vec<u8> {
        let a_bytes = self.first.to_bytes();
        let b_bytes = self.second.to_bytes();
        let c_bytes = self.third.to_bytes();

        let mut result = Vec::with_capacity(12 + a_bytes.len() + b_bytes.len() + c_bytes.len());

        result.extend_from_slice(&(a_bytes.len() as u32).to_be_bytes());
        result.extend_from_slice(&a_bytes);
        result.extend_from_slice(&(b_bytes.len() as u32).to_be_bytes());
        result.extend_from_slice(&b_bytes);
        result.extend_from_slice(&(c_bytes.len() as u32).to_be_bytes());
        result.extend_from_slice(&c_bytes);

        result
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() < 4 {
            return Err(CoreError::InvalidFormat {
                message: "composite key too short".into(),
            });
        }

        let mut pos = 0;

        // Read first
        let a_len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;
        if bytes.len() < pos + a_len + 4 {
            return Err(CoreError::InvalidFormat {
                message: "composite key truncated".into(),
            });
        }
        let first = A::from_bytes(&bytes[pos..pos + a_len])?;
        pos += a_len;

        // Read second
        let b_len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;
        if bytes.len() < pos + b_len + 4 {
            return Err(CoreError::InvalidFormat {
                message: "composite key truncated".into(),
            });
        }
        let second = B::from_bytes(&bytes[pos..pos + b_len])?;
        pos += b_len;

        // Read third
        let c_len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;
        if bytes.len() < pos + c_len {
            return Err(CoreError::InvalidFormat {
                message: "composite key truncated".into(),
            });
        }
        let third = C::from_bytes(&bytes[pos..pos + c_len])?;

        Ok(Self {
            first,
            second,
            third,
        })
    }
}

// Implement IndexKey for tuples as a convenience

impl<A: IndexKey, B: IndexKey> IndexKey for (A, B) {
    fn to_bytes(&self) -> Vec<u8> {
        CompositeKey2::new(self.0.clone(), self.1.clone()).to_bytes()
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        let composite = CompositeKey2::<A, B>::from_bytes(bytes)?;
        Ok((composite.first, composite.second))
    }
}

impl<A: IndexKey, B: IndexKey, C: IndexKey> IndexKey for (A, B, C) {
    fn to_bytes(&self) -> Vec<u8> {
        CompositeKey3::new(self.0.clone(), self.1.clone(), self.2.clone()).to_bytes()
    }

    fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        let composite = CompositeKey3::<A, B, C>::from_bytes(bytes)?;
        Ok((composite.first, composite.second, composite.third))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite2_roundtrip() {
        let key = CompositeKey2::new("Smith".to_string(), 42i64);
        let bytes = key.to_bytes();
        let decoded = CompositeKey2::<String, i64>::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn composite2_ordering() {
        let a = CompositeKey2::new("Smith".to_string(), 1i64);
        let b = CompositeKey2::new("Smith".to_string(), 2i64);
        let c = CompositeKey2::new("Jones".to_string(), 100i64);

        assert!(a < b); // Same first, different second
        assert!(c < a); // Different first
    }

    #[test]
    fn composite3_roundtrip() {
        let key = CompositeKey3::new(2024i64, 3i64, 15i64);
        let bytes = key.to_bytes();
        let decoded = CompositeKey3::<i64, i64, i64>::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn tuple2_roundtrip() {
        let key = ("hello".to_string(), 42i64);
        let bytes = key.to_bytes();
        let decoded = <(String, i64)>::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn tuple3_roundtrip() {
        let key = (1i64, 2i64, 3i64);
        let bytes = key.to_bytes();
        let decoded = <(i64, i64, i64)>::from_bytes(&bytes).unwrap();
        assert_eq!(key, decoded);
    }

    #[test]
    fn composite2_with_btree_index() {
        use crate::index::{BTreeIndex, Index, IndexSpec};
        use crate::types::CollectionId;
        use crate::EntityId;

        let spec = IndexSpec::new(CollectionId::new(1), "name_idx");
        let mut index: BTreeIndex<CompositeKey2<String, String>> = BTreeIndex::new(spec);

        let e1 = EntityId::new();
        let e2 = EntityId::new();
        let e3 = EntityId::new();

        index
            .insert(CompositeKey2::new("Smith".to_string(), "John".to_string()), e1)
            .unwrap();
        index
            .insert(CompositeKey2::new("Smith".to_string(), "Jane".to_string()), e2)
            .unwrap();
        index
            .insert(CompositeKey2::new("Jones".to_string(), "Bob".to_string()), e3)
            .unwrap();

        // Lookup exact
        let result = index
            .lookup(&CompositeKey2::new("Smith".to_string(), "John".to_string()))
            .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&e1));

        // Range query for all "Smith"
        let start = CompositeKey2::new("Smith".to_string(), String::new());
        let end = CompositeKey2::new("Smith".to_string() + "\x7F", String::new());
        let range_result = index.range(start..end).unwrap();
        assert_eq!(range_result.len(), 2);
        assert!(range_result.contains(&e1));
        assert!(range_result.contains(&e2));
    }

    #[test]
    fn composite2_persistence() {
        use crate::index::{BTreeIndex, Index, IndexSpec};
        use crate::types::CollectionId;
        use crate::EntityId;

        let spec = IndexSpec::new(CollectionId::new(1), "test");
        let mut index: BTreeIndex<CompositeKey2<i64, String>> = BTreeIndex::new(spec);

        let e1 = EntityId::new();
        index
            .insert(CompositeKey2::new(2024, "event".to_string()), e1)
            .unwrap();

        // Serialize and deserialize
        let bytes = index.to_bytes();
        let loaded: BTreeIndex<CompositeKey2<i64, String>> =
            BTreeIndex::from_bytes(&bytes).unwrap();

        let result = loaded
            .lookup(&CompositeKey2::new(2024, "event".to_string()))
            .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&e1));
    }
}
