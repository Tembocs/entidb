//! # EntiDB Sync Protocol
//!
//! Sync protocol types and CBOR codecs for EntiDB.
//!
//! This crate provides:
//! - `SyncOperation` for replication records
//! - `ChangeFeed` for emitting committed operations
//! - Protocol messages (Handshake, Pull, Push)
//! - CBOR encoding/decoding
//!
//! This is a pure protocol crate with no I/O operations.
//!
//! ## Key Invariants
//!
//! - Change feed emits only committed operations
//! - Change feed preserves commit order
//! - Applying the same operation multiple times is idempotent

#![deny(unsafe_code)]
#![warn(missing_docs)]
// Production code MUST NOT use panic!/unwrap()/expect() - see docs/invariants.md
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod change_feed;
mod conflict;
mod messages;
mod operation;
mod oplog;

pub use change_feed::{ChangeEvent, ChangeFeed, ChangeType};
pub use conflict::{Conflict, ConflictPolicy, ConflictResolution};
pub use messages::{
    HandshakeRequest, HandshakeResponse, PullRequest, PullResponse, PushRequest, PushResponse,
    SyncMessage,
};
pub use operation::{OperationType, SyncOperation};
pub use oplog::{LogicalOplog, OplogEntry};
