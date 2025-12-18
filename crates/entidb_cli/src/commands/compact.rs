//! Compact command implementation.

use entidb_storage::{FileBackend, StorageBackend};
use std::collections::HashMap;
use std::path::Path;

/// Compaction statistics.
#[derive(Debug)]
pub struct CompactStats {
    /// Input records.
    pub input_records: usize,
    /// Output records.
    pub output_records: usize,
    /// Tombstones removed.
    pub tombstones_removed: usize,
    /// Obsolete versions removed.
    pub obsolete_removed: usize,
    /// Bytes before compaction.
    pub bytes_before: u64,
    /// Bytes after compaction.
    pub bytes_after: u64,
}

/// Runs the compact command.
pub fn run(
    path: &Path,
    remove_tombstones: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let segment_path = path.join("segments.dat");

    if !segment_path.exists() {
        return Err("Segment file not found".into());
    }

    println!("Compacting segments at {:?}", path);
    if dry_run {
        println!("(dry run - no changes will be made)");
    }
    println!();

    let backend = FileBackend::open(&segment_path)?;
    let stats = analyze_compaction(&backend, remove_tombstones)?;

    println!("Compaction Analysis:");
    println!("  Input records:     {}", stats.input_records);
    println!("  Output records:    {}", stats.output_records);
    println!("  Tombstones:        {} (will be {})", 
        stats.tombstones_removed,
        if remove_tombstones { "removed" } else { "kept" }
    );
    println!("  Obsolete versions: {}", stats.obsolete_removed);
    println!();
    println!("  Size before: {} bytes", stats.bytes_before);
    println!("  Size after:  {} bytes", stats.bytes_after);
    println!(
        "  Space saved: {} bytes ({:.1}%)",
        stats.bytes_before - stats.bytes_after,
        if stats.bytes_before > 0 {
            ((stats.bytes_before - stats.bytes_after) as f64 / stats.bytes_before as f64) * 100.0
        } else {
            0.0
        }
    );

    if !dry_run {
        if stats.output_records < stats.input_records {
            println!();
            println!("Performing compaction...");
            perform_compaction(&segment_path, remove_tombstones)?;
            println!("✓ Compaction complete");
        } else {
            println!();
            println!("No compaction needed - database is already optimal");
        }
    }

    Ok(())
}

fn analyze_compaction(
    backend: &dyn StorageBackend,
    remove_tombstones: bool,
) -> Result<CompactStats, Box<dyn std::error::Error>> {
    let size = backend.size()?;
    let mut offset = 0u64;

    let mut input_records = 0;
    let mut tombstone_count = 0;
    let mut obsolete_count = 0;

    // Track latest version per entity
    let mut latest: HashMap<(u32, [u8; 16]), (u64, bool, usize)> = HashMap::new();

    while offset + 4 < size {
        let len_bytes = backend.read_at(offset, 4)?;
        let record_len =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as u64;

        if offset + record_len > size {
            break;
        }

        let data = backend.read_at(offset, record_len as usize)?;

        if data.len() >= 29 {
            let collection_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let mut entity_id = [0u8; 16];
            entity_id.copy_from_slice(&data[8..24]);
            let flags = data[24];
            let is_tombstone = flags & 0x01 != 0;

            // Parse sequence number (8 bytes after flags)
            let seq = if data.len() >= 33 {
                u64::from_le_bytes([
                    data[25], data[26], data[27], data[28],
                    data[29], data[30], data[31], data[32],
                ])
            } else {
                input_records as u64
            };

            let key = (collection_id, entity_id);

            match latest.get(&key) {
                Some(&(existing_seq, _, _)) if seq > existing_seq => {
                    obsolete_count += 1;
                    latest.insert(key, (seq, is_tombstone, record_len as usize));
                }
                Some(_) => {
                    obsolete_count += 1;
                }
                None => {
                    latest.insert(key, (seq, is_tombstone, record_len as usize));
                }
            }

            if is_tombstone {
                tombstone_count += 1;
            }
        }

        input_records += 1;
        offset += record_len;
    }

    // Calculate output size
    let mut output_records = 0;
    let mut output_size = 0u64;

    for (_, (_, is_tombstone, record_size)) in &latest {
        if *is_tombstone && remove_tombstones {
            continue;
        }
        output_records += 1;
        output_size += *record_size as u64;
    }

    let tombstones_in_output = if remove_tombstones { 0 } else {
        latest.values().filter(|(_, is_ts, _)| *is_ts).count()
    };

    Ok(CompactStats {
        input_records,
        output_records,
        tombstones_removed: tombstone_count - tombstones_in_output,
        obsolete_removed: obsolete_count,
        bytes_before: size,
        bytes_after: output_size,
    })
}

fn perform_compaction(
    segment_path: &Path,
    remove_tombstones: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let backend = FileBackend::open(segment_path)?;
    let size = backend.size()?;

    // Collect all records
    let mut records: Vec<(u32, [u8; 16], u64, bool, Vec<u8>)> = Vec::new();
    let mut offset = 0u64;

    while offset + 4 < size {
        let len_bytes = backend.read_at(offset, 4)?;
        let record_len =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as u64;

        if offset + record_len > size {
            break;
        }

        let data = backend.read_at(offset, record_len as usize)?;

        if data.len() >= 29 {
            let collection_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let mut entity_id = [0u8; 16];
            entity_id.copy_from_slice(&data[8..24]);
            let flags = data[24];
            let is_tombstone = flags & 0x01 != 0;

            let seq = if data.len() >= 33 {
                u64::from_le_bytes([
                    data[25], data[26], data[27], data[28],
                    data[29], data[30], data[31], data[32],
                ])
            } else {
                records.len() as u64
            };

            records.push((collection_id, entity_id, seq, is_tombstone, data));
        }

        offset += record_len;
    }

    // Keep only latest version of each entity
    let mut latest: HashMap<(u32, [u8; 16]), (u64, bool, Vec<u8>)> = HashMap::new();

    for (collection_id, entity_id, seq, is_tombstone, data) in records {
        let key = (collection_id, entity_id);
        match latest.get(&key) {
            Some((existing_seq, _, _)) if seq > *existing_seq => {
                latest.insert(key, (seq, is_tombstone, data));
            }
            None => {
                latest.insert(key, (seq, is_tombstone, data));
            }
            _ => {}
        }
    }

    // Write compacted output to temp file
    let temp_path = segment_path.with_extension("compact");
    {
        let mut temp_file = std::fs::File::create(&temp_path)?;

        for (_, (_, is_tombstone, data)) in &latest {
            if *is_tombstone && remove_tombstones {
                continue;
            }
            temp_file.write_all(data)?;
        }

        temp_file.sync_all()?;
    }

    // Replace original with compacted
    drop(backend);
    std::fs::rename(&temp_path, segment_path)?;

    Ok(())
}
