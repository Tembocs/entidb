//! # EntiDB Sync Protocol
//!
//! Sync protocol types and CBOR codecs for EntiDB.
//!
//! This crate provides:
//! - `SyncOperation` for replication records
//! - `Conflict` for conflict detection
//! - Protocol messages (Handshake, Pull, Push)
//! - CBOR encoding/decoding
//!
//! This is a pure protocol crate with no I/O operations.

#![deny(unsafe_code)]
#![warn(missing_docs)]

// TODO: Implement in Phase 7
