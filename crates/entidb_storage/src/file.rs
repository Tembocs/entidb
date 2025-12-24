//! File-based storage backend for persistent storage.

use crate::backend::StorageBackend;
use crate::error::{StorageError, StorageResult};
use parking_lot::RwLock;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A file-based storage backend.
///
/// This backend provides persistent storage using OS file APIs.
/// Data survives process restarts.
///
/// # Durability
///
/// - `flush()` calls `File::flush()` to push data to the OS
/// - `sync()` calls `File::sync_all()` to ensure data is on disk
///
/// # Thread Safety
///
/// This backend is thread-safe and can be shared across threads.
/// Internal locking ensures consistent access.
///
/// # Example
///
/// ```no_run
/// use entidb_storage::{StorageBackend, FileBackend};
/// use std::path::Path;
///
/// let mut backend = FileBackend::open(Path::new("data.bin")).unwrap();
/// let offset = backend.append(b"persistent data").unwrap();
/// backend.sync().unwrap();  // Ensure data is durable
/// ```
#[derive(Debug)]
pub struct FileBackend {
    path: PathBuf,
    file: RwLock<File>,
    size: RwLock<u64>,
}

impl FileBackend {
    /// Opens or creates a file backend at the given path.
    ///
    /// If the file exists, it is opened for reading and appending.
    /// If it doesn't exist, a new file is created.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or created.
    pub fn open(path: &Path) -> StorageResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        let size = file.metadata()?.len();

        Ok(Self {
            path: path.to_path_buf(),
            file: RwLock::new(file),
            size: RwLock::new(size),
        })
    }

    /// Opens or creates a file backend, creating parent directories if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if directories cannot be created or file cannot be opened.
    pub fn open_with_create_dirs(path: &Path) -> StorageResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::open(path)
    }

    /// Returns the path to the underlying file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl StorageBackend for FileBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        let size = *self.size.read();
        let end = offset.saturating_add(len as u64);

        if offset > size || end > size {
            return Err(StorageError::ReadPastEnd { offset, len, size });
        }

        if len == 0 {
            return Ok(Vec::new());
        }

        let mut file = self.file.write();
        file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; len];
        file.read_exact(&mut buffer)?;

        Ok(buffer)
    }

    fn append(&mut self, data: &[u8]) -> StorageResult<u64> {
        if data.is_empty() {
            return Ok(*self.size.read());
        }

        let mut file = self.file.write();
        let mut size = self.size.write();

        let offset = *size;
        file.seek(SeekFrom::End(0))?;
        file.write_all(data)?;
        *size += data.len() as u64;

        Ok(offset)
    }

    fn flush(&mut self) -> StorageResult<()> {
        let mut file = self.file.write();
        file.flush()?;
        Ok(())
    }

    fn size(&self) -> StorageResult<u64> {
        Ok(*self.size.read())
    }

    fn sync(&mut self) -> StorageResult<()> {
        let file = self.file.write();
        file.sync_all()?;
        Ok(())
    }

    fn truncate(&mut self, new_size: u64) -> StorageResult<()> {
        let mut file = self.file.write();
        let mut size = self.size.write();

        if new_size > *size {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "cannot truncate to size {} which is greater than current size {}",
                    new_size, *size
                ),
            )));
        }

        file.set_len(new_size)?;
        file.sync_all()?;
        *size = new_size;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn file_create_new() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let backend = FileBackend::open(&path).unwrap();
        assert_eq!(backend.size().unwrap(), 0);
        assert!(path.exists());
    }

    #[test]
    fn file_append_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let mut backend = FileBackend::open(&path).unwrap();

        let offset1 = backend.append(b"hello").unwrap();
        assert_eq!(offset1, 0);

        let offset2 = backend.append(b" world").unwrap();
        assert_eq!(offset2, 5);

        assert_eq!(backend.size().unwrap(), 11);

        let data = backend.read_at(0, 11).unwrap();
        assert_eq!(&data, b"hello world");
    }

    #[test]
    fn file_read_partial() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let mut backend = FileBackend::open(&path).unwrap();
        backend.append(b"hello world").unwrap();

        let data = backend.read_at(6, 5).unwrap();
        assert_eq!(&data, b"world");
    }

    #[test]
    fn file_read_past_end_fails() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let mut backend = FileBackend::open(&path).unwrap();
        backend.append(b"hello").unwrap();

        let result = backend.read_at(10, 5);
        assert!(matches!(result, Err(StorageError::ReadPastEnd { .. })));
    }

    #[test]
    fn file_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        // Write data
        {
            let mut backend = FileBackend::open(&path).unwrap();
            backend.append(b"persistent data").unwrap();
            backend.sync().unwrap();
        }

        // Reopen and read
        {
            let backend = FileBackend::open(&path).unwrap();
            assert_eq!(backend.size().unwrap(), 15);

            let data = backend.read_at(0, 15).unwrap();
            assert_eq!(&data, b"persistent data");
        }
    }

    #[test]
    fn file_empty_append() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let mut backend = FileBackend::open(&path).unwrap();
        backend.append(b"x").unwrap();

        let offset = backend.append(b"").unwrap();
        assert_eq!(offset, 1);
        assert_eq!(backend.size().unwrap(), 1);
    }

    #[test]
    fn file_empty_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let mut backend = FileBackend::open(&path).unwrap();
        backend.append(b"hello").unwrap();

        let data = backend.read_at(2, 0).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn file_create_with_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("path").join("test.bin");

        let backend = FileBackend::open_with_create_dirs(&path).unwrap();
        assert_eq!(backend.size().unwrap(), 0);
        assert!(path.exists());
    }

    #[test]
    fn file_flush_and_sync() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let mut backend = FileBackend::open(&path).unwrap();
        backend.append(b"data").unwrap();

        assert!(backend.flush().is_ok());
        assert!(backend.sync().is_ok());
    }

    #[test]
    fn file_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        let backend = FileBackend::open(&path).unwrap();
        assert_eq!(backend.path(), path);
    }
}
