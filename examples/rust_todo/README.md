# EntiDB Rust Todo Example

A simple todo application demonstrating core EntiDB functionality.

## Requirements

- Rust 1.75+

## Running

```bash
cargo run
```

## Features Demonstrated

- Opening and closing a database
- Defining entities with `Entity` and `EntityCodec` traits
- CRUD operations (Create, Read, Update, Delete)
- Transactions with closures
- Filtering using Rust iterators (`filter`, `map`, `collect`)
- Partitioning data with `partition`
- No SQL - pure Rust data manipulation
