# EntiDB API Reference

This document provides comprehensive API documentation for EntiDB - an embedded entity database engine with ACID transactions and offline-first synchronization.

## Table of Contents

1. [Core Concepts](#core-concepts)
2. [Database API](#database-api)
3. [Transaction API](#transaction-api)
4. [Entity API](#entity-api)
5. [Collection API](#collection-api)
6. [Index API](#index-api)
7. [Backup API](#backup-api)
8. [Sync API](#sync-api)
9. [Storage Backend API](#storage-backend-api)
10. [Error Handling](#error-handling)

---

## Core Concepts

### Entities

Entities are the fundamental unit of data in EntiDB. Each entity:
- Has a unique, immutable `EntityId` (128-bit UUID)
- Belongs to exactly one collection
- Is stored as canonical CBOR bytes
- Has a version tracked by sequence number

### Collections

Collections group related entities:
- Identified by string name (mapped to `CollectionId`)
- Provide type-safe access when using `Collection<T>`
- Support full-table scans and indexed lookups

### Transactions

All mutations happen within ACID transactions:
- **Atomicity**: All-or-nothing commits
- **Consistency**: Invariants preserved
- **Isolation**: Snapshot isolation (readers see consistent state)
- **Durability**: Committed data survives crashes

---

## Database API

### Opening a Database

```rust
use entidb_core::{Database, Config};
use entidb_storage::{InMemoryBackend, FileBackend};

// In-memory database (for testing)
let db = Database::open_in_memory()?;

// File-based database
let wal_backend = FileBackend::open_with_create_dirs("data/wal.log")?;
let segment_backend = FileBackend::open_with_create_dirs("data/segments.dat")?;
let db = Database::open_with_backends(
    Config::default(),
    Box::new(wal_backend),
    Box::new(segment_backend),
)?;
```

### Configuration

```rust
use entidb_core::Config;

let config = Config::builder()
    .max_segment_size(64 * 1024 * 1024)  // 64 MB segments
    .sync_on_commit(true)                 // Durability guarantee
    .build();
```

### Database Methods

| Method | Description |
|--------|-------------|
| `open_in_memory()` | Opens an in-memory database |
| `open_with_backends(config, wal, segments)` | Opens with custom backends |
| `collection(name)` | Gets or creates a collection by name |
| `get_collection(name)` | Gets collection if it exists |
| `get(collection, id)` | Reads an entity (snapshot isolation) |
| `list(collection)` | Lists all entities in a collection |
| `transaction(fn)` | Executes a transaction |
| `checkpoint()` | Creates a recovery checkpoint |
| `close()` | Closes the database |
| `is_open()` | Checks if database is open |
| `entity_count()` | Returns total entity count |
| `committed_seq()` | Returns last committed sequence |

---

## Transaction API

### Basic Transaction

```rust
db.transaction(|txn| {
    txn.put(collection, entity_id, data)?;
    Ok(())
})?;
```

### Transaction Operations

| Method | Description |
|--------|-------------|
| `put(collection, id, data)` | Inserts or updates an entity |
| `delete(collection, id)` | Marks an entity as deleted |
| `get(collection, id)` | Reads an entity within the transaction |

### Error Handling in Transactions

```rust
db.transaction(|txn| {
    txn.put(collection, id1, data1)?;
    
    // If this fails, the entire transaction is aborted
    txn.put(collection, id2, data2)?;
    
    // Explicit abort
    if some_condition {
        return Err(CoreError::InvalidOperation { 
            message: "Validation failed".into() 
        });
    }
    
    Ok(())
})?;
```

### Manual Transaction Control

```rust
let mut txn = db.begin()?;

txn.put(collection, id, data)?;

// Choose to commit or abort
if success {
    db.commit(&mut txn)?;
} else {
    db.abort(&mut txn)?;
}
```

---

## Entity API

### EntityId

```rust
use entidb_core::EntityId;

// Create new random ID
let id = EntityId::new();

// From raw bytes
let id = EntityId::from_bytes([0u8; 16]);

// From UUID
use uuid::Uuid;
let id = EntityId::from_uuid(Uuid::new_v4());

// Access bytes
let bytes: &[u8; 16] = id.as_bytes();

// Convert to UUID
let uuid: Uuid = id.to_uuid();
```

### Entity Codec

Implement `EntityCodec` for type-safe entities:

```rust
use entidb_core::EntityCodec;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
}

impl EntityCodec for User {
    fn encode(&self) -> Vec<u8> {
        // Use CBOR encoding
        let mut encoder = entidb_codec::CanonicalEncoder::new();
        // ... encode fields
        encoder.into_bytes()
    }
    
    fn decode(bytes: &[u8]) -> Result<Self, entidb_core::CoreError> {
        // Decode from CBOR
        // ...
    }
}
```

---

## Collection API

### Typed Collections

```rust
use entidb_core::Collection;

let users: Collection<User> = Collection::new(&db, "users");

// Insert
db.transaction(|txn| {
    users.put(txn, &user)?;
    Ok(())
})?;

// Get
let user = users.get(entity_id)?;

// Scan all
for (id, user) in users.scan_all()? {
    println!("{}: {}", id, user.name);
}
```

### Raw Collections

```rust
let collection_id = db.collection("raw_data");

db.transaction(|txn| {
    txn.put(collection_id, entity_id, raw_bytes)?;
    Ok(())
})?;

let data = db.get(collection_id, entity_id)?;
```

---

## Index API

### Hash Index (Equality Lookups)

```rust
use entidb_core::index::HashIndex;

let mut index: HashIndex<String, EntityId> = HashIndex::new(false);

// Insert
index.insert("key".to_string(), entity_id);

// Lookup
let ids = index.get(&"key".to_string());

// Remove
index.remove(&"key".to_string(), &entity_id);
```

### BTree Index (Range Queries)

```rust
use entidb_core::index::BTreeIndex;

let mut index: BTreeIndex<i64, EntityId> = BTreeIndex::new(false);

// Insert
index.insert(42, entity_id);

// Range query
for (key, id) in index.range(10..50) {
    // ...
}

// Min/max
let min = index.min_key();
let max = index.max_key();
```

---

## Backup API

### Creating Backups

```rust
use entidb_core::backup::{BackupManager, BackupConfig};

let backup_mgr = BackupManager::new(BackupConfig::default());

// Create backup
let backup_result = backup_mgr.create_backup(&segment_manager)?;

// Save to file
std::fs::write("backup.endb", &backup_result.data)?;
```

### Restoring from Backup

```rust
let backup_data = std::fs::read("backup.endb")?;

// Validate
backup_mgr.validate(&backup_data)?;

// Read metadata
let metadata = backup_mgr.read_metadata(&backup_data)?;
println!("Backup contains {} entities", metadata.record_count);

// Restore
let restore_result = backup_mgr.restore_from_backup(&backup_data)?;
```

---

## Sync API

### Sync Protocol

```rust
use entidb_sync_protocol::{SyncOperation, OperationType};

// Create operations
let op = SyncOperation::new(
    OperationType::Put,
    collection_id,
    entity_id,
    payload,
    sequence,
);
```

### Sync Engine

```rust
use entidb_sync_engine::{SyncEngine, SyncConfig};

let config = SyncConfig::builder()
    .server_url("https://sync.example.com")
    .device_id(device_id)
    .build();

let engine = SyncEngine::new(config);

// Perform sync
engine.sync().await?;
```

### Change Feed

```rust
use entidb_sync_protocol::ChangeFeed;

let mut feed = ChangeFeed::new();

// Poll for changes
let events = feed.poll(cursor, limit)?;

for event in events {
    match event.operation_type {
        OperationType::Put => { /* handle put */ }
        OperationType::Delete => { /* handle delete */ }
    }
}
```

---

## Storage Backend API

### Implementing Custom Backends

```rust
use entidb_storage::{StorageBackend, StorageResult};

struct CustomBackend { /* ... */ }

impl StorageBackend for CustomBackend {
    fn read_at(&self, offset: u64, len: usize) -> StorageResult<Vec<u8>> {
        // Read bytes at offset
    }
    
    fn append(&mut self, data: &[u8]) -> StorageResult<u64> {
        // Append data, return offset
    }
    
    fn flush(&mut self) -> StorageResult<()> {
        // Ensure data is durable
    }
    
    fn sync(&mut self) -> StorageResult<()> {
        // Force sync to disk
    }
    
    fn size(&self) -> StorageResult<u64> {
        // Return current size
    }
}
```

### Built-in Backends

| Backend | Description |
|---------|-------------|
| `InMemoryBackend` | Memory-based storage (testing) |
| `FileBackend` | File-based persistent storage |

---

## Error Handling

### Error Types

```rust
use entidb_core::CoreError;

match result {
    Err(CoreError::DatabaseClosed) => { /* ... */ }
    Err(CoreError::TransactionAborted { .. }) => { /* ... */ }
    Err(CoreError::WalCorruption { .. }) => { /* ... */ }
    Err(CoreError::SegmentCorruption { .. }) => { /* ... */ }
    Err(CoreError::InvalidOperation { .. }) => { /* ... */ }
    Err(CoreError::StorageError(_)) => { /* ... */ }
    Ok(value) => { /* ... */ }
}
```

### Result Types

```rust
use entidb_core::CoreResult;
use entidb_storage::StorageResult;
use entidb_codec::CodecResult;

// Core operations return CoreResult<T>
fn my_operation() -> CoreResult<()> {
    // ...
}
```

---

## Version Information

- **EntiDB Core**: 2.0.0-alpha.1
- **WAL Format Version**: 1
- **Segment Format Version**: 1
- **Backup Format Version**: 1

---

## See Also

- [Architecture Guide](architecture.md)
- [File Format Specification](file_format.md)
- [Transaction Semantics](transactions.md)
- [Sync Protocol](sync_protocol.md)
