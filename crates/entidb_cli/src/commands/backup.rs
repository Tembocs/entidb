//! Backup and restore commands.

use entidb_core::{BackupConfig, BackupManager, BackupMetadata};
use entidb_storage::FileBackend;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use tracing::info;

/// Create a backup of the database.
pub fn create(
    db_path: &Path,
    output_path: &Path,
    include_tombstones: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Creating backup of {:?}", db_path);

    // Open segments
    let segment_path = db_path.join("SEGMENTS");
    if !segment_path.exists() {
        return Err(format!("Segments not found at {:?}", segment_path).into());
    }

    let segment_backend = FileBackend::open(&segment_path)?;
    let segment_manager = entidb_core::SegmentManager::new(
        Box::new(segment_backend),
        64 * 1024 * 1024, // 64MB max segment size
    );

    // Create backup
    let config = BackupConfig {
        include_tombstones,
        compress: false,
    };
    let manager = BackupManager::new(config);

    // Get current sequence (read from WAL if available)
    let current_seq = entidb_core::SequenceNumber::new(0);
    let result = manager.create_backup(&segment_manager, current_seq)?;

    // Write to output file
    let mut file = fs::File::create(output_path)?;
    file.write_all(&result.data)?;
    file.sync_all()?;

    println!("✓ Backup created successfully");
    println!("  Path: {:?}", output_path);
    println!("  Size: {} bytes", result.metadata.size);
    println!("  Records: {}", result.metadata.record_count);
    println!(
        "  Timestamp: {}",
        format_timestamp(result.metadata.timestamp)
    );

    Ok(())
}

/// Restore database from a backup.
pub fn restore(
    db_path: &Path,
    input_path: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Restoring database from {:?}", input_path);

    // Check if database already exists
    let segment_path = db_path.join("SEGMENTS");
    if segment_path.exists() && !force {
        return Err("Database already exists. Use --force to overwrite.".into());
    }

    // Read backup file
    let mut file = fs::File::open(input_path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    // Validate and restore
    let manager = BackupManager::with_defaults();
    let result = manager.restore_from_backup(&data)?;

    // Create database directory
    fs::create_dir_all(db_path)?;
    fs::create_dir_all(&segment_path)?;

    // Write restored records to new segment file
    let segment_file = segment_path.join("seg-000001.dat");
    let mut segment = fs::File::create(&segment_file)?;

    for record in &result.records {
        let encoded = record.encode()?;
        segment.write_all(&encoded)?;
    }
    segment.sync_all()?;

    println!("✓ Database restored successfully");
    println!("  Path: {:?}", db_path);
    println!("  Records restored: {}", result.records.len());
    println!(
        "  From backup created: {}",
        format_timestamp(result.metadata.timestamp)
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
