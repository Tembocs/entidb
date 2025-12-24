//! Index implementations for access paths.
//!
//! EntiDB provides indexes as internal access paths, not as query DSLs.
//! Indexes are:
//! - Declared via typed API calls
//! - Maintained atomically with transactions
//! - Fully derivable from segments + WAL
//! - Persistable to disk for fast reopening
//!
//! # Index Types
//!
//! - [`HashIndex`]: O(1) equality lookup
//! - [`BTreeIndex`]: Ordered traversal and range queries
//! - [`FtsIndex`]: Full-text search with token matching (Phase 2)
//!
//! # Composite Keys
//!
//! For multi-field indexes, use [`CompositeKey2`] or [`CompositeKey3`]:
//!
//! ```rust,ignore
//! use entidb_core::index::{BTreeIndex, CompositeKey2, IndexSpec};
//!
//! // Index on (last_name, first_name)
//! let mut index: BTreeIndex<CompositeKey2<String, String>> = BTreeIndex::new(spec);
//! ```
//!
//! # Persistence
//!
//! Indexes can be persisted to disk using the `persistence` module.
//! This allows fast database reopening without expensive rebuilds.
//!
//! # Warning
//!
//! Users do NOT reference indexes by name during queries.
//! Indexes are internal optimization structures.

mod btree;
mod composite;
mod fts;
mod hash;
pub mod persistence;
mod traits;

pub use btree::BTreeIndex;
pub use composite::{CompositeKey2, CompositeKey3};
pub use fts::{FtsIndex, FtsIndexSpec, TokenizerConfig};
pub use hash::HashIndex;
pub use persistence::{IndexHeader, IndexType};
pub use traits::{Index, IndexKey, IndexSpec};
