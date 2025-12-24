//! Index implementations for access paths.
//!
//! EntiDB provides indexes as internal access paths, not as query DSLs.
//! Indexes are:
//! - Declared via typed API calls
//! - Maintained atomically with transactions
//! - Fully derivable from segments + WAL
//!
//! # Index Types
//!
//! - [`HashIndex`]: O(1) equality lookup
//! - [`BTreeIndex`]: Ordered traversal and range queries
//! - [`FtsIndex`]: Full-text search with token matching (Phase 2)
//!
//! # Warning
//!
//! Users do NOT reference indexes by name during queries.
//! Indexes are internal optimization structures.

mod btree;
mod fts;
mod hash;
mod traits;

pub use btree::BTreeIndex;
pub use fts::{FtsIndex, FtsIndexSpec, TokenizerConfig};
pub use hash::HashIndex;
pub use traits::{Index, IndexKey, IndexSpec};
