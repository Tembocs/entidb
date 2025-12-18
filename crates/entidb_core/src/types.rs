//! Core type definitions for EntiDB.

use std::fmt;

/// Unique identifier for a transaction.
///
/// Transaction IDs are monotonically increasing and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransactionId(pub u64);

impl TransactionId {
    /// Creates a new transaction ID.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "txn:{}", self.0)
    }
}

/// Sequence number for ordering commits.
///
/// Sequence numbers provide total ordering of committed transactions.
/// Higher sequence numbers indicate later commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SequenceNumber(pub u64);

impl SequenceNumber {
    /// Creates a new sequence number.
    #[must_use]
    pub const fn new(seq: u64) -> Self {
        Self(seq)
    }

    /// Returns the raw sequence value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns the next sequence number.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for SequenceNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "seq:{}", self.0)
    }
}

/// Identifier for a collection (entity type container).
///
/// Collection IDs are stable and assigned when collections are created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollectionId(pub u32);

impl CollectionId {
    /// Creates a new collection ID.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for CollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "col:{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_id_ordering() {
        let t1 = TransactionId::new(1);
        let t2 = TransactionId::new(2);
        assert!(t1 < t2);
    }

    #[test]
    fn sequence_number_next() {
        let s1 = SequenceNumber::new(5);
        let s2 = s1.next();
        assert_eq!(s2.as_u64(), 6);
    }

    #[test]
    fn collection_id_display() {
        let c = CollectionId::new(42);
        assert_eq!(format!("{c}"), "col:42");
    }
}
