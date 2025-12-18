//! # EntiDB FFI
//!
//! Stable C ABI for EntiDB bindings (Dart, Python).
//!
//! This crate provides:
//! - C-compatible function exports
//! - Memory ownership conventions
//! - Error code mapping
//! - Buffer management

#![deny(unsafe_code)] // Will need to allow unsafe for FFI
#![warn(missing_docs)]

// TODO: Implement in Phase 8
