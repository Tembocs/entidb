//! # EntiDB Core
//!
//! Core database engine for EntiDB - an embedded entity database with
//! ACID transactions, WAL-based durability, and crash recovery.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                    Database                          │
//! │  (public facade: open, close, collection, txn)       │
//! └─────────────────────┬───────────────────────────────┘
//!                       │
//! ┌─────────────────────▼───────────────────────────────┐
//! │               TransactionManager                     │
//! │  (begin, commit, abort, snapshot isolation)          │
//! └─────────────────────┬───────────────────────────────┘
//!                       │
//! ┌─────────────────────▼───────────────────────────────┐
//! │                 EntityStore                          │
//! │  (put, get, delete for raw CBOR payloads)            │
//! └──────────┬──────────────────────────┬───────────────┘
//!            │                          │
//! ┌──────────▼──────────┐    ┌──────────▼───────────────┐
//! │    WalManager       │    │    SegmentManager        │
//! │  (append-only log)  │    │  (immutable segments)    │
//! └──────────┬──────────┘    └──────────┬───────────────┘
//!            │                          │
//! ┌──────────▼──────────────────────────▼───────────────┐
//! │              StorageBackend (trait)                  │
//! │  (opaque byte store: InMemory, File, OPFS)           │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Invariants
//!
//! - **ACID transactions**: All-or-nothing, snapshot isolation, durable after commit
//! - **Single writer**: Only one write transaction active at a time
//! - **WAL-first**: All mutations go to WAL before commit acknowledgment
//! - **Crash recovery**: Database recovers to last committed state after any crash
//!
//! ## Example
//!
//! ```rust,ignore
//! use entidb_core::{Database, Config};
//! use entidb_storage::InMemoryBackend;
//!
//! // Open database
//! let db = Database::open(Config::default(), backend)?;
//!
//! // Execute transaction
//! db.transaction(|txn| {
//!     txn.put("users", entity_id, &user_bytes)?;
//!     Ok(())
//! })?;
//!
//! // Read outside transaction (snapshot)
//! let user = db.get("users", entity_id)?;
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod backup;
mod collection;
mod config;
pub mod crypto;
mod database;
mod entity;
mod error;
mod index;
mod manifest;
mod migration;
mod segment;
mod transaction;
mod types;
mod wal;

pub use backup::{BackupConfig, BackupManager, BackupMetadata, BackupResult, RestoreResult};
pub use collection::{Collection, EntityCodec};
pub use config::Config;
#[cfg(feature = "encryption")]
pub use crypto::{CryptoManager, EncryptionKey};
pub use database::Database;
pub use entity::{EntityId, EntityStore};
pub use error::{CoreError, CoreResult};
pub use index::{BTreeIndex, HashIndex, Index, IndexKey, IndexSpec};
pub use manifest::Manifest;
pub use migration::{
    AppliedMigration, Migration, MigrationContext, MigrationInfo, MigrationManager,
    MigrationOperation, MigrationResult, MigrationRunResult, MigrationState, MigrationVersion,
};
pub use segment::{CompactionConfig, CompactionResult, Compactor, Segment, SegmentManager};
pub use transaction::{Transaction, TransactionManager};
pub use types::{CollectionId, SequenceNumber, TransactionId};
pub use wal::{WalManager, WalRecord, WalRecordType};
