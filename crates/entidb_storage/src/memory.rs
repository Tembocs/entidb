//! In-memory storage backend for testing.

use crate::backend::StorageBackend;
use crate::error::{StorageError, StorageResult};
use parking_lot::RwLock;

/// An in-memory storage backend.
///
/// This backend stores all data in memory and is suitable for:
/// - Unit tests
/// - Integration tests
/// - Ephemeral databases that don't need persistence
///
/// # Thread Safety
///
/// This backend is thread-safe and can be shared across threads.
///
/// # Example
///
/// ```rust
/// use entidb_storage::{StorageBackend, InMemoryBackend};
///
/// let mut backend = InMemoryBackend::new();
/// let offset = backend.append(b"test data").unwrap();
/// assert_eq!(offset, 0);
/// assert_eq!(backend.size().unwrap(), 9);
/// ```
#[derive(Debug, Default)]
pub struct InMemoryBackend {
    data: RwLock<Vec<u8>>,
}

impl InMemoryBackend {
    /// Creates a new empty in-memory backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new in-memory backend with pre-existing data.
    ///
    /// Useful for testing recovery scenarios.
    #[must_use]
    pub fn with_data(data: Vec<u8>) -> Self {
        Self {
            data: RwLock::new(data),
        }
    }

    /// Returns a copy of all data in the backend.
    ///
    /// Useful for testing and debugging.
    #[must_use]
    pub fn data(&self) -> Vec<u8> {
        self.data.read().clone()
    }

    /// Clears all data from the backend.
    pub fn clear(&mut self) {
        self.data.write().clear();
    }
}

impl StorageBackend for InMemoryBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        let data = self.data.read();
        let size = data.len() as u64;
        let offset_usize = offset as usize;
        let end = offset_usize.saturating_add(len);

        if offset > size || end > data.len() {
            return Err(StorageError::ReadPastEnd { offset, len, size });
        }

        Ok(data[offset_usize..end].to_vec())
    }

    fn append(&mut self, new_data: &[u8]) -> StorageResult<u64> {
        let mut data = self.data.write();
        let offset = data.len() as u64;
        data.extend_from_slice(new_data);
        Ok(offset)
    }

    fn flush(&mut self) -> StorageResult<()> {
        // In-memory backend has no pending writes
        Ok(())
    }

    fn size(&self) -> StorageResult<u64> {
        Ok(self.data.read().len() as u64)
    }

    fn sync(&mut self) -> StorageResult<()> {
        // In-memory backend has no metadata to sync
        Ok(())
    }

    fn truncate(&mut self, new_size: u64) -> StorageResult<()> {
        let mut data = self.data.write();
        let current_size = data.len() as u64;

        if new_size > current_size {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "cannot truncate to size {} which is greater than current size {}",
                    new_size, current_size
                ),
            )));
        }

        data.truncate(new_size as usize);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_new_is_empty() {
        let backend = InMemoryBackend::new();
        assert_eq!(backend.size().unwrap(), 0);
        assert!(backend.data().is_empty());
    }

    #[test]
    fn memory_append_returns_correct_offset() {
        let mut backend = InMemoryBackend::new();

        let offset1 = backend.append(b"hello").unwrap();
        assert_eq!(offset1, 0);

        let offset2 = backend.append(b" world").unwrap();
        assert_eq!(offset2, 5);

        assert_eq!(backend.size().unwrap(), 11);
    }

    #[test]
    fn memory_read_at_returns_correct_data() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello world").unwrap();

        let data = backend.read_at(0, 5).unwrap();
        assert_eq!(&data, b"hello");

        let data = backend.read_at(6, 5).unwrap();
        assert_eq!(&data, b"world");
    }

    #[test]
    fn memory_read_at_past_end_fails() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello").unwrap();

        let result = backend.read_at(10, 5);
        assert!(matches!(result, Err(StorageError::ReadPastEnd { .. })));
    }

    #[test]
    fn memory_read_at_extending_past_end_fails() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello").unwrap();

        let result = backend.read_at(3, 10);
        assert!(matches!(result, Err(StorageError::ReadPastEnd { .. })));
    }

    #[test]
    fn memory_empty_append() {
        let mut backend = InMemoryBackend::new();
        let offset = backend.append(b"").unwrap();
        assert_eq!(offset, 0);
        assert_eq!(backend.size().unwrap(), 0);
    }

    #[test]
    fn memory_empty_read() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello").unwrap();

        let data = backend.read_at(2, 0).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn memory_with_data() {
        let backend = InMemoryBackend::with_data(b"preloaded".to_vec());
        assert_eq!(backend.size().unwrap(), 9);
        assert_eq!(backend.read_at(0, 9).unwrap(), b"preloaded");
    }

    #[test]
    fn memory_clear() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"some data").unwrap();
        backend.clear();
        assert_eq!(backend.size().unwrap(), 0);
    }

    #[test]
    fn memory_flush_succeeds() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"data").unwrap();
        assert!(backend.flush().is_ok());
    }

    #[test]
    fn memory_sync_succeeds() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"data").unwrap();
        assert!(backend.sync().is_ok());
    }

    #[test]
    fn memory_truncate_to_zero() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello world").unwrap();
        assert_eq!(backend.size().unwrap(), 11);

        backend.truncate(0).unwrap();
        assert_eq!(backend.size().unwrap(), 0);
        assert!(backend.data().is_empty());
    }

    #[test]
    fn memory_truncate_partial() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello world").unwrap();
        
        backend.truncate(5).unwrap();
        assert_eq!(backend.size().unwrap(), 5);
        assert_eq!(backend.read_at(0, 5).unwrap(), b"hello");
    }

    #[test]
    fn memory_truncate_to_larger_size_fails() {
        let mut backend = InMemoryBackend::new();
        backend.append(b"hello").unwrap();
        
        let result = backend.truncate(100);
        assert!(result.is_err());
    }
}
