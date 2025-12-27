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
//! ## Invariants
//!
//! - WAL is **append-only** - records are never modified after write
//! - WAL is **flushed before commit acknowledgment**
//! - Recovery replays only **committed** transactions
//! - Replay is **idempotent** - multiple replays produce same state

mod iterator;
mod record;
mod writer;

pub use iterator::{StreamingRecovery, WalRecordIterator};
pub use record::{compute_crc32, WalRecord, WalRecordType};
pub use writer::WalManager;
