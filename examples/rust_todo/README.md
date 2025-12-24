# EntiDB Rust Todo Example

A simple todo application demonstrating core EntiDB functionality.

## Requirements

- Rust 1.75+

## Running

```bash
cargo run -p rust_todo
```

## Features Demonstrated

- Opening an in-memory database
- CBOR encoding/decoding for entities
- CRUD operations (Create, Read, Update, Delete)
- Transactions with closures (`db.transaction(|txn| { ... })`)
- Filtering using Rust iterators (`filter`, `map`, `collect`)
- Partitioning data with `partition`
- **No SQL** - pure Rust data manipulation

## Key Concepts

### Entity Encoding

Entities are stored as canonical CBOR bytes. The example shows how to encode/decode
using `entidb_codec::Encoder` and `Decoder`.

### Transactions

```rust
db.transaction(|txn| {
    txn.put(collection_id, entity_id, encoded_bytes)?;
    Ok(())
})?;
```

### Filtering with Rust Iterators

```rust
let urgent: Vec<&Todo> = all_todos
    .iter()
    .filter(|t| !t.completed && t.priority == 1)
    .collect();
```
