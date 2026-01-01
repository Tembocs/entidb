//! Migration commands.

use entidb_core::{MigrationManager, MigrationState};
use std::path::Path;
use tracing::info;

/// Show current migration status.
pub fn status(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Checking migration status for {:?}", db_path);

    // In a real implementation, we would load the migration state from the database
    // For now, we show a placeholder implementation
    let state = load_migration_state(db_path)?;

    println!("Migration Status");
    println!("================");
    println!("  Current version: {}", state.current_version);
    println!("  Applied migrations: {}", state.applied.len());

    if !state.applied.is_empty() {
        println!("\nApplied Migrations:");
        for migration in &state.applied {
            println!(
                "  v{}: {} (applied at {})",
                migration.version,
                migration.name,
                format_timestamp(migration.applied_at)
            );
        }
    }

    Ok(())
}

/// List all registered migrations.
pub fn list(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Listing migrations for {:?}", db_path);

    let state = load_migration_state(db_path)?;
    let manager = create_migration_manager();

    let migrations = manager.list();

    println!("Registered Migrations");
    println!("====================");

    if migrations.is_empty() {
        println!("  No migrations registered.");
    } else {
        for migration in &migrations {
            let status = if state.is_applied(migration.version) {
                "✓ applied"
            } else {
                "○ pending"
            };

            println!("  v{}: {} [{}]", migration.version, migration.name, status);

            if let Some(desc) = &migration.description {
                println!("      {}", desc);
            }
        }
    }

    Ok(())
}

/// Run pending migrations.
pub fn run(
    db_path: &Path,
    to_version: Option<u64>,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Running migrations for {:?}", db_path);

    let mut state = load_migration_state(db_path)?;
    let manager = create_migration_manager();

    let pending = manager.pending(&state);

    if pending.is_empty() {
        println!("✓ No pending migrations to run.");
        return Ok(());
    }

    // Filter by version if specified
    let to_run: Vec<_> = if let Some(max_version) = to_version {
        pending
            .into_iter()
            .filter(|m| m.version <= max_version)
            .collect()
    } else {
        pending
    };

    if dry_run {
        println!("Dry run - would apply {} migration(s):", to_run.len());
        for migration in &to_run {
            println!("  v{}: {}", migration.version, migration.name);
        }
        return Ok(());
    }

    println!("Running {} migration(s)...", to_run.len());

    let result = manager.run_pending(&mut state)?;

    if result.failed_count > 0 {
        println!("\n⚠ Migration failed!");
        for m in &result.migrations {
            if !m.success {
                println!("  Failed: v{} - {}", m.version, m.name);
                if let Some(err) = &m.error {
                    println!("    Error: {}", err);
                }
            }
        }
        return Err("Migration failed".into());
    }

    println!(
        "\n✓ Successfully applied {} migration(s)",
        result.applied_count
    );
    println!("  Final version: {}", result.final_version);

    // Save updated state
    save_migration_state(db_path, &state)?;

    Ok(())
}

/// Load migration state from database.
fn load_migration_state(db_path: &Path) -> Result<MigrationState, Box<dyn std::error::Error>> {
    let state_file = db_path.join("MIGRATIONS");

    if state_file.exists() {
        let data = std::fs::read_to_string(&state_file)?;
        let state: MigrationStateFile = serde_json::from_str(&data)?;
        Ok(MigrationState {
            current_version: state.current_version,
            applied: state
                .applied
                .into_iter()
                .map(|m| entidb_core::AppliedMigration {
                    version: m.version,
                    name: m.name,
                    applied_at: m.applied_at,
                })
                .collect(),
        })
    } else {
        Ok(MigrationState::new())
    }
}

/// Save migration state to database.
fn save_migration_state(
    db_path: &Path,
    state: &MigrationState,
) -> Result<(), Box<dyn std::error::Error>> {
    let state_file = db_path.join("MIGRATIONS");

    let file_state = MigrationStateFile {
        current_version: state.current_version,
        applied: state
            .applied
            .iter()
            .map(|m| AppliedMigrationFile {
                version: m.version,
                name: m.name.clone(),
                applied_at: m.applied_at,
            })
            .collect(),
    };

    let data = serde_json::to_string_pretty(&file_state)?;
    std::fs::write(&state_file, data)?;

    Ok(())
}

/// Create migration manager with registered migrations.
fn create_migration_manager() -> MigrationManager {
    // In a real application, migrations would be registered here
    // For the CLI, we create an empty manager
    MigrationManager::new()
}

fn format_timestamp(ms: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let datetime = UNIX_EPOCH + Duration::from_millis(ms);
    if let Ok(duration) = datetime.duration_since(UNIX_EPOCH) {
        let secs = duration.as_secs();
        format!("{} seconds since epoch", secs)
    } else {
        format!("{} ms", ms)
    }
}

// File format for migration state
#[derive(serde::Serialize, serde::Deserialize)]
struct MigrationStateFile {
    current_version: u64,
    applied: Vec<AppliedMigrationFile>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AppliedMigrationFile {
    version: u64,
    name: String,
    applied_at: u64,
}
