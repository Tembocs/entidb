# entidb_testkit

Test utilities for EntiDB.

## Overview

This crate provides comprehensive testing utilities for EntiDB, including property-based
testing, fuzz harnesses, golden tests, and test vector validation.

## Features

- **Property testing**: Proptest strategies for all core types
- **Test vectors**: JSON-based cross-language test vectors
- **Fuzzing harnesses**: Corpus-based fuzzing for codec and storage
- **Temporary databases**: Helpers for creating test databases

## Test Vectors

The crate includes validation against the canonical test vectors in
[docs/test_vectors/](../../docs/test_vectors/):

- `cbor.json` - Canonical CBOR encoding vectors
- `entity_id.json` - Entity ID generation and parsing
- `segment.json` - Segment record format
- `wal.json` - WAL record format

## Usage

```rust
use entidb_testkit::{TestDatabase, arb_entity_id};

// Create a temporary test database
let db = TestDatabase::new();

// Use proptest strategies
proptest! {
    fn test_entity_roundtrip(id in arb_entity_id()) {
        // ...
    }
}
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
