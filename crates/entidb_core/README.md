# entidb_core

Core database engine for EntiDB - an embedded entity database with ACID transactions.

## Overview

`entidb_core` implements the complete durable storage engine:

```
┌─────────────────────────────────────────────────────────────────┐
│                         Database                                 │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │                  TransactionManager                      │   │
│   │   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   │   │
│   │   │ WalManager  │   │SegmentMgr  │   │  Manifest   │   │   │
│   │   └─────────────┘   └─────────────┘   └─────────────┘   │   │
│   └─────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              ▼                                   │
│                    ┌─────────────────┐                          │
│                    │ StorageBackend  │                          │
│                    └─────────────────┘                          │
└─────────────────────────────────────────────────────────────────┘
```

## Features

- **ACID Transactions**: Full atomicity, consistency, isolation, durability
- **Write-Ahead Log (WAL)**: Crash recovery with committed transaction replay
- **Segment Storage**: Immutable, append-only segments with checksums
- **Single-Writer Concurrency**: Mutex-protected write path, snapshot isolation for readers
- **Entity-First API**: 128-bit EntityId, collection-based organization
- **Pluggable Storage**: Works with any `StorageBackend` implementation

## Quick Start

```rust
use entidb_core::{Database, Config, EntityId, CollectionId};
use entidb_storage::InMemoryBackend;

// Open database with in-memory backends
let db = Database::open_in_memory(Config::default())?;

// Get or create a collection
let users = db.collection("users")?;

// Write in a transaction
db.transaction(|txn| {
    let id = EntityId::new();
    txn.put(users, id, b"user data".to_vec())?;
    Ok(())
})?;

// Read data
let entities = db.list(users)?;
```

## Key Types

| Type | Description |
|------|-------------|
| `Database` | Main entry point with recovery and transaction API |
| `TransactionManager` | ACID transaction coordinator |
| `WalManager` | Write-ahead log for durability |
| `SegmentManager` | Immutable segment storage with index |
| `EntityId` | 128-bit globally unique entity identifier |
| `CollectionId` | Numeric collection identifier |

## WAL Format

```
| magic (4) | version (2) | type (1) | length (4) | payload (N) | crc32 (4) |
```

Record types: `BEGIN`, `PUT`, `DELETE`, `COMMIT`, `ABORT`, `CHECKPOINT`

## Segment Format

```
| record_len (4) | collection_id (4) | entity_id (16) | flags (1) | sequence (8) | payload (N) | checksum (4) |
```

Flags: `0x01` = tombstone, `0x02` = encrypted (reserved)

## Invariants

1. **WAL-first writes**: All mutations written to WAL before segment
2. **Commit requires flush**: WAL must be flushed before commit acknowledgment
3. **Single writer**: Only one write transaction active at a time
4. **Snapshot isolation**: Readers see consistent snapshots
5. **Recovery correctness**: Only committed transactions replayed

## Testing

```bash
cargo test -p entidb_core
```

87 tests covering:
- WAL record serialization and recovery
- Segment append, index, and tombstone handling
- Transaction lifecycle and isolation
- Database operations and crash recovery

## License

See repository root.
