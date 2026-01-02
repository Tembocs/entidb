//! Database directory management.
//!
//! This module handles the file system layout for EntiDB:
//!
//! ```text
//! <db_path>/
//! ├─ MANIFEST          # Metadata (collections, format version)
//! ├─ LOCK              # Advisory lock for single-writer
//! ├─ wal.log           # Write-ahead log
//! └─ segments.dat      # Segment storage
//! ```
//!
//! The LOCK file ensures only one process can write to the database at a time.
//! The MANIFEST file persists collection mappings across restarts.

use crate::error::{CoreError, CoreResult};
use crate::manifest::Manifest;
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// File names within the database directory.
const MANIFEST_FILE: &str = "MANIFEST";
const LOCK_FILE: &str = "LOCK";
const WAL_FILE: &str = "wal.log";
#[allow(dead_code)]
const SEGMENT_FILE: &str = "segments.dat";
/// Directory for segment files (multi-segment mode).
const SEGMENTS_DIR: &str = "SEGMENTS";
/// Temporary file for atomic manifest writes.
const MANIFEST_TEMP: &str = "MANIFEST.tmp";

/// Manages the database directory structure and file locking.
///
/// # Thread Safety
///
/// The `DatabaseDir` holds an exclusive lock on the database directory.
/// Only one `DatabaseDir` instance can exist per directory at a time.
///
/// # Example
///
/// ```rust,ignore
/// use entidb_core::dir::DatabaseDir;
/// use std::path::Path;
///
/// let dir = DatabaseDir::open(Path::new("my_db"), true)?;
/// println!("WAL path: {:?}", dir.wal_path());
/// ```
#[derive(Debug)]
pub struct DatabaseDir {
    /// Root directory path.
    path: PathBuf,
    /// Lock file handle (held for exclusive access).
    _lock_file: File,
}

impl DatabaseDir {
    /// Opens or creates a database directory.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the database directory
    /// * `create_if_missing` - If true, creates the directory if it doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory doesn't exist and `create_if_missing` is false
    /// - Another process holds the lock (returns `DatabaseLocked`)
    /// - I/O errors occur
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let dir = DatabaseDir::open(Path::new("my_db"), true)?;
    /// ```
    pub fn open(path: &Path, create_if_missing: bool) -> CoreResult<Self> {
        // Create directory if needed
        if !path.exists() {
            if create_if_missing {
                fs::create_dir_all(path)?;
            } else {
                return Err(CoreError::invalid_format(format!(
                    "database directory does not exist: {}",
                    path.display()
                )));
            }
        }

        // Verify it's a directory
        if !path.is_dir() {
            return Err(CoreError::invalid_format(format!(
                "path is not a directory: {}",
                path.display()
            )));
        }

        // Acquire exclusive lock
        let lock_path = path.join(LOCK_FILE);
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        // Try to acquire exclusive lock (non-blocking)
        if lock_file.try_lock_exclusive().is_err() {
            return Err(CoreError::DatabaseLocked);
        }

        Ok(Self {
            path: path.to_path_buf(),
            _lock_file: lock_file,
        })
    }

    /// Returns the path to the database directory.
    #[must_use]
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the path to the WAL file.
    #[must_use]
    pub fn wal_path(&self) -> PathBuf {
        self.path.join(WAL_FILE)
    }

    /// Returns the path to the segment file (legacy single-file mode).
    #[must_use]
    #[allow(dead_code)]
    pub fn segment_path(&self) -> PathBuf {
        self.path.join(SEGMENT_FILE)
    }

    /// Returns the path to the segments directory (multi-segment mode).
    ///
    /// This directory contains individual segment files like `seg-000001.dat`.
    /// Use this for production databases that need segment rotation.
    #[must_use]
    pub fn segments_dir(&self) -> PathBuf {
        self.path.join(SEGMENTS_DIR)
    }

    /// Returns the path to the MANIFEST file.
    #[must_use]
    pub fn manifest_path(&self) -> PathBuf {
        self.path.join(MANIFEST_FILE)
    }

    /// Loads the manifest from disk.
    ///
    /// Returns `None` if the manifest file doesn't exist (new database).
    pub fn load_manifest(&self) -> CoreResult<Option<Manifest>> {
        let manifest_path = self.manifest_path();

        if !manifest_path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&manifest_path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        if data.is_empty() {
            return Ok(None);
        }

        let manifest = Manifest::decode(&data)?;
        Ok(Some(manifest))
    }

    /// Saves the manifest to disk atomically.
    ///
    /// Uses write-then-rename pattern for crash safety:
    /// 1. Write to temporary file
    /// 2. Sync temporary file to disk
    /// 3. Rename temporary file to MANIFEST
    /// 4. Fsync the directory to ensure metadata update is durable
    pub fn save_manifest(&self, manifest: &Manifest) -> CoreResult<()> {
        let manifest_path = self.manifest_path();
        let temp_path = self.path.join(MANIFEST_TEMP);

        // Write to temp file
        let data = manifest.encode();
        let mut file = File::create(&temp_path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        drop(file);

        // Atomic rename
        fs::rename(&temp_path, &manifest_path)?;

        // Fsync directory to ensure rename is durable
        self.sync_directory()?;

        Ok(())
    }

    /// Syncs the database directory to ensure metadata updates are durable.
    ///
    /// This is critical for crash safety: after creating, renaming, or deleting
    /// files, the directory must be fsynced to ensure the metadata is on disk.
    ///
    /// On Windows, directory fsync is not supported in the same way as Unix.
    /// Windows NTFS uses journaling which provides similar durability guarantees
    /// for metadata operations, so we skip the explicit fsync on Windows.
    #[cfg(unix)]
    fn sync_directory(&self) -> CoreResult<()> {
        use std::os::unix::io::AsRawFd;
        // Open directory and sync it
        let dir = File::open(&self.path)?;
        // On Unix, fsync on a directory syncs the directory entries
        dir.sync_all()?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn sync_directory(&self) -> CoreResult<()> {
        // Windows NTFS journal provides metadata durability guarantees
        // Directory fsync is not directly supported on Windows
        Ok(())
    }

    /// Syncs the segments directory to ensure metadata updates are durable.
    #[cfg(unix)]
    fn sync_segments_directory(&self) -> CoreResult<()> {
        let segments_dir = self.segments_dir();
        if segments_dir.exists() {
            let dir = File::open(&segments_dir)?;
            dir.sync_all()?;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn sync_segments_directory(&self) -> CoreResult<()> {
        // Windows NTFS journal provides metadata durability guarantees
        Ok(())
    }

    /// Checks if this is a new (empty) database directory.
    #[must_use]
    pub fn is_new_database(&self) -> bool {
        !self.manifest_path().exists() && !self.wal_path().exists()
    }

    /// Returns the path to a specific segment file.
    ///
    /// # Arguments
    ///
    /// * `segment_id` - The segment ID (e.g., 1 produces "seg-000001.dat")
    #[must_use]
    #[allow(dead_code)] // Public API for bindings
    pub fn segment_file_path(&self, segment_id: u64) -> PathBuf {
        self.segments_dir().join(format!("seg-{segment_id:06}.dat"))
    }

    /// Deletes segment files for the given segment IDs.
    ///
    /// This is used during compaction to remove old segment files
    /// after their data has been merged into a new segment.
    ///
    /// After deletion, the segments directory is fsynced to ensure
    /// the metadata updates are crash-safe.
    ///
    /// # Arguments
    ///
    /// * `segment_ids` - List of segment IDs to delete
    ///
    /// # Returns
    ///
    /// The number of files successfully deleted.
    pub fn delete_segment_files(&self, segment_ids: &[u64]) -> CoreResult<usize> {
        let mut deleted = 0;
        let segments_dir = self.segments_dir();

        for &segment_id in segment_ids {
            let segment_path = segments_dir.join(format!("seg-{segment_id:06}.dat"));
            if segment_path.exists() {
                fs::remove_file(&segment_path)?;
                deleted += 1;
            }
        }

        // Fsync segments directory to ensure deletions are durable
        if deleted > 0 {
            self.sync_segments_directory()?;
        }

        Ok(deleted)
    }

    /// Creates a new segment file and returns its path.
    ///
    /// After creation, the segments directory is fsynced to ensure
    /// the new file's metadata is crash-safe.
    #[allow(dead_code)] // Public API for future use
    pub fn create_segment_file(&self, segment_id: u64) -> CoreResult<PathBuf> {
        let segments_dir = self.segments_dir();
        let segment_path = segments_dir.join(format!("seg-{segment_id:06}.dat"));

        // Create the file
        File::create(&segment_path)?;

        // Fsync segments directory to ensure creation is durable
        self.sync_segments_directory()?;

        Ok(segment_path)
    }
}

impl Drop for DatabaseDir {
    fn drop(&mut self) {
        // Lock is automatically released when file is closed
        // The fs2 crate handles this properly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_creates_directory() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("new_db");

        assert!(!db_path.exists());

        let dir = DatabaseDir::open(&db_path, true).unwrap();
        assert!(db_path.exists());
        assert!(db_path.is_dir());

        drop(dir);
    }

    #[test]
    fn open_fails_if_not_exists_and_no_create() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("nonexistent");

        let result = DatabaseDir::open(&db_path, false);
        assert!(result.is_err());
    }

    #[test]
    fn lock_prevents_second_open() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("locked_db");

        let _dir1 = DatabaseDir::open(&db_path, true).unwrap();

        // Second open should fail with DatabaseLocked
        let result = DatabaseDir::open(&db_path, true);
        assert!(matches!(result, Err(CoreError::DatabaseLocked)));
    }

    #[test]
    fn lock_released_on_drop() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("reopen_db");

        {
            let _dir = DatabaseDir::open(&db_path, true).unwrap();
        }

        // Should succeed after first dir is dropped
        let _dir2 = DatabaseDir::open(&db_path, true).unwrap();
    }

    #[test]
    fn manifest_round_trip() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("manifest_db");

        let dir = DatabaseDir::open(&db_path, true).unwrap();

        // Initially no manifest
        assert!(dir.load_manifest().unwrap().is_none());
        assert!(dir.is_new_database());

        // Create and save manifest
        let mut manifest = Manifest::new((1, 0));
        manifest.get_or_create_collection("users");
        manifest.get_or_create_collection("posts");

        dir.save_manifest(&manifest).unwrap();

        // Load and verify
        let loaded = dir.load_manifest().unwrap().unwrap();
        assert_eq!(loaded.format_version, (1, 0));
        assert_eq!(loaded.get_collection("users"), Some(1));
        assert_eq!(loaded.get_collection("posts"), Some(2));
    }

    #[test]
    fn paths_are_correct() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("paths_db");

        let dir = DatabaseDir::open(&db_path, true).unwrap();

        assert_eq!(dir.wal_path(), db_path.join("wal.log"));
        assert_eq!(dir.segment_path(), db_path.join("segments.dat"));
        assert_eq!(dir.manifest_path(), db_path.join("MANIFEST"));
    }
}
