//! In-memory storage backend for WASM.
//!
//! This is a simple in-memory storage that works in any browser.
//! It's primarily used for testing and ephemeral databases.

#![allow(dead_code)]

use entidb_storage::{StorageBackend, StorageError, StorageResult};
use std::sync::RwLock;

/// In-memory storage backend for WASM environments.
///
/// This backend stores all data in memory and is lost when the page is closed.
/// It's useful for testing and temporary data that doesn't need persistence.
pub struct WasmMemoryBackend {
    data: RwLock<Vec<u8>>,
}

impl WasmMemoryBackend {
    /// Creates a new empty in-memory backend.
    pub fn new() -> Self {
        Self {
            data: RwLock::new(Vec::new()),
        }
    }

    /// Creates a backend with initial data.
    pub fn with_data(data: Vec<u8>) -> Self {
        Self {
            data: RwLock::new(data),
        }
    }
}

impl Default for WasmMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for WasmMemoryBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        let data = self.data.read().map_err(|_| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;

        let offset = offset as usize;
        if offset > data.len() {
            return Err(StorageError::ReadPastEnd {
                offset: offset as u64,
                len,
                size: data.len() as u64,
            });
        }

        let end = offset.saturating_add(len);
        if end > data.len() {
            return Err(StorageError::ReadPastEnd {
                offset: offset as u64,
                len,
                size: data.len() as u64,
            });
        }

        Ok(data[offset..end].to_vec())
    }

    fn append(&mut self, bytes: &[u8]) -> StorageResult<u64> {
        let mut data = self.data.write().map_err(|_| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;

        let offset = data.len() as u64;
        data.extend_from_slice(bytes);
        Ok(offset)
    }

    fn flush(&mut self) -> StorageResult<()> {
        // No-op for in-memory storage
        Ok(())
    }

    fn size(&self) -> StorageResult<u64> {
        let data = self.data.read().map_err(|_| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        Ok(data.len() as u64)
    }

    fn sync(&mut self) -> StorageResult<()> {
        // No-op for in-memory storage
        Ok(())
    }

    fn truncate(&mut self, new_size: u64) -> StorageResult<()> {
        let mut data = self.data.write().map_err(|_| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "lock poisoned",
            ))
        })?;
        data.truncate(new_size as usize);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_backend_append_and_read() {
        let mut backend = WasmMemoryBackend::new();

        let offset = backend.append(b"hello").unwrap();
        assert_eq!(offset, 0);

        let data = backend.read_at(0, 5).unwrap();
        assert_eq!(&data, b"hello");
    }

    #[test]
    fn memory_backend_size() {
        let mut backend = WasmMemoryBackend::new();
        assert_eq!(backend.size().unwrap(), 0);

        backend.append(b"hello").unwrap();
        assert_eq!(backend.size().unwrap(), 5);
    }
}
