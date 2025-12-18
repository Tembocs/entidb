//! # EntiDB Sync Engine
//!
//! Sync state machine and engine for EntiDB.
//!
//! This crate provides:
//! - Sync state machine (idle → pulling → pushing → synced)
//! - Cursor management
//! - Conflict detection and resolution
//! - Retry with exponential backoff
//!
//! ## Architecture
//!
//! The sync engine implements a **pull-then-push** synchronization model:
//! 1. Pull remote changes first (server is authoritative)
//! 2. Apply remote changes locally
//! 3. Push local changes to server
//!
//! ## Key Invariants
//!
//! - Server is authoritative
//! - Pull always happens before push
//! - Operations are idempotent
//! - Sync operations are atomic per batch

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod error;
mod state;
mod transport;

pub use config::{RetryConfig, SyncConfig};
pub use error::{SyncError, SyncResult};
pub use state::{MemorySyncApplier, SyncApplier, SyncCycleResult, SyncEngine, SyncState, SyncStats};
pub use transport::{MockTransport, SyncRequest, SyncResponse, SyncTransport};
