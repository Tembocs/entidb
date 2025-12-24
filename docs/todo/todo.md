# EntiDB Missing Features Report

## Executive Summary

The core database engine has solid foundations (storage, WAL, segments, transactions, indexes), but several features are **incomplete** or **missing entirely**. The sync layer exists but is not integrated with the core. Bindings lack critical features.

**Update (December 2024):** Phase 1 (Core Completeness) is now ✅ COMPLETE.
**Update (December 2024):** Phase 2 (Binding Parity) is now ✅ COMPLETE.

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

---

## 🔴 Critical Missing Features

### 5. **Change Feed Integration with Core** - MISSING
**Current State:** `ChangeFeed` exists in `entidb_sync_protocol` but is **not wired** into `Database` or `TransactionManager`.

**Impact:** No way to observe committed changes for sync, reactive UIs, or auditing.

**Required:**
- Hook in `TransactionManager::commit()` to emit `ChangeEvent`
- Expose `db.subscribe()` or `db.changes()` API

---

### 6. **Index APIs in Bindings** - MISSING
**Current State:** Core has full index implementations (BTree, Hash), but FFI/Dart/Python expose **none**.

| Feature | Core | FFI | Dart | Python | WASM |
|---------|:----:|:---:|:----:|:------:|:----:|
| Hash Index | ✅ | ❌ | ❌ | ❌ | ❌ |
| BTree Index | ✅ | ❌ | ❌ | ❌ | ❌ |
| Index Query | ✅ | ❌ | ❌ | ❌ | ❌ |
| Encryption | ✅ | ❌ | ❌ | ❌ | ❌ |

---

## 🟡 Moderate Missing Features

### 7. **Segment Auto-Sealing & Rotation** - PARTIAL
**Current State:** `SegmentManager` has a single segment. `max_segment_size` config exists but is **never checked**.

**Impact:** Single segment grows forever. No multi-segment structure.

**Required:**
- Auto-seal when size exceeded
- Create new segment file
- Manage multiple segment files

---

### 8. **Telemetry/Diagnostics (AC-11)** - MISSING
**Current State:** Doc comments warn about scans but **no actual telemetry**.

**Impact:** No way to detect performance issues, full scans, or gather metrics.

**Required:**
- Event hooks for operations
- Metrics counters (reads, writes, scans, transactions)
- `db.stats()` or `db.info()` method

---

### 9. **Full-Text Index (FtsIndex)** - MISSING
**Current State:** Mentioned as "Phase 2" in docs. Not implemented.

---

### 10. **Sync Layer Not Integrated** - PARTIAL
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

### Phase 3: Index APIs
7. **Index creation in FFI/bindings** - Query efficiency
8. **Index query APIs** - Fast lookups

### Phase 4: Observability
9. **Change feed integration** - Sync prerequisite, reactive apps
10. **Telemetry hooks** - AC-11 compliance, debugging

### Phase 5: Advanced
11. **Segment rotation** - Large database support
12. **Full-text index** - Text search capability
13. **Complete sync layer** - Offline-first apps

---
