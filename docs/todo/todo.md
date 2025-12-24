# EntiDB Missing Features Report

## Executive Summary

The core database engine has solid foundations (storage, WAL, segments, transactions, indexes), but several features are **incomplete** or **missing entirely**. The sync layer exists but is not integrated with the core. Bindings lack critical features.

**Update (December 2024):** Phase 1 (Core Completeness) is now ✅ COMPLETE.

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

---

## 🔴 Critical Missing Features

### 4. **Change Feed Integration with Core** - MISSING
**Current State:** `ChangeFeed` exists in `entidb_sync_protocol` but is **not wired** into `Database` or `TransactionManager`.

**Impact:** No way to observe committed changes for sync, reactive UIs, or auditing.

**Required:**
- Hook in `TransactionManager::commit()` to emit `ChangeEvent`
- Expose `db.subscribe()` or `db.changes()` API

---

### 5. **Backup/Restore/Index APIs in Bindings** - MISSING
**Current State:** Core has full implementations, but FFI/Dart/Python expose **none**.

| Feature | Core | FFI | Dart | Python | WASM |
|---------|:----:|:---:|:----:|:------:|:----:|
| Backup/Restore | ✅ | ❌ | ❌ | ❌ | ❌ |
| Indexes | ✅ | ❌ | ❌ | ❌ | ❌ |
| Checkpoint | ✅ | ❌ | ❌ | ❌ | ✅ |
| Encryption | ✅ | ❌ | ❌ | ❌ | ❌ |

---

## 🟡 Moderate Missing Features

### 6. **Segment Auto-Sealing & Rotation** - PARTIAL
**Current State:** `SegmentManager` has a single segment. `max_segment_size` config exists but is **never checked**.

**Impact:** Single segment grows forever. No multi-segment structure.

**Required:**
- Auto-seal when size exceeded
- Create new segment file
- Manage multiple segment files

---

### 7. **Telemetry/Diagnostics (AC-11)** - MISSING
**Current State:** Doc comments warn about scans but **no actual telemetry**.

**Impact:** No way to detect performance issues, full scans, or gather metrics.

**Required:**
- Event hooks for operations
- Metrics counters (reads, writes, scans, transactions)
- `db.stats()` or `db.info()` method

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

### Phase 2: Binding Parity
4. **Backup/Restore in FFI/bindings** - Data portability
5. **Index APIs in FFI/bindings** - Query efficiency
6. **Checkpoint in FFI/bindings** - Manual durability control

### Phase 3: Observability
7. **Change feed integration** - Sync prerequisite, reactive apps
8. **Telemetry hooks** - AC-11 compliance, debugging

### Phase 4: Advanced
9. **Segment rotation** - Large database support
10. **Full-text index** - Text search capability
11. **Complete sync layer** - Offline-first apps

---
