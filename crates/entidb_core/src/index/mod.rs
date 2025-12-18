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
//! - `HashIndex`: O(1) equality lookup
//! - `BTreeIndex`: Ordered traversal and range queries
//!
//! # Warning
//!
//! Users do NOT reference indexes by name during queries.
//! Indexes are internal optimization structures.

mod btree;
mod hash;
mod traits;

pub use btree::BTreeIndex;
pub use hash::HashIndex;
pub use traits::{Index, IndexKey, IndexSpec};
