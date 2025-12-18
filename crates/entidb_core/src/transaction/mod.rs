//! Transaction management with ACID guarantees.
//!
//! EntiDB provides ACID transactions with:
//! - **Atomicity**: All-or-nothing commits
//! - **Consistency**: Internal invariants preserved
//! - **Isolation**: Snapshot isolation (readers don't see uncommitted changes)
//! - **Durability**: Committed transactions survive crashes

mod manager;
mod state;

pub use manager::TransactionManager;
pub use state::{PendingWrite, Transaction};
