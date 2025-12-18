//! Entity identifier.

use std::fmt;
use uuid::Uuid;

/// Unique identifier for an entity.
///
/// Entity IDs are 128-bit UUIDs that are:
/// - Globally unique within a database
/// - Immutable once assigned
/// - Never reused
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId([u8; 16]);

impl EntityId {
    /// Creates an entity ID from raw bytes.
    #[inline]
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Creates a new random entity ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().into_bytes())
    }

    /// Creates an entity ID from a UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid.into_bytes())
    }

    /// Returns the raw bytes.
    #[inline]
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Converts to a UUID.
    #[must_use]
    pub fn to_uuid(&self) -> Uuid {
        Uuid::from_bytes(self.0)
    }

    /// Creates an entity ID from a slice.
    ///
    /// Returns `None` if the slice is not exactly 16 bytes.
    #[must_use]
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        if slice.len() == 16 {
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(slice);
            Some(Self(bytes))
        } else {
            None
        }
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityId({})", self.to_uuid())
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uuid())
    }
}

impl From<Uuid> for EntityId {
    fn from(uuid: Uuid) -> Self {
        Self::from_uuid(uuid)
    }
}

impl From<EntityId> for Uuid {
    fn from(id: EntityId) -> Self {
        id.to_uuid()
    }
}

impl From<[u8; 16]> for EntityId {
    fn from(bytes: [u8; 16]) -> Self {
        Self::from_bytes(bytes)
    }
}

impl From<EntityId> for [u8; 16] {
    fn from(id: EntityId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_unique() {
        let id1 = EntityId::new();
        let id2 = EntityId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn from_bytes_roundtrip() {
        let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let id = EntityId::from_bytes(bytes);
        assert_eq!(*id.as_bytes(), bytes);
    }

    #[test]
    fn uuid_conversion() {
        let uuid = Uuid::new_v4();
        let id = EntityId::from_uuid(uuid);
        assert_eq!(id.to_uuid(), uuid);
    }

    #[test]
    fn from_slice() {
        let bytes = [0u8; 16];
        assert!(EntityId::from_slice(&bytes).is_some());
        assert!(EntityId::from_slice(&[0u8; 15]).is_none());
        assert!(EntityId::from_slice(&[0u8; 17]).is_none());
    }

    #[test]
    fn ordering() {
        let id1 = EntityId::from_bytes([0; 16]);
        let id2 = EntityId::from_bytes([1; 16]);
        assert!(id1 < id2);
    }

    #[test]
    fn display() {
        let id = EntityId::from_bytes([0; 16]);
        let s = format!("{id}");
        assert!(!s.is_empty());
    }
}
