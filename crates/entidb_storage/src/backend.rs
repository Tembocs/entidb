//! Storage backend trait definition.

use crate::error::StorageResult;

/// A low-level storage backend for EntiDB.
///
/// Storage backends are **opaque byte stores**. They provide simple operations
/// for reading, appending, and flushing data. EntiDB owns all file format
/// interpretation - backends do not understand WAL records, segments, or entities.
///
/// # Invariants
///
/// - `append` returns the offset where data was written
/// - `read_at` returns exactly the bytes previously written at that offset
/// - `flush` ensures all appended data is durable
/// - Backends must be `Send + Sync` for concurrent access
///
/// # Implementors
///
/// - [`super::InMemoryBackend`] - For testing
/// - [`super::FileBackend`] - For persistent storage
pub trait StorageBackend: Send + Sync {
    /// Reads `len` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The offset is beyond the current size
    /// - The read would extend beyond the current size
    /// - An I/O error occurs
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>>;

    /// Appends data to the end of the storage.
    ///
    /// Returns the offset where the data was written.
    ///
    /// # Errors
    ///
    /// Returns an error if an I/O error occurs.
    fn append(&mut self, data: &[u8]) -> StorageResult<u64>;

    /// Flushes all pending writes to durable storage.
    ///
    /// After this returns successfully, all previously appended data
    /// is guaranteed to survive process termination.
    ///
    /// # Errors
    ///
    /// Returns an error if the flush operation fails.
    fn flush(&mut self) -> StorageResult<()>;

    /// Returns the current size of the storage in bytes.
    ///
    /// This is the offset where the next `append` will write.
    ///
    /// # Errors
    ///
    /// Returns an error if the size cannot be determined.
    fn size(&self) -> StorageResult<u64>;

    /// Syncs all data and metadata to durable storage.
    ///
    /// This is a stronger guarantee than `flush` - it ensures that
    /// file metadata (size, timestamps) is also durable.
    ///
    /// # Errors
    ///
    /// Returns an error if the sync operation fails.
    fn sync(&mut self) -> StorageResult<()>;

    /// Truncates the storage to the given size.
    ///
    /// This removes all data after the specified offset. This is used
    /// for WAL truncation after checkpoint.
    ///
    /// # Arguments
    ///
    /// * `new_size` - The new size of the storage (offset to truncate to)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The truncation fails
    /// - `new_size` is greater than current size
    fn truncate(&mut self, new_size: u64) -> StorageResult<()>;
}
