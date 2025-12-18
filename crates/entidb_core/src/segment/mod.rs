//! Segment storage for entities.
//!
//! Segments are immutable, append-only files that store entity records.
//! Once sealed, segments are never modified.
//!
//! ## Segment Record Format
//!
//! ```text
//! | record_len (4) | collection_id (4) | entity_id (16) | flags (1) | payload (N) | checksum (4) |
//! ```
//!
//! Flags:
//! - `0x01` = tombstone (deleted entity)
//! - `0x02` = encrypted

mod compaction;
mod record;
mod store;

pub use compaction::{CompactionConfig, CompactionResult, Compactor};
pub use record::{Segment, SegmentRecord};
pub use store::SegmentManager;

