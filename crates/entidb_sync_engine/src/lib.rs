//! # EntiDB Sync Engine
//!
//! Sync state machine and engine for EntiDB.
//!
//! This crate provides:
//! - Sync state machine (idle → pulling → pushing → synced)
//! - Cursor management
//! - Conflict detection and resolution
//! - Retry with exponential backoff
//! - HTTP transport abstraction
//! - Database-backed sync applier
//!
//! ## Architecture
//!
//! The sync engine implements a **pull-then-push** synchronization model:
//! 1. Pull remote changes first (server is authoritative)
//! 2. Apply remote changes locally
//! 3. Push local changes to server
//!
//! The sync server uses the **same EntiDB core** as clients, ensuring
//! consistent storage semantics everywhere.
//!
//! ## Key Invariants
//!
//! - Server is authoritative
//! - Pull always happens before push
//! - Operations are idempotent
//! - Sync operations are atomic per batch
//! - Server uses EntiDB for persistence (no external database)

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod db_applier;
mod error;
mod http;
mod state;
mod transport;

pub use config::{RetryConfig, SyncConfig};
pub use db_applier::DatabaseApplier;
pub use error::{SyncError, SyncResult};
pub use http::{CborDecode, CborEncode, HttpClient, HttpTransport, LoopbackClient, LoopbackServer};
pub use state::{MemorySyncApplier, SyncApplier, SyncCycleResult, SyncEngine, SyncState, SyncStats};
pub use transport::{MockTransport, SyncRequest, SyncResponse, SyncTransport};
