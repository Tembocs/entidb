# EntiDB Missing Features Report

## Executive Summary

The core database engine has solid foundations (storage, WAL, segments, transactions, indexes), but several features are **incomplete** or **missing entirely**. The sync layer exists but is not integrated with the core. Bindings lack critical features.

**Update (December 2024):** Phase 1 (Core Completeness) is now ✅ COMPLETE.
**Update (December 2024):** Phase 2 (Binding Parity) is now ✅ COMPLETE.
**Update (December 2024):** Phase 3 (Index APIs) is now ✅ COMPLETE.
**Update (December 2024):** Phase 4 (Observability) is now ✅ COMPLETE.

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

## � Moderate Missing Features

### 7. **Segment Auto-Sealing & Rotation** - PARTIAL
**Current State:** `SegmentManager` has a single segment. `max_segment_size` config exists but is **never checked**.

**Impact:** Single segment grows forever. No multi-segment structure.

**Required:**
- Auto-seal when size exceeded
- Create new segment file
- Manage multiple segment files

---

### 8. **Full-Text Index (FtsIndex)** - MISSING
**Current State:** Mentioned as "Phase 2" in docs. Not implemented.

---

### 9. **Sync Layer Not Integrated** - PARTIAL
**Current State:** Sync protocol, engine, and server exist but:
- Client oplog is in-memory only
- Server doesn't use EntiDB for storage
- No real HTTP transport (only mock)
- No authentication

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

### Phase 5: Advanced
11. **Segment rotation** - Large database support
12. **Full-text index** - Text search capability
13. **Complete sync layer** - Offline-first apps

---
