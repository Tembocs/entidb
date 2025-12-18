//! Dump oplog command implementation.

use entidb_storage::{FileBackend, StorageBackend};
use serde::Serialize;
use std::path::Path;

/// WAL record representation for output.
#[derive(Debug, Serialize)]
pub struct WalRecordInfo {
    /// Offset in the WAL file.
    pub offset: u64,
    /// Record type.
    pub record_type: String,
    /// Transaction ID (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<u64>,
    /// Collection ID (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_id: Option<u32>,
    /// Entity ID (if applicable, hex-encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// Sequence number (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// Payload size in bytes (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<usize>,
}

/// Runs the dump-oplog command.
pub fn run(
    path: &Path,
    limit: Option<usize>,
    start_offset: u64,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let wal_path = path.join("wal.log");

    if !wal_path.exists() {
        return Err("WAL file not found".into());
    }

    let backend = FileBackend::open(&wal_path)?;
    let records = read_wal_records(&backend, start_offset, limit)?;

    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        _ => {
            print_text_output(&records);
        }
    }

    Ok(())
}

fn read_wal_records(
    backend: &dyn StorageBackend,
    start_offset: u64,
    limit: Option<usize>,
) -> Result<Vec<WalRecordInfo>, Box<dyn std::error::Error>> {
    let size = backend.size()?;
    let mut offset = start_offset;
    let mut records = Vec::new();
    let max_records = limit.unwrap_or(usize::MAX);

    while offset + 11 < size && records.len() < max_records {
        // Read header
        let header = backend.read_at(offset, 11)?;

        // Check magic
        if &header[0..4] != b"ENTW" {
            break;
        }

        // Get record type
        let record_type_byte = header[6];
        let record_type = match record_type_byte {
            0x01 => "BEGIN",
            0x02 => "PUT",
            0x03 => "DELETE",
            0x04 => "COMMIT",
            0x05 => "ABORT",
            0x06 => "CHECKPOINT",
            _ => "UNKNOWN",
        };

        // Get payload length
        let len = u32::from_le_bytes([header[7], header[8], header[9], header[10]]) as usize;

        // Read payload
        let payload = if len > 0 {
            backend.read_at(offset + 11, len)?
        } else {
            Vec::new()
        };

        let mut record = WalRecordInfo {
            offset,
            record_type: record_type.to_string(),
            txid: None,
            collection_id: None,
            entity_id: None,
            sequence: None,
            payload_size: None,
        };

        // Parse payload based on record type
        match record_type {
            "BEGIN" => {
                if payload.len() >= 8 {
                    record.txid = Some(u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ]));
                }
            }
            "PUT" => {
                if payload.len() >= 28 {
                    record.txid = Some(u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ]));
                    record.collection_id = Some(u32::from_le_bytes([
                        payload[8], payload[9], payload[10], payload[11],
                    ]));
                    let entity_bytes = &payload[12..28];
                    record.entity_id = Some(hex_encode(entity_bytes));
                    record.payload_size = Some(len - 28);
                }
            }
            "DELETE" => {
                if payload.len() >= 28 {
                    record.txid = Some(u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ]));
                    record.collection_id = Some(u32::from_le_bytes([
                        payload[8], payload[9], payload[10], payload[11],
                    ]));
                    let entity_bytes = &payload[12..28];
                    record.entity_id = Some(hex_encode(entity_bytes));
                }
            }
            "COMMIT" => {
                if payload.len() >= 16 {
                    record.txid = Some(u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ]));
                    record.sequence = Some(u64::from_le_bytes([
                        payload[8], payload[9], payload[10], payload[11],
                        payload[12], payload[13], payload[14], payload[15],
                    ]));
                }
            }
            "ABORT" => {
                if payload.len() >= 8 {
                    record.txid = Some(u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ]));
                }
            }
            "CHECKPOINT" => {
                if payload.len() >= 8 {
                    record.sequence = Some(u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ]));
                }
            }
            _ => {}
        }

        records.push(record);

        // Move to next record: header (11) + payload + crc (4)
        offset += 11 + len as u64 + 4;
    }

    Ok(records)
}

fn print_text_output(records: &[WalRecordInfo]) {
    println!("WAL Records ({} total)", records.len());
    println!("================");
    println!();

    for record in records {
        print!("[{:08}] {:10}", record.offset, record.record_type);

        if let Some(txid) = record.txid {
            print!(" txid={}", txid);
        }
        if let Some(seq) = record.sequence {
            print!(" seq={}", seq);
        }
        if let Some(cid) = record.collection_id {
            print!(" collection={}", cid);
        }
        if let Some(ref eid) = record.entity_id {
            print!(" entity={}...", &eid[..8.min(eid.len())]);
        }
        if let Some(size) = record.payload_size {
            print!(" payload={} bytes", size);
        }

        println!();
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
