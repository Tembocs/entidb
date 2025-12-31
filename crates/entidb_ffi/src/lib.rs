//! # EntiDB FFI
//!
//! Stable C ABI for EntiDB bindings (Dart, Python).
//!
//! This crate provides:
//! - C-compatible function exports
//! - Memory ownership conventions
//! - Error code mapping
//! - Buffer management
//!
//! ## Memory Ownership
//!
//! - Rust owns all allocated buffers
//! - Bindings must call `entidb_free_*` functions to release memory
//! - Strings are null-terminated UTF-8
//! - Byte buffers use (ptr, len) pairs
//!
//! ## Error Handling
//!
//! All functions return `EntiDbResult` with error codes.
//! Use `entidb_get_last_error()` for detailed error messages.
//!
//! ## Transaction API
//!
//! Use `entidb_txn_begin`, `entidb_txn_put`, `entidb_txn_delete`,
//! `entidb_txn_commit`, `entidb_txn_abort` for explicit transactions.
//!
//! ## Iteration API
//!
//! Use `entidb_iter_create`, `entidb_iter_has_next`, `entidb_iter_next`,
//! `entidb_iter_free` to iterate over entities.
//!
//! ## Backup/Restore API
//!
//! Use `entidb_checkpoint`, `entidb_backup`, `entidb_restore`,
//! `entidb_validate_backup` for backup and restore operations.
//!
//! ## Encryption API
//!
//! Use `entidb_crypto_create`, `entidb_crypto_encrypt`, `entidb_crypto_decrypt`
//! for encryption operations. Requires the `encryption` feature.

#![warn(missing_docs)]
// Production code MUST NOT use panic!/unwrap()/expect() - see docs/invariants.md
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod buffer;
mod crypto;
mod database;
mod error;
mod iterator;
mod transaction;
mod types;

pub use buffer::{EntiDbBuffer, EntiDbString};
pub use crypto::*;
pub use database::*;
pub use error::{EntiDbResult, ErrorCode};
pub use iterator::*;
pub use transaction::*;
pub use types::*;
