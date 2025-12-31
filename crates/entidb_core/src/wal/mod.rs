//! Write-Ahead Log (WAL) for durability and crash recovery.
//!
//! The WAL is the foundation of EntiDB's durability guarantees. All mutations
//! are written to the WAL before being acknowledged. On crash, the WAL is
//! replayed to recover committed transactions.
//!
//! ## WAL Record Format
//!
//! ```text
//! | magic (4) | version (2) | type (1) | length (4) | payload (N) | crc32 (4) |
//! ```
//!
//! ## Streaming Replay
//!
//! For memory efficiency, WAL replay uses streaming iteration:
//!
//! ```ignore
//! for result in wal.iter()? {
//!     let (offset, record) = result?;
//!     // Process record without loading entire WAL into memory
//! }
//! ```
//!
//! ## Recovery Policy
//!
//! The WAL iterator distinguishes between **tolerated** and **fatal** conditions
//! during recovery:
//!
//! ### Tolerated Conditions (treat as clean end-of-log)
//!
//! - **Truncated header**: Fewer than 11 bytes available at end → `Ok(None)`
//! - **Truncated payload**: Record length exceeds available bytes → `Ok(None)`
//!
//! These represent crashes mid-write before fsync completed. The incomplete
//! record is discarded and recovery proceeds with earlier complete records.
//! Any uncommitted transaction whose COMMIT record was not fully written
//! will be rolled back.
//!
//! ### Fatal Conditions (abort open with error)
//!
//! - **CRC mismatch**: Stored checksum doesn't match computed → `Err(ChecksumMismatch)`
//! - **Invalid magic bytes**: Not 0xDB_ED_01_01 → `Err(WalCorruption)`
//! - **Unknown record type**: Unrecognized type byte → `Err(WalCorruption)`
//! - **Unsupported version**: Future format version → `Err(WalCorruption)`
//!
//! These indicate actual data corruption (bit rot, storage failure, or
//! malicious modification) and the database MUST NOT open to prevent
//! silent data loss.
//!
//! ## Invariants
//!
//! - WAL is **append-only** - records are never modified after write
//! - WAL is **flushed before commit acknowledgment**
//! - Recovery replays only **committed** transactions
//! - Replay is **idempotent** - multiple replays produce same state
//! - CRC failures are **fatal** - no heuristic repair attempted

mod iterator;
mod record;
mod writer;

pub use iterator::{StreamingRecovery, WalRecordIterator};
pub use record::{compute_crc32, WalRecord, WalRecordType};
pub use writer::WalManager;
