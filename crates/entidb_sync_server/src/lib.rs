//! # EntiDB Sync Server
//!
//! Reference HTTP sync server for EntiDB.
//!
//! This crate provides:
//! - HTTP endpoints (handshake, pull, push)
//! - Server oplog persistence
//! - Authentication middleware (HMAC-SHA256 tokens)
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
//! # Authentication
//!
//! Authentication is optional but recommended for production:
//!
//! ```rust,ignore
//! use entidb_sync_server::{ServerConfig, AuthConfig, TokenValidator};
//!
//! let secret = b"my-secure-secret-32-bytes-long!".to_vec();
//! let config = ServerConfig::default().with_auth(secret.clone());
//!
//! // Create tokens for devices
//! let auth_config = AuthConfig::new(secret);
//! let validator = TokenValidator::new(auth_config);
//! let token = validator.create_token(device_id, db_id);
//! ```
//!
//! # Protocol
//!
//! The server implements pull-then-push synchronization:
//! 1. Client handshakes with device credentials (+ auth token if enabled)
//! 2. Client pulls changes since last cursor
//! 3. Client pushes local changes
//! 4. Server detects conflicts and applies policy

#![deny(unsafe_code)]
#![warn(missing_docs)]
// Production code MUST NOT use panic!/unwrap()/expect() - see docs/invariants.md
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod auth;
mod config;
mod error;
mod handler;
mod oplog;
mod server;

pub use auth::{AuthConfig, SimpleTokenValidator, TokenValidator};
pub use config::ServerConfig;
pub use error::{ServerError, ServerResult};
pub use handler::{HandlerContext, RequestHandler};
pub use oplog::ServerOplog;
pub use server::SyncServer;
