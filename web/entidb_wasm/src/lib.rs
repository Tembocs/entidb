//! # EntiDB WASM
//!
//! WebAssembly bindings for EntiDB with web-native storage backends.
//!
//! This crate provides:
//! - OPFS (Origin Private File System) storage backend for modern browsers
//! - IndexedDB storage backend as fallback for older browsers
//! - JavaScript-friendly API via wasm-bindgen
//!
//! ## Storage Backend Selection
//!
//! The crate automatically selects the best available storage backend:
//! 1. **OPFS** (preferred) - Uses the Origin Private File System API for
//!    file-like access with synchronous operations in a Web Worker
//! 2. **IndexedDB** (fallback) - Uses IndexedDB as a key-value store
//!    when OPFS is not available
//!
//! ## Usage
//!
//! ```javascript
//! import init, { Database, EntityId } from 'entidb_wasm';
//!
//! async function main() {
//!     await init();
//!     
//!     const db = await Database.openMemory();
//!     const users = db.collection("users");
//!     
//!     const id = EntityId.generate();
//!     db.put(users, id, new Uint8Array([1, 2, 3]));
//!     
//!     const data = db.get(users, id);
//!     console.log(data);
//!     
//!     db.close();
//! }
//! ```
//!
//! ## Web Worker Requirement
//!
//! For optimal performance and to use OPFS synchronous access handles,
//! EntiDB should be run inside a Web Worker. The main thread API uses
//! async operations which may have higher latency.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod backend;
mod database;
mod entity;
mod error;
mod utils;

pub use database::*;
pub use entity::*;
pub use error::*;

use wasm_bindgen::prelude::*;

/// Initialize the WASM module.
///
/// This sets up panic hooks for better error messages in the browser console.
#[wasm_bindgen(start)]
pub fn init() {
    utils::set_panic_hook();
}
