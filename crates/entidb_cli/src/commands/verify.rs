//! Verify command implementation.

use entidb_storage::{FileBackend, StorageBackend};
use std::path::Path;

/// Verification result.
#[derive(Debug)]
pub struct VerifyResult {
    /// Number of records checked.
    pub records_checked: usize,
    /// Number of valid records.
    pub valid_records: usize,
    /// Number of corrupt records.
    pub corrupt_records: usize,
    /// List of errors found.
    pub errors: Vec<String>,
}

impl VerifyResult {
    fn new() -> Self {
        Self {
            records_checked: 0,
            valid_records: 0,
            corrupt_records: 0,
            errors: Vec::new(),
        }
    }

    fn is_ok(&self) -> bool {
        self.corrupt_records == 0 && self.errors.is_empty()
    }
}

/// Runs the verify command.
pub fn run(
    path: &Path,
    check_wal: bool,
    check_segments: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Verifying database at {:?}", path);
    println!();

    let mut wal_result = VerifyResult::new();
    let mut segment_result = VerifyResult::new();

    if check_wal {
        let wal_path = path.join("wal.log");
        if wal_path.exists() {
            println!("Checking WAL...");
            let backend = FileBackend::open(&wal_path)?;
            wal_result = verify_wal(&backend)?;
            print_result("WAL", &wal_result);
        } else {
            println!("WAL file not found (this may be normal for new databases)");
        }
    }

    if check_segments {
        let segment_path = path.join("segments.dat");
        if segment_path.exists() {
            println!("Checking segments...");
            let backend = FileBackend::open(&segment_path)?;
            segment_result = verify_segments(&backend)?;
            print_result("Segments", &segment_result);
        } else {
            println!("Segment file not found (this may be normal for new databases)");
        }
    }

    println!();
    if wal_result.is_ok() && segment_result.is_ok() {
        println!("✓ Database verification passed");
        Ok(())
    } else {
        println!("✗ Database verification failed");
        Err("Verification failed".into())
    }
}

fn verify_wal(backend: &dyn StorageBackend) -> Result<VerifyResult, Box<dyn std::error::Error>> {
    let mut result = VerifyResult::new();
    let size = backend.size()?;
    let mut offset = 0u64;

    while offset + 11 < size {
        result.records_checked += 1;

        // Read header
        let header = match backend.read_at(offset, 11) {
            Ok(h) => h,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to read header at {}: {}", offset, e));
                result.corrupt_records += 1;
                break;
            }
        };

        // Check magic
        if &header[0..4] != b"ENTW" {
            result.errors.push(format!(
                "Invalid magic at offset {}: expected ENTW, got {:?}",
                offset,
                &header[0..4]
            ));
            result.corrupt_records += 1;
            break;
        }

        // Get version
        let version = u16::from_le_bytes([header[4], header[5]]);
        if version != 1 {
            result.errors.push(format!(
                "Unsupported version at offset {}: {}",
                offset, version
            ));
        }

        // Get payload length
        let len = u32::from_le_bytes([header[7], header[8], header[9], header[10]]) as u64;

        // Verify we can read the full record
        let record_size = 11 + len + 4;
        if offset + record_size > size {
            result.errors.push(format!(
                "Truncated record at offset {}: needs {} bytes, only {} available",
                offset,
                record_size,
                size - offset
            ));
            result.corrupt_records += 1;
            break;
        }

        // Read full record and verify CRC
        let record_data = backend.read_at(offset, record_size as usize)?;
        let stored_crc = u32::from_le_bytes([
            record_data[(record_size - 4) as usize],
            record_data[(record_size - 3) as usize],
            record_data[(record_size - 2) as usize],
            record_data[(record_size - 1) as usize],
        ]);
        let computed_crc = compute_crc32(&record_data[..(record_size - 4) as usize]);

        if stored_crc != computed_crc {
            result.errors.push(format!(
                "CRC mismatch at offset {}: stored={:08x}, computed={:08x}",
                offset, stored_crc, computed_crc
            ));
            result.corrupt_records += 1;
        } else {
            result.valid_records += 1;
        }

        offset += record_size;
    }

    Ok(result)
}

fn verify_segments(
    backend: &dyn StorageBackend,
) -> Result<VerifyResult, Box<dyn std::error::Error>> {
    let mut result = VerifyResult::new();
    let size = backend.size()?;
    let mut offset = 0u64;

    while offset + 4 < size {
        result.records_checked += 1;

        // Read length
        let len_bytes = match backend.read_at(offset, 4) {
            Ok(b) => b,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to read length at {}: {}", offset, e));
                result.corrupt_records += 1;
                break;
            }
        };
        let record_len =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as u64;

        if record_len == 0 {
            result
                .errors
                .push(format!("Zero-length record at offset {}", offset));
            result.corrupt_records += 1;
            break;
        }

        if offset + record_len > size {
            result.errors.push(format!(
                "Truncated record at offset {}: needs {} bytes, only {} available",
                offset,
                record_len,
                size - offset
            ));
            result.corrupt_records += 1;
            break;
        }

        // Read full record
        let data = backend.read_at(offset, record_len as usize)?;

        // Verify minimum size: 4 (len) + 4 (collection) + 16 (entity) + 1 (flags) + 4 (checksum)
        if data.len() < 29 {
            result.errors.push(format!(
                "Record too small at offset {}: {} bytes",
                offset,
                data.len()
            ));
            result.corrupt_records += 1;
        } else {
            // Verify checksum
            let stored_crc = u32::from_le_bytes([
                data[data.len() - 4],
                data[data.len() - 3],
                data[data.len() - 2],
                data[data.len() - 1],
            ]);
            let computed_crc = compute_crc32(&data[..data.len() - 4]);

            if stored_crc != computed_crc {
                result.errors.push(format!(
                    "CRC mismatch at offset {}: stored={:08x}, computed={:08x}",
                    offset, stored_crc, computed_crc
                ));
                result.corrupt_records += 1;
            } else {
                result.valid_records += 1;
            }
        }

        offset += record_len;
    }

    Ok(result)
}

fn print_result(name: &str, result: &VerifyResult) {
    println!(
        "  {} records checked: {}, valid: {}, corrupt: {}",
        name, result.records_checked, result.valid_records, result.corrupt_records
    );
    for error in &result.errors {
        println!("    ERROR: {}", error);
    }
}

/// CRC32 computation (same as in entidb_core).
fn compute_crc32(data: &[u8]) -> u32 {
    const CRC32_TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut j = 0;
            while j < 8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
                j += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };

    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let index = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}
