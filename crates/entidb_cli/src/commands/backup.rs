//! Backup and restore commands.
//!
//! This module provides CLI commands for backing up and restoring EntiDB databases.
//! It uses the proper Database API rather than directly manipulating storage files,
//! ensuring correct sequence numbers, manifest handling, and ACID guarantees.

use entidb_core::{BackupManager, BackupMetadata, Database};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use tracing::info;

/// Create a backup of the database.
///
/// Opens the database properly using `Database::open()` and uses the built-in
/// `backup()` method to ensure:
/// - Correct sequence number from transaction manager
/// - Consistent point-in-time snapshot
/// - Proper handling of WAL and segments
pub fn create(
    db_path: &Path,
    output_path: &Path,
    include_tombstones: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Creating backup of {:?}", db_path);

    // Open database properly - this handles manifest, WAL recovery, locking, etc.
    let db = Database::open(db_path)?;

    // Create backup using the proper API
    let backup_data = if include_tombstones {
        db.backup_with_options(true)?
    } else {
        db.backup()?
    };

    // Get backup metadata for display
    let manager = BackupManager::with_defaults();
    let metadata = manager.read_metadata(&backup_data)?;

    // Write to output file
    let mut file = fs::File::create(output_path)?;
    file.write_all(&backup_data)?;
    file.sync_all()?;

    // Close database gracefully
    db.close()?;

    println!("✓ Backup created successfully");
    println!("  Path: {:?}", output_path);
    println!("  Size: {} bytes", metadata.size);
    println!("  Records: {}", metadata.record_count);
    println!("  Sequence: {}", metadata.sequence.as_u64());
    println!("  Timestamp: {}", format_timestamp(metadata.timestamp));

    Ok(())
}

/// Restore database from a backup.
///
/// Uses the proper Database API to restore, ensuring:
/// - MANIFEST is created correctly
/// - Entities are imported via proper transactions
/// - WAL and segments are set up correctly
pub fn restore(
    db_path: &Path,
    input_path: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Restoring database from {:?}", input_path);

    // Check if database already exists
    if db_path.exists() && !force {
        let manifest_path = db_path.join("MANIFEST");
        let segments_path = db_path.join("SEGMENTS");
        if manifest_path.exists() || segments_path.exists() {
            return Err("Database already exists. Use --force to overwrite.".into());
        }
    }

    // If force, remove existing database
    if force && db_path.exists() {
        fs::remove_dir_all(db_path)?;
    }

    // Read backup file
    let mut file = fs::File::open(input_path)?;
    let mut backup_data = Vec::new();
    file.read_to_end(&mut backup_data)?;

    // Validate backup first
    let manager = BackupManager::with_defaults();
    if !manager.validate_backup(&backup_data)? {
        return Err("Backup file is invalid or corrupted".into());
    }

    let metadata = manager.read_metadata(&backup_data)?;

    // Open/create a new database at the target path
    let db = Database::open(db_path)?;

    // Restore using the proper API
    let stats = db.restore(&backup_data)?;

    // Close database gracefully
    db.close()?;

    println!("✓ Database restored successfully");
    println!("  Path: {:?}", db_path);
    println!("  Entities restored: {}", stats.entities_restored);
    println!("  Tombstones applied: {}", stats.tombstones_applied);
    println!(
        "  From backup created: {}",
        format_timestamp(metadata.timestamp)
    );

    Ok(())
}

/// Validate a backup file without restoring.
pub fn validate(input_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Validating backup {:?}", input_path);

    // Read backup file
    let mut file = fs::File::open(input_path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    // Validate
    let manager = BackupManager::with_defaults();
    let is_valid = manager.validate_backup(&data)?;

    if is_valid {
        println!("✓ Backup is valid");

        // Also show metadata
        let metadata = manager.read_metadata(&data)?;
        print_metadata(&metadata);

        Ok(())
    } else {
        println!("✗ Backup is invalid or corrupted");
        Err("Backup validation failed".into())
    }
}

/// Show backup metadata.
pub fn info(input_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Reading backup info from {:?}", input_path);

    // Read backup file
    let mut file = fs::File::open(input_path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    // Read metadata
    let manager = BackupManager::with_defaults();
    let metadata = manager.read_metadata(&data)?;

    println!("Backup Information");
    println!("==================");
    print_metadata(&metadata);

    Ok(())
}

fn print_metadata(metadata: &BackupMetadata) {
    println!("  File size: {} bytes", metadata.size);
    println!("  Record count: {}", metadata.record_count);
    println!("  Sequence: {}", metadata.sequence.as_u64());
    println!("  Created: {}", format_timestamp(metadata.timestamp));
}

fn format_timestamp(ms: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let datetime = UNIX_EPOCH + Duration::from_millis(ms);
    if let Ok(duration) = datetime.duration_since(UNIX_EPOCH) {
        let secs = duration.as_secs();
        let hours = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let secs = secs % 60;
        let days = secs / 86400;
        format!(
            "{} days, {:02}:{:02}:{:02} since epoch",
            days, hours, mins, secs
        )
    } else {
        format!("{} ms since epoch", ms)
    }
}
