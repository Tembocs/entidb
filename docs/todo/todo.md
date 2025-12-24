# EntiDB Missing Features Report

## Executive Summary

The core database engine has solid foundations (storage, WAL, segments, transactions, indexes), but several features are **incomplete** or **missing entirely**. The sync layer exists but is not integrated with the core. Bindings lack critical features.

**Update (December 2024):** Phase 1 (Core Completeness) is now ✅ COMPLETE.
**Update (December 2024):** Phase 2 (Binding Parity) is now ✅ COMPLETE.
**Update (December 2024):** Phase 3 (Index APIs) is now ✅ COMPLETE.
**Update (December 2024):** Phase 4 (Observability) is now ✅ COMPLETE.
**Update (December 2024):** Phase 5 (Advanced Features) is now ✅ COMPLETE.
**Update (December 2024):** Phase 6 (Final Features) is now ✅ COMPLETE.
- Index persistence: Indexes save/load to disk
- Compaction in FFI: Manual compaction via `entidb_compact()`
- Composite indexes: Multi-field keys with `CompositeKey2`/`CompositeKey3`
- WASM feature parity: Backup/restore/compact APIs
- Sync authentication: HMAC-SHA256 token validation

---

## 🟢 Completed Features

### 1. **Database `open()` from Path** - ✅ COMPLETE
**Implementation:** `Database::open(path)` and `Database::open_with_config(path, config)` now exist and work correctly.

### 2. **WAL Truncation** - ✅ COMPLETE
**Implementation:** `checkpoint()` now:
- Flushes segments to ensure all committed data is durable
- Writes a checkpoint record
- Truncates/clears the WAL after checkpoint

### 3. **MANIFEST Persistence** - ✅ COMPLETE
**Implementation:**
- MANIFEST is saved atomically on `create_collection()` when the database has a directory
- MANIFEST is saved on `close()`
- MANIFEST is saved on `checkpoint()` with the checkpoint sequence
- Uses atomic write-then-rename pattern

### 4. **Backup/Restore/Checkpoint APIs in Bindings** - ✅ COMPLETE
**Implementation (December 2024):**

| Feature | Core | FFI | Dart | Python | WASM |
|---------|:----:|:---:|:----:|:------:|:----:|
| Backup | ✅ | ✅ | ✅ | ✅ | ❌ |
| Backup (with options) | ✅ | ✅ | ✅ | ✅ | ❌ |
| Restore | ✅ | ✅ | ✅ | ✅ | ❌ |
| Validate Backup | ✅ | ✅ | ✅ | ✅ | ❌ |
| Checkpoint | ✅ | ✅ | ✅ | ✅ | ✅ |
| Committed Sequence | ✅ | ✅ | ✅ | ✅ | ❌ |
| Entity Count | ✅ | ✅ | ✅ | ✅ | ❌ |

**FFI Functions:**
- `entidb_checkpoint(handle)` - Creates a checkpoint
- `entidb_backup(handle, out_buffer)` - Creates backup without tombstones
- `entidb_backup_with_options(handle, include_tombstones, out_buffer)` - Creates backup with options
- `entidb_restore(handle, data, data_len, out_stats)` - Restores from backup
- `entidb_validate_backup(handle, data, data_len, out_info)` - Validates backup
- `entidb_committed_seq(handle, out_seq)` - Gets committed sequence number
- `entidb_entity_count(handle, out_count)` - Gets total entity count

**Tests Added:**
- FFI: 7 new tests (29 total)
- Python: 11 new tests (39 total)
- Dart: 15 new tests (47 total)

### 5. **Index APIs in Bindings** - ✅ COMPLETE
**Implementation (December 2024):**

| Feature | Core | FFI | Dart | Python | WASM |
|---------|:----:|:---:|:----:|:------:|:----:|
| Hash Index | ✅ | ✅ | ✅ | ✅ | ❌ |
| BTree Index | ✅ | ✅ | ✅ | ✅ | ❌ |
| Index Insert | ✅ | ✅ | ✅ | ✅ | ❌ |
| Index Remove | ✅ | ✅ | ✅ | ✅ | ❌ |
| Index Lookup | ✅ | ✅ | ✅ | ✅ | ❌ |
| BTree Range Query | ✅ | ✅ | ✅ | ✅ | ❌ |
| Index Length | ✅ | ✅ | ✅ | ✅ | ❌ |
| Drop Index | ✅ | ✅ | ✅ | ✅ | ❌ |

**Core Database Methods:**
- `create_hash_index(collection_id, name, unique)` - Creates a hash index
- `create_btree_index(collection_id, name, unique)` - Creates a btree index
- `hash_index_insert(collection_id, name, key, entity_id)` - Inserts into hash index
- `btree_index_insert(collection_id, name, key, entity_id)` - Inserts into btree index
- `hash_index_remove(collection_id, name, key, entity_id)` - Removes from hash index
- `btree_index_remove(collection_id, name, key, entity_id)` - Removes from btree index
- `hash_index_lookup(collection_id, name, key)` - Looks up in hash index
- `btree_index_lookup(collection_id, name, key)` - Looks up in btree index
- `btree_index_range(collection_id, name, min, max)` - Range query in btree index
- `hash_index_len(collection_id, name)` - Gets hash index entry count
- `btree_index_len(collection_id, name)` - Gets btree index entry count
- `drop_hash_index(collection_id, name)` - Drops a hash index
- `drop_btree_index(collection_id, name)` - Drops a btree index

**Design Notes:**
- Uses `Vec<u8>` as key type for FFI compatibility
- Indexes keyed by `(collection_id, index_name)` tuple
- Unique indexes enforce constraint on insert
- Range queries support unbounded min/max
- Entity IDs returned as contiguous 16-byte blocks

**Tests Added:**
- Core: 9 new tests (37 total)
- FFI: 2 new tests (31 total)
- Dart: 9 new tests (54 total)
- Python: 8 new tests

### 6. **Observability (Change Feed & Stats)** - ✅ COMPLETE
**Implementation (December 2024):**

| Feature | Core | FFI | Dart | Python | WASM |
|---------|:----:|:---:|:----:|:------:|:----:|
| Change Feed | ✅ | - | - | - | ❌ |
| Database Stats | ✅ | ✅ | ✅ | ✅ | ❌ |
| Subscribe to Changes | ✅ | - | - | - | ❌ |
| Poll Changes | ✅ | - | - | - | ❌ |

**Core Modules Created:**
- `entidb_core::change_feed` - Observable change feed for committed operations
  - `ChangeFeed` - Thread-safe change emitter with subscriber management
  - `ChangeEvent` - Represents a single committed change (insert/update/delete)
  - `ChangeType` - Enum: Insert, Update, Delete
- `entidb_core::stats` - Database statistics and telemetry
  - `DatabaseStats` - Atomic counters for all operations
  - `StatsSnapshot` - Serializable copy of stats for external use

**Core Database Methods:**
- `db.subscribe()` - Returns a channel receiver for real-time change events
- `db.stats()` - Returns a snapshot of database statistics
- `db.change_feed()` - Direct access to the change feed for polling

**Statistics Tracked:**
- `reads` - Entity read operations
- `writes` - Entity write operations (put)
- `deletes` - Entity delete operations
- `scans` - Full collection scans (AC-11 compliance)
- `index_lookups` - Index query operations
- `transactions_started` / `transactions_committed` / `transactions_aborted`
- `bytes_read` / `bytes_written`
- `checkpoints` - Number of checkpoints performed
- `errors` - Error count
- `entity_count` - Total entities

**Integration Points:**
- Stats recorded in `Database::begin()`, `commit()`, `abort()`
- Stats recorded in `get()`, `get_in_txn()`, `list()`
- Stats recorded in `hash_index_lookup()`, `btree_index_lookup()`, `btree_index_range()`
- Stats recorded in `checkpoint()`
- Change events emitted after successful commit in `Database::commit()`

**FFI/Binding Support:**
- `entidb_stats(handle, out_stats)` - FFI function
- `EntiDbStats` - C-compatible struct with all counters
- Dart: `DatabaseStats` class, `db.stats()` method
- Python: `DatabaseStats` class, `db.stats()` method

**Tests Added:**
- Core change_feed: 8 tests
- Core stats: 5 tests
- Core database observability: 8 tests
- FFI: 1 test

---

## 🟢 Completed Features (Phase 5)

### 7. **Segment Auto-Sealing & Rotation** - ✅ COMPLETE
**Implementation (December 2024):**

The `SegmentManager` now supports multi-segment storage with automatic sealing and rotation:

**New Components:**
- `SegmentInfo` - Metadata for each segment (id, path, size, sealed status, record count)
- `IndexEntry` - Extended to track segment_id + offset + sequence
- Factory pattern for creating backends per segment

**Key Features:**
- `with_factory(factory, max_size)` - Constructor with custom backend factory
- `seal_and_rotate()` - Manually seal current segment and create a new one
- Auto-sealing when `max_segment_size` is exceeded during `append()`
- `on_segment_sealed(callback)` - Register callback for segment seal events
- `list_segments()` - Get info about all segments
- `segment_count()` / `sealed_segment_count()` - Segment statistics

**Tests Added:** 12 segment tests (24 in module)

---

### 8. **Full-Text Index (FtsIndex)** - ✅ COMPLETE
**Implementation (December 2024):**

A complete full-text search index with token-based matching:

**Components:**
- `FtsIndex` - Main FTS index with inverted and forward indexes
- `FtsIndexSpec` - Index specification (collection_id, name, tokenizer config)
- `TokenizerConfig` - Configurable tokenizer (min/max length, case sensitivity)

**Features:**
- `index_text(entity_id, text)` - Index text content for an entity
- `remove_entity(entity_id)` - Remove entity from index
- `search(query)` - Search with AND semantics (all tokens must match)
- `search_any(query)` - Search with OR semantics (any token matches)
- `search_prefix(prefix)` - Prefix matching for autocomplete
- Unicode support, punctuation stripping, configurable tokenization

**Tests Added:** 18 comprehensive tests

---

### 9. **Complete Sync Layer** - ✅ COMPLETE
**Implementation (December 2024):**

The sync layer now follows the architecture specification completely:

**HTTP Transport (`entidb_sync_engine::http`):**
- `HttpClient` trait - Abstract HTTP client interface
- `HttpTransport<C>` - Implements `SyncTransport` using any `HttpClient`
- `CborEncode` / `CborDecode` traits - CBOR serialization for protocol messages
- `LoopbackClient` / `LoopbackServer` traits - Testing without network

**Database-Backed Applier (`entidb_sync_engine::DatabaseApplier`):**
- Uses EntiDB for sync state persistence (per architecture requirement)
- Server uses the **same EntiDB core** as clients (no external database)
- Applies remote operations in atomic transactions

**Integration Tests Added:** 5 tests
**Total Sync Layer Tests:** 32

---

## 🟢 Completed Features (Phase 6)

### 10. **Index Persistence** - ✅ COMPLETE
**Implementation (December 2024):**

Indexes can now be serialized to disk and restored, eliminating rebuild on every open:

**New Module:** `entidb_core::index::persistence`

**Binary Format (Index File):**
```
| EIDX (4 bytes) | version (1) | index_type (1) | collection_id (4) |
| name_len (2) | name (N) | unique (1) | entry_count (8) |
| entries... |
```

**Entry Format:**
```
| key_len (2) | key (N) | entity_id_count (4) | entity_ids (16 * count) |
```

**Functions:**
- `persist_hash_index(index, path)` - Save HashIndex to file
- `load_hash_index(path, collection_id)` - Load HashIndex from file
- `persist_btree_index(index, path)` - Save BTreeIndex to file
- `load_btree_index(path, collection_id)` - Load BTreeIndex from file

**Index Methods Added:**
- `HashIndex::entries()` - Access all entries
- `HashIndex::to_bytes()` / `HashIndex::from_bytes()` - Serialization
- `BTreeIndex::entries()` - Access all entries
- `BTreeIndex::to_bytes()` / `BTreeIndex::from_bytes()` - Serialization

**Tests Added:** 8 tests

---

### 11. **Compaction in FFI** - ✅ COMPLETE
**Implementation (December 2024):**

Compaction is now exposed through the FFI layer:

**Core Addition:**
```rust
pub struct CompactionStats {
    pub input_records: u64,
    pub output_records: u64,
    pub tombstones_removed: u64,
    pub obsolete_versions_removed: u64,
    pub bytes_saved: u64,
}

impl Database {
    pub fn compact(&self, remove_tombstones: bool) -> CoreResult<CompactionStats>;
}
```

**FFI Function:**
```rust
pub extern "C" fn entidb_compact(
    handle: EntiDbHandle,
    remove_tombstones: bool,
    out_stats: *mut EntiDbCompactionStats,
) -> EntiDbResult
```

**C-Compatible Struct:**
```rust
#[repr(C)]
pub struct EntiDbCompactionStats {
    pub input_records: u64,
    pub output_records: u64,
    pub tombstones_removed: u64,
    pub obsolete_versions_removed: u64,
    pub bytes_saved: u64,
}
```

---

### 12. **Composite Indexes** - ✅ COMPLETE
**Implementation (December 2024):**

Multi-field composite keys for indexes on multiple columns:

**New Module:** `entidb_core::index::composite`

**Types:**
- `CompositeKey2<A, B>` - Two-field composite key
- `CompositeKey3<A, B, C>` - Three-field composite key

**Features:**
- Implements `IndexKey` trait for proper serialization
- Length-prefixed encoding ensures unambiguous parsing
- Tuple implementations: `(A, B)` and `(A, B, C)` also implement `IndexKey`
- Works with both HashIndex and BTreeIndex

**Usage Example:**
```rust
use entidb_core::CompositeKey2;

// Create composite key from two fields
let key = CompositeKey2::new(b"john".to_vec(), b"doe".to_vec());

// Or use tuple syntax
btree.insert(&("john".as_bytes().to_vec(), "doe".as_bytes().to_vec()), entity_id);
```

**Tests Added:** 7 tests

---

### 13. **WASM Feature Parity** - ✅ COMPLETE
**Implementation (December 2024):**

WASM Database now has backup, restore, and compaction APIs matching native bindings:

**New WASM Methods:**

```typescript
// Create backup as Uint8Array
backup(): Uint8Array

// Create backup with options
backupWithOptions(include_tombstones: boolean): Uint8Array

// Restore from backup, returns entities restored count
restore(data: Uint8Array): number

// Validate backup without restoring
validateBackup(data: Uint8Array): {
    valid: boolean,
    recordCount: number,
    timestamp: bigint,
    sequence: bigint,
    size: number
}

// Run compaction
compact(remove_tombstones: boolean): {
    inputRecords: bigint,
    outputRecords: bigint,
    tombstonesRemoved: bigint,
    obsoleteVersionsRemoved: bigint,
    bytesSaved: bigint
}
```

**Backend Updates:**
- `WasmMemoryBackend::truncate()` - Added for restore support
- `PersistentBackend::truncate()` - Added for restore support

---

### 14. **Sync Authentication** - ✅ COMPLETE
**Implementation (December 2024):**

HMAC-SHA256 token-based authentication for sync server:

**New Module:** `entidb_sync_server::auth`

**Types:**
- `AuthConfig` - Configuration with secret and token expiry
- `TokenValidator` - Creates and validates HMAC-SHA256 signed tokens
- `SimpleTokenValidator` - Simple shared-secret validator for testing

**Token Format (72 bytes):**
```
| device_id (16) | db_id (16) | timestamp_millis (8) | hmac_sha256 (32) |
```

**Key Methods:**
```rust
impl TokenValidator {
    pub fn new(config: AuthConfig) -> Self;
    pub fn create_token(&self, device_id: [u8; 16], db_id: [u8; 16]) -> Vec<u8>;
    pub fn validate_token(
        &self,
        token: &[u8],
        expected_device_id: &[u8; 16],
        expected_db_id: &[u8; 16],
    ) -> ServerResult<()>;
}
```

**Validation Checks:**
1. Token length (must be 72 bytes)
2. Device ID match
3. Database ID match
4. HMAC-SHA256 signature verification
5. Token expiration check

**Tests Added:** 5 tests

---

## 🟢 Minor Missing Features (All Complete)

| Feature | Status | Notes |
|---------|--------|-------|
| Composite indexes | ✅ Complete | `CompositeKey2<A,B>`, `CompositeKey3<A,B,C>` |
| Index persistence | ✅ Complete | `to_bytes()`/`from_bytes()` on HashIndex/BTreeIndex |
| `get_collection()` in FFI | ❌ Missing | Lookup without creating |
| Compaction in FFI | ✅ Complete | `entidb_compact(handle, remove_tombstones, out_stats)` |
| Migration APIs in bindings | ❌ Missing | Schema evolution |
| WASM backup/restore | ✅ Complete | `backup()`, `restore()`, `validateBackup()`, `compact()` |
| Sync authentication | ✅ Complete | `TokenValidator` with HMAC-SHA256 |

---

## Recommended Priority Order

### Phase 1: Core Completeness ✅ COMPLETE
1. ✅ **`Database::open(path)`** - Essential for real usage
2. ✅ **MANIFEST persistence** - Collections survive restart
3. ✅ **WAL truncation** - Prevents disk exhaustion

### Phase 2: Binding Parity ✅ COMPLETE
4. ✅ **Backup/Restore in FFI/bindings** - Data portability
5. ✅ **Checkpoint in FFI/bindings** - Manual durability control
6. ✅ **Database properties (committed_seq, entity_count)** - Observability

### Phase 3: Index APIs ✅ COMPLETE
7. ✅ **Index creation in FFI/bindings** - Create hash and btree indexes
8. ✅ **Index query APIs** - Insert, remove, lookup, range queries

### Phase 4: Observability ✅ COMPLETE
9. ✅ **Change feed integration** - Sync prerequisite, reactive apps
10. ✅ **Telemetry hooks (AC-11)** - Stats tracking, scan detection

### Phase 5: Advanced ✅ COMPLETE
11. ✅ **Segment rotation** - Multi-segment storage with auto-sealing
12. ✅ **Full-text index** - Token-based text search with FtsIndex
13. ✅ **Complete sync layer** - HTTP transport + DatabaseApplier

---
