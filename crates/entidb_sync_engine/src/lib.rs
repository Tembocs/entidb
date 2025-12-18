//! # EntiDB Sync Engine
//!
//! Sync state machine and engine for EntiDB.
//!
//! This crate provides:
//! - Sync state machine (idle → pulling → pushing → synced)
//! - Cursor management
//! - Conflict detection
//! - Retry with exponential backoff

#![deny(unsafe_code)]
#![warn(missing_docs)]

// TODO: Implement in Phase 7
