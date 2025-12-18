//! # EntiDB Sync Server
//!
//! Reference HTTP sync server for EntiDB.
//!
//! This crate provides:
//! - HTTP endpoints (handshake, pull, push)
//! - Server oplog persistence
//! - Authentication middleware
//! - Conflict detection
//!
//! # Architecture
//!
//! The sync server uses the same EntiDB core as clients (no external database).
//! It maintains:
//! - A server-side oplog of all operations
//! - Current cursor position for each device
//! - Authentication state
//!
//! # Protocol
//!
//! The server implements pull-then-push synchronization:
//! 1. Client handshakes with device credentials
//! 2. Client pulls changes since last cursor
//! 3. Client pushes local changes
//! 4. Server detects conflicts and applies policy

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod error;
mod handler;
mod oplog;
mod server;

pub use config::ServerConfig;
pub use error::{ServerError, ServerResult};
pub use handler::{HandlerContext, RequestHandler};
pub use oplog::ServerOplog;
pub use server::SyncServer;
