//! Database configuration.

use std::time::Duration;

/// Configuration for opening a database.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether to create the database if it doesn't exist.
    pub create_if_missing: bool,

    /// Whether to error if the database already exists.
    pub error_if_exists: bool,

    /// Maximum size of a single WAL file before rotation.
    pub max_wal_size: u64,

    /// Maximum size of a single segment file before sealing.
    pub max_segment_size: u64,

    /// How often to automatically checkpoint (0 = never).
    pub checkpoint_interval: Duration,

    /// Whether to sync WAL on every commit (safer but slower).
    pub sync_on_commit: bool,

    /// Format version to use for new databases.
    pub format_version: (u16, u16),
}

impl Default for Config {
    fn default() -> Self {
        Self {
            create_if_missing: true,
            error_if_exists: false,
            max_wal_size: 64 * 1024 * 1024,      // 64 MB
            max_segment_size: 256 * 1024 * 1024, // 256 MB
            checkpoint_interval: Duration::ZERO, // disabled
            sync_on_commit: true,
            format_version: (1, 0),
        }
    }
}

impl Config {
    /// Creates a new configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to create the database if missing.
    #[must_use]
    pub const fn create_if_missing(mut self, value: bool) -> Self {
        self.create_if_missing = value;
        self
    }

    /// Sets whether to error if database exists.
    #[must_use]
    pub const fn error_if_exists(mut self, value: bool) -> Self {
        self.error_if_exists = value;
        self
    }

    /// Sets maximum WAL file size.
    #[must_use]
    pub const fn max_wal_size(mut self, size: u64) -> Self {
        self.max_wal_size = size;
        self
    }

    /// Sets maximum segment file size.
    #[must_use]
    pub const fn max_segment_size(mut self, size: u64) -> Self {
        self.max_segment_size = size;
        self
    }

    /// Sets whether to sync WAL on every commit.
    #[must_use]
    pub const fn sync_on_commit(mut self, value: bool) -> Self {
        self.sync_on_commit = value;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = Config::default();
        assert!(config.create_if_missing);
        assert!(!config.error_if_exists);
        assert!(config.sync_on_commit);
    }

    #[test]
    fn builder_pattern() {
        let config = Config::new()
            .create_if_missing(false)
            .sync_on_commit(false)
            .max_wal_size(1024);

        assert!(!config.create_if_missing);
        assert!(!config.sync_on_commit);
        assert_eq!(config.max_wal_size, 1024);
    }
}
