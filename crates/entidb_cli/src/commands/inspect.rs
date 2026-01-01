//! Inspect command implementation.

use entidb_storage::{FileBackend, StorageBackend};
use serde::Serialize;
use std::path::Path;

/// Database inspection result.
#[derive(Debug, Serialize)]
pub struct InspectResult {
    /// Database path.
    pub path: String,
    /// WAL file size in bytes.
    pub wal_size: u64,
    /// Segment file size in bytes.
    pub segment_size: u64,
    /// Total size in bytes.
    pub total_size: u64,
    /// Number of WAL records.
    pub wal_record_count: usize,
    /// Number of segment records.
    pub segment_record_count: usize,
    /// Number of live entities.
    pub entity_count: usize,
    /// Number of tombstones.
    pub tombstone_count: usize,
    /// Collection statistics (if requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<CollectionStats>>,
}

/// Statistics for a single collection.
#[derive(Debug, Serialize)]
pub struct CollectionStats {
    /// Collection ID.
    pub id: u32,
    /// Number of entities.
    pub entity_count: usize,
    /// Total data size in bytes.
    pub data_size: usize,
}

/// Runs the inspect command.
pub fn run(
    path: &Path,
    show_collections: bool,
    show_segments: bool,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let wal_path = path.join("wal.log");
    let segment_path = path.join("segments.dat");

    // Check if files exist
    if !wal_path.exists() && !segment_path.exists() {
        return Err(format!("No database found at {:?}", path).into());
    }

    let mut result = InspectResult {
        path: path.display().to_string(),
        wal_size: 0,
        segment_size: 0,
        total_size: 0,
        wal_record_count: 0,
        segment_record_count: 0,
        entity_count: 0,
        tombstone_count: 0,
        collections: None,
    };

    // Get WAL stats
    if wal_path.exists() {
        let wal_backend = FileBackend::open(&wal_path)?;
        result.wal_size = wal_backend.size()?;
        result.wal_record_count = count_wal_records(&wal_backend)?;
    }

    // Get segment stats
    if segment_path.exists() {
        let segment_backend = FileBackend::open(&segment_path)?;
        result.segment_size = segment_backend.size()?;
        let (records, entities, tombstones, collections) =
            analyze_segments(&segment_backend, show_collections)?;
        result.segment_record_count = records;
        result.entity_count = entities;
        result.tombstone_count = tombstones;
        if show_collections {
            result.collections = Some(collections);
        }
    }

    result.total_size = result.wal_size + result.segment_size;

    // Output
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            print_text_output(&result, show_segments);
        }
    }

    Ok(())
}

fn count_wal_records(backend: &dyn StorageBackend) -> Result<usize, Box<dyn std::error::Error>> {
    let size = backend.size()?;
    let mut count = 0;
    let mut offset = 0u64;

    while offset + 11 < size {
        // Read header
        let header = backend.read_at(offset, 11)?;

        // Check magic
        if &header[0..4] != b"ENTW" {
            break;
        }

        // Get payload length
        let len = u32::from_le_bytes([header[7], header[8], header[9], header[10]]) as u64;

        // Record = header (11) + payload + crc (4)
        let record_size = 11 + len + 4;
        offset += record_size;
        count += 1;
    }

    Ok(count)
}

fn analyze_segments(
    backend: &dyn StorageBackend,
    collect_stats: bool,
) -> Result<(usize, usize, usize, Vec<CollectionStats>), Box<dyn std::error::Error>> {
    use std::collections::HashMap;

    let size = backend.size()?;
    let mut record_count = 0;
    let mut entity_count = 0;
    let mut tombstone_count = 0;
    let mut offset = 0u64;

    // Track latest version per entity
    let mut entities: HashMap<(u32, [u8; 16]), (bool, usize)> = HashMap::new();
    let mut collection_stats: HashMap<u32, (usize, usize)> = HashMap::new();

    while offset + 4 < size {
        let len_bytes = backend.read_at(offset, 4)?;
        let record_len =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as u64;

        if offset + record_len > size {
            break;
        }

        let data = backend.read_at(offset, record_len as usize)?;

        // Parse minimal record info
        if data.len() >= 26 {
            // 4 (len) + 4 (collection) + 16 (entity_id) + 1 (flags) + 1 (min payload)
            let collection_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let mut entity_id = [0u8; 16];
            entity_id.copy_from_slice(&data[8..24]);
            let flags = data[24];
            let is_tombstone = flags & 0x01 != 0;
            let payload_size = data.len() - 29; // Approximate

            let key = (collection_id, entity_id);
            entities.insert(key, (is_tombstone, payload_size));

            if collect_stats {
                let entry = collection_stats.entry(collection_id).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += payload_size;
            }
        }

        record_count += 1;
        offset += record_len;
    }

    // Count live entities vs tombstones
    for (_, (is_tombstone, _)) in &entities {
        if *is_tombstone {
            tombstone_count += 1;
        } else {
            entity_count += 1;
        }
    }

    let collections: Vec<CollectionStats> = collection_stats
        .into_iter()
        .map(|(id, (count, size))| CollectionStats {
            id,
            entity_count: count,
            data_size: size,
        })
        .collect();

    Ok((record_count, entity_count, tombstone_count, collections))
}

fn print_text_output(result: &InspectResult, _show_segments: bool) {
    println!("EntiDB Database Inspection");
    println!("==========================");
    println!();
    println!("Path: {}", result.path);
    println!();
    println!("Storage:");
    println!("  WAL size:      {} bytes", format_size(result.wal_size));
    println!(
        "  Segment size:  {} bytes",
        format_size(result.segment_size)
    );
    println!("  Total size:    {} bytes", format_size(result.total_size));
    println!();
    println!("Records:");
    println!("  WAL records:     {}", result.wal_record_count);
    println!("  Segment records: {}", result.segment_record_count);
    println!();
    println!("Entities:");
    println!("  Live entities: {}", result.entity_count);
    println!("  Tombstones:    {}", result.tombstone_count);

    if let Some(collections) = &result.collections {
        println!();
        println!("Collections:");
        for col in collections {
            println!(
                "  [{}] {} entities, {} bytes",
                col.id, col.entity_count, col.data_size
            );
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
