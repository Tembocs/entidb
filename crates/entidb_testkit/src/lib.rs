//! # EntiDB Testkit
//!
//! Test utilities for EntiDB.
//!
//! This crate provides:
//! - Test fixtures and database helpers
//! - Property-based test generators using proptest
//! - Golden test utilities for format verification
//! - Cross-crate integration test helpers
//! - Fuzz testing harnesses
//! - Stress testing utilities
//! - Cross-language test vectors
//!
//! ## Usage
//!
//! ```rust,ignore
//! use entidb_testkit::prelude::*;
//!
//! #[test]
//! fn test_with_database() {
//!     with_temp_db(|db| {
//!         let collection = db.collection("test");
//!         // ... test operations
//!     });
//! }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod fixtures;
pub mod fuzz;
pub mod generators;
pub mod golden;
pub mod integration;
pub mod stress;
pub mod vectors;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::fixtures::*;
    pub use crate::fuzz::*;
    pub use crate::generators::*;
    pub use crate::integration::*;
    pub use crate::stress::*;
    pub use crate::vectors::*;
}

pub use fixtures::*;
pub use fuzz::*;
pub use generators::*;
pub use golden::*;
pub use integration::*;
pub use stress::*;
pub use vectors::*;
