//! Database migration support.
//!
//! This module provides schema versioning and migration capabilities for
//! upgrading database schemas over time.
//!
//! ## Design Philosophy
//!
//! Migrations in EntiDB are:
//! - **Metadata-driven**: No code generation, migrations are defined in code
//! - **Forward-only**: Rollbacks are not automatic (use backups instead)
//! - **Explicit**: All migrations must be registered and run explicitly
//! - **Transactional**: Each migration runs in its own transaction
//!
//! ## Usage
//!
//! ```ignore
//! use entidb_core::migration::{MigrationManager, Migration};
//!
//! struct AddEmailIndex;
//! impl Migration for AddEmailIndex {
//!     fn version(&self) -> u64 { 1 }
//!     fn name(&self) -> &str { "add_email_index" }
//!     fn up(&self, ctx: &mut MigrationContext) -> CoreResult<()> {
//!         ctx.create_index("users", "email_idx", IndexSpec::hash("email"))?;
//!         Ok(())
//!     }
//! }
//!
//! let mut manager = MigrationManager::new();
//! manager.register(Box::new(AddEmailIndex));
//! manager.run_pending(&mut db)?;
//! ```

use crate::error::{CoreError, CoreResult};
use std::collections::BTreeMap;

/// Version number for migrations.
pub type MigrationVersion = u64;

/// Information about a migration.
#[derive(Debug, Clone)]
pub struct MigrationInfo {
    /// Version number (unique, sequential).
    pub version: MigrationVersion,
    /// Human-readable name.
    pub name: String,
    /// Description of what this migration does.
    pub description: Option<String>,
}

/// Result of running a single migration.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// The migration version that was run.
    pub version: MigrationVersion,
    /// The migration name.
    pub name: String,
    /// Whether the migration succeeded.
    pub success: bool,
    /// Error message if migration failed.
    pub error: Option<String>,
}

/// Result of running all pending migrations.
#[derive(Debug, Clone)]
pub struct MigrationRunResult {
    /// List of migrations that were run.
    pub migrations: Vec<MigrationResult>,
    /// The final schema version.
    pub final_version: MigrationVersion,
    /// Number of migrations applied.
    pub applied_count: usize,
    /// Number of migrations that failed.
    pub failed_count: usize,
}

/// Context passed to migration functions.
///
/// This provides the operations that can be performed during a migration.
#[derive(Debug)]
pub struct MigrationContext {
    /// The current database schema version.
    pub current_version: MigrationVersion,
    /// Operations performed during this migration (for logging/debugging).
    pub operations: Vec<MigrationOperation>,
}

/// An operation performed during a migration.
#[derive(Debug, Clone)]
pub enum MigrationOperation {
    /// Created a new collection.
    CreateCollection {
        /// Name of the collection.
        name: String,
    },
    /// Dropped a collection.
    DropCollection {
        /// Name of the collection.
        name: String,
    },
    /// Created an index.
    CreateIndex {
        /// Collection the index is on.
        collection: String,
        /// Name of the index.
        index_name: String,
    },
    /// Dropped an index.
    DropIndex {
        /// Collection the index was on.
        collection: String,
        /// Name of the index.
        index_name: String,
    },
    /// Custom operation.
    Custom {
        /// Description of the operation.
        description: String,
    },
}

impl MigrationContext {
    /// Creates a new migration context.
    #[must_use]
    pub fn new(current_version: MigrationVersion) -> Self {
        Self {
            current_version,
            operations: Vec::new(),
        }
    }

    /// Records a create collection operation.
    pub fn create_collection(&mut self, name: &str) {
        self.operations.push(MigrationOperation::CreateCollection {
            name: name.to_string(),
        });
    }

    /// Records a drop collection operation.
    pub fn drop_collection(&mut self, name: &str) {
        self.operations.push(MigrationOperation::DropCollection {
            name: name.to_string(),
        });
    }

    /// Records a create index operation.
    pub fn create_index(&mut self, collection: &str, index_name: &str) {
        self.operations.push(MigrationOperation::CreateIndex {
            collection: collection.to_string(),
            index_name: index_name.to_string(),
        });
    }

    /// Records a drop index operation.
    pub fn drop_index(&mut self, collection: &str, index_name: &str) {
        self.operations.push(MigrationOperation::DropIndex {
            collection: collection.to_string(),
            index_name: index_name.to_string(),
        });
    }

    /// Records a custom operation.
    pub fn custom(&mut self, description: &str) {
        self.operations.push(MigrationOperation::Custom {
            description: description.to_string(),
        });
    }
}

/// Trait for defining migrations.
pub trait Migration: Send + Sync {
    /// Returns the version number for this migration.
    ///
    /// Versions must be unique and sequential starting from 1.
    fn version(&self) -> MigrationVersion;

    /// Returns the name of this migration.
    fn name(&self) -> &str;

    /// Returns an optional description.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Runs the migration.
    ///
    /// This is called when the migration is applied.
    fn up(&self, ctx: &mut MigrationContext) -> CoreResult<()>;
}

/// Stored migration state.
#[derive(Debug, Clone)]
pub struct AppliedMigration {
    /// Version number.
    pub version: MigrationVersion,
    /// Migration name.
    pub name: String,
    /// When the migration was applied (Unix timestamp in milliseconds).
    pub applied_at: u64,
}

/// Migration state for the database.
#[derive(Debug, Clone, Default)]
pub struct MigrationState {
    /// Current schema version.
    pub current_version: MigrationVersion,
    /// List of applied migrations.
    pub applied: Vec<AppliedMigration>,
}

impl MigrationState {
    /// Creates a new empty migration state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_version: 0,
            applied: Vec::new(),
        }
    }

    /// Checks if a version has been applied.
    #[must_use]
    pub fn is_applied(&self, version: MigrationVersion) -> bool {
        self.applied.iter().any(|m| m.version == version)
    }

    /// Records a migration as applied.
    pub fn record(&mut self, version: MigrationVersion, name: &str, applied_at: u64) {
        if !self.is_applied(version) {
            self.applied.push(AppliedMigration {
                version,
                name: name.to_string(),
                applied_at,
            });
            if version > self.current_version {
                self.current_version = version;
            }
        }
    }
}

/// Manages database migrations.
pub struct MigrationManager {
    /// Registered migrations, keyed by version.
    migrations: BTreeMap<MigrationVersion, Box<dyn Migration>>,
}

impl MigrationManager {
    /// Creates a new migration manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            migrations: BTreeMap::new(),
        }
    }

    /// Registers a migration.
    ///
    /// Returns an error if a migration with the same version already exists.
    pub fn register(&mut self, migration: Box<dyn Migration>) -> CoreResult<()> {
        let version = migration.version();
        if self.migrations.contains_key(&version) {
            return Err(CoreError::migration_failed(format!(
                "migration version {} already registered",
                version
            )));
        }
        self.migrations.insert(version, migration);
        Ok(())
    }

    /// Returns list of registered migrations.
    #[must_use]
    pub fn list(&self) -> Vec<MigrationInfo> {
        self.migrations
            .values()
            .map(|m| MigrationInfo {
                version: m.version(),
                name: m.name().to_string(),
                description: m.description().map(String::from),
            })
            .collect()
    }

    /// Returns pending migrations that haven't been applied yet.
    #[must_use]
    pub fn pending(&self, state: &MigrationState) -> Vec<MigrationInfo> {
        self.migrations
            .values()
            .filter(|m| !state.is_applied(m.version()))
            .map(|m| MigrationInfo {
                version: m.version(),
                name: m.name().to_string(),
                description: m.description().map(String::from),
            })
            .collect()
    }

    /// Runs all pending migrations.
    pub fn run_pending(&self, state: &mut MigrationState) -> CoreResult<MigrationRunResult> {
        let pending: Vec<_> = self
            .migrations
            .iter()
            .filter(|(v, _)| !state.is_applied(**v))
            .collect();

        let mut results = Vec::new();
        let mut applied_count = 0;
        let mut failed_count = 0;

        for (version, migration) in pending {
            let mut ctx = MigrationContext::new(state.current_version);

            let result = match migration.up(&mut ctx) {
                Ok(()) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    state.record(*version, migration.name(), now);
                    applied_count += 1;

                    MigrationResult {
                        version: *version,
                        name: migration.name().to_string(),
                        success: true,
                        error: None,
                    }
                }
                Err(e) => {
                    failed_count += 1;
                    let result = MigrationResult {
                        version: *version,
                        name: migration.name().to_string(),
                        success: false,
                        error: Some(e.to_string()),
                    };
                    results.push(result);
                    break; // Stop on first failure
                }
            };

            results.push(result);
        }

        Ok(MigrationRunResult {
            migrations: results,
            final_version: state.current_version,
            applied_count,
            failed_count,
        })
    }

    /// Runs a specific migration by version.
    pub fn run_one(
        &self,
        version: MigrationVersion,
        state: &mut MigrationState,
    ) -> CoreResult<MigrationResult> {
        let migration = self
            .migrations
            .get(&version)
            .ok_or_else(|| CoreError::migration_failed(format!("migration {} not found", version)))?;

        if state.is_applied(version) {
            return Err(CoreError::migration_failed(format!(
                "migration {} already applied",
                version
            )));
        }

        let mut ctx = MigrationContext::new(state.current_version);

        match migration.up(&mut ctx) {
            Ok(()) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                state.record(version, migration.name(), now);

                Ok(MigrationResult {
                    version,
                    name: migration.name().to_string(),
                    success: true,
                    error: None,
                })
            }
            Err(e) => Ok(MigrationResult {
                version,
                name: migration.name().to_string(),
                success: false,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Validates that migrations are sequential with no gaps.
    pub fn validate(&self) -> CoreResult<()> {
        let versions: Vec<_> = self.migrations.keys().copied().collect();
        if versions.is_empty() {
            return Ok(());
        }

        // Check that versions start at 1 and are sequential
        for (i, version) in versions.iter().enumerate() {
            let expected = (i + 1) as u64;
            if *version != expected {
                return Err(CoreError::migration_failed(format!(
                    "migration version gap: expected {}, got {}",
                    expected, version
                )));
            }
        }

        Ok(())
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMigration {
        version: MigrationVersion,
        name: String,
        should_fail: bool,
    }

    impl Migration for TestMigration {
        fn version(&self) -> MigrationVersion {
            self.version
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn up(&self, ctx: &mut MigrationContext) -> CoreResult<()> {
            if self.should_fail {
                return Err(CoreError::migration_failed("intentional failure"));
            }
            ctx.custom("test operation");
            Ok(())
        }
    }

    fn make_migration(version: u64, name: &str) -> Box<dyn Migration> {
        Box::new(TestMigration {
            version,
            name: name.to_string(),
            should_fail: false,
        })
    }

    fn make_failing_migration(version: u64, name: &str) -> Box<dyn Migration> {
        Box::new(TestMigration {
            version,
            name: name.to_string(),
            should_fail: true,
        })
    }

    #[test]
    fn register_and_list_migrations() {
        let mut manager = MigrationManager::new();

        manager.register(make_migration(1, "first")).unwrap();
        manager.register(make_migration(2, "second")).unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].version, 1);
        assert_eq!(list[0].name, "first");
        assert_eq!(list[1].version, 2);
        assert_eq!(list[1].name, "second");
    }

    #[test]
    fn duplicate_version_rejected() {
        let mut manager = MigrationManager::new();

        manager.register(make_migration(1, "first")).unwrap();
        let result = manager.register(make_migration(1, "duplicate"));

        assert!(result.is_err());
    }

    #[test]
    fn run_pending_migrations() {
        let mut manager = MigrationManager::new();
        let mut state = MigrationState::new();

        manager.register(make_migration(1, "first")).unwrap();
        manager.register(make_migration(2, "second")).unwrap();

        let result = manager.run_pending(&mut state).unwrap();

        assert_eq!(result.applied_count, 2);
        assert_eq!(result.failed_count, 0);
        assert_eq!(result.final_version, 2);
        assert_eq!(state.current_version, 2);
        assert!(state.is_applied(1));
        assert!(state.is_applied(2));
    }

    #[test]
    fn pending_skips_applied() {
        let mut manager = MigrationManager::new();
        let mut state = MigrationState::new();

        manager.register(make_migration(1, "first")).unwrap();
        manager.register(make_migration(2, "second")).unwrap();

        // Apply first migration
        manager.run_one(1, &mut state).unwrap();

        // Only second should be pending
        let pending = manager.pending(&state);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, 2);
    }

    #[test]
    fn migration_failure_stops_execution() {
        let mut manager = MigrationManager::new();
        let mut state = MigrationState::new();

        manager.register(make_migration(1, "first")).unwrap();
        manager
            .register(make_failing_migration(2, "failing"))
            .unwrap();
        manager.register(make_migration(3, "third")).unwrap();

        let result = manager.run_pending(&mut state).unwrap();

        // First succeeds, second fails, third not attempted
        assert_eq!(result.applied_count, 1);
        assert_eq!(result.failed_count, 1);
        assert_eq!(result.migrations.len(), 2);
        assert!(state.is_applied(1));
        assert!(!state.is_applied(2));
        assert!(!state.is_applied(3));
    }

    #[test]
    fn validate_sequential_versions() {
        let mut manager = MigrationManager::new();

        manager.register(make_migration(1, "first")).unwrap();
        manager.register(make_migration(2, "second")).unwrap();

        assert!(manager.validate().is_ok());
    }

    #[test]
    fn validate_detects_gaps() {
        let mut manager = MigrationManager::new();

        manager.register(make_migration(1, "first")).unwrap();
        manager.register(make_migration(3, "third")).unwrap(); // Gap!

        assert!(manager.validate().is_err());
    }

    #[test]
    fn migration_state_tracks_applied() {
        let mut state = MigrationState::new();

        assert!(!state.is_applied(1));

        state.record(1, "first", 1000);
        assert!(state.is_applied(1));
        assert_eq!(state.current_version, 1);

        state.record(2, "second", 2000);
        assert!(state.is_applied(2));
        assert_eq!(state.current_version, 2);
    }

    #[test]
    fn migration_context_records_operations() {
        let mut ctx = MigrationContext::new(0);

        ctx.create_collection("users");
        ctx.create_index("users", "email_idx");
        ctx.custom("data transformation");

        assert_eq!(ctx.operations.len(), 3);
    }

    #[test]
    fn empty_migration_manager() {
        let manager = MigrationManager::new();
        let state = MigrationState::new();

        assert!(manager.list().is_empty());
        assert!(manager.pending(&state).is_empty());
        assert!(manager.validate().is_ok());
    }
}
