//! Segment storage for entities.
//!
//! Segments are immutable, append-only files that store entity records.
//! Once sealed, segments are never modified.
//!
//! ## Segment Record Format
//!
//! ```text
//! | record_len (4) | collection_id (4) | entity_id (16) | flags (1) | sequence (8) | payload (N) | checksum (4) |
//! ```
//!
//! Fields:
//! - `sequence` (8 bytes): Commit sequence number; latest wins during compaction.
//!
//! Flags:
//! - `0x01` = tombstone (deleted entity)
//! - `0x02` = encrypted
//!
//! ## Segment Auto-Sealing & Rotation
//!
//! The [`SegmentManager`] automatically seals segments when they exceed
//! `max_segment_size` and creates new segments for writes. This ensures:
//! - Individual segments remain manageable in size
//! - Sealed segments can be backed up or replicated independently
//! - Compaction can be performed on sealed segments

mod compaction;
mod record;
mod store;

pub use compaction::{CompactionConfig, CompactionResult, Compactor};
pub use record::{Segment, SegmentRecord};
#[allow(unused_imports)]
pub use store::{SegmentInfo, SegmentManager};

