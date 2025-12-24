# EntiDB Missing Features Report

## Executive Summary

The core database engine has solid foundations (storage, WAL, segments, transactions, indexes), but several features are **incomplete** or **missing entirely**. The sync layer exists but is not integrated with the core. Bindings lack critical features.

**Update (December 2024):** Phase 1 (Core Completeness) is now ✅ COMPLETE.
**Update (December 2024):** Phase 2 (Binding Parity) is now ✅ COMPLETE.
**Update (December 2024):** Phase 3 (Index APIs) is now ✅ COMPLETE.
**Update (December 2024):** Phase 4 (Observability) is now ✅ COMPLETE.
**Update (December 2024):** Phase 5 (Advanced Features) is now ✅ COMPLETE.

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

## 🟢 Minor Missing Features

| Feature | Status | Notes |
|---------|--------|-------|
| Composite indexes | ❌ Missing | Multi-field indexes |
| Index persistence | ❌ Missing | Rebuilt on every open |
| `get_collection()` in FFI | ❌ Missing | Lookup without creating |
| Compaction in FFI | ❌ Missing | Manual trigger |
| Migration APIs in bindings | ❌ Missing | Schema evolution |

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
