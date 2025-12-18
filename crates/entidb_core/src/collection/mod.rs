//! Typed collection API.
//!
//! Provides `Collection<T>` for type-safe entity storage with automatic
//! CBOR encoding/decoding via the `EntityCodec` trait.

mod codec;
mod typed;

pub use codec::EntityCodec;
pub use typed::Collection;
