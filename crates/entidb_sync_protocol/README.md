# entidb_sync_protocol

Sync protocol types and CBOR codecs for EntiDB.

## Overview

This crate defines the sync protocol message types and their canonical CBOR
serialization for EntiDB's offline-first synchronization system.

## Features

- **Protocol types**: Complete set of sync operation types (Put, Delete, Tombstone)
- **CBOR codecs**: Canonical encoding/decoding for all message types
- **Version vectors**: Clock-based conflict detection primitives
- **Cursor management**: Server and client cursor types

## Design Principles

- **Pure types**: No I/O, no networking - just types and codecs
- **Canonical encoding**: Deterministic CBOR for consistent hashing
- **Language-agnostic**: Can be reimplemented in any binding language

## Protocol Operations

```rust
use entidb_sync_protocol::{SyncOperation, OperationType};

// Operations are canonical CBOR-encoded
let op = SyncOperation::new(
    entity_id,
    OperationType::Put,
    Some(entity_bytes),
    clock,
);
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
