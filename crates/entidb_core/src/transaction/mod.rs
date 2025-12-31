//! Transaction management with ACID guarantees.
//!
//! EntiDB provides ACID transactions with:
//! - **Atomicity**: All-or-nothing commits
//! - **Consistency**: Internal invariants preserved
//! - **Isolation**: Snapshot isolation (readers don't see uncommitted changes)
//! - **Durability**: Committed transactions survive crashes
//!
//! ## Single-Writer Guarantee
//!
//! EntiDB enforces single-writer semantics. Use `begin_write()` to start a
//! write transaction, which acquires an exclusive lock for its lifetime.
//! Multiple read-only transactions can run concurrently with snapshot isolation.

mod manager;
mod state;

pub use manager::TransactionManager;
pub use state::{compute_content_hash, PendingWrite, Transaction, WriteTransaction};
