# EntiDB AI Agent Instructions

**Purpose:** This document provides authoritative instructions for AI coding agents working on EntiDB. Agents **MUST** follow these instructions exactly. Violations indicate architectural drift and must be corrected immediately.

---

## 1. Project identity

EntiDB is a **custom embedded entity database engine** implemented in Rust with mandatory Dart and Python bindings.

**Key characteristics:**

- Entity-first, not table-first
- CBOR-native storage and transmission
- ACID transactions with WAL-based durability
- Optional offline-first synchronization
- Web support via storage backend abstraction

---

## 2. Absolute prohibitions

The following are **strictly forbidden**. Any agent that introduces these has failed:

### 2.1 No query languages

- **NO SQL**
- **NO SQL-like APIs**
- **NO query builders**
- **NO DSLs (domain-specific languages)**
- **NO string-based query syntax**

Filtering is performed using **host-language constructs only** (Rust iterators, Dart `where`, Python comprehensions).

### 2.2 No external database dependencies

- **NO RocksDB**
- **NO SQLite**
- **NO LMDB**
- **NO sled**
- **NO LevelDB**
- **NO any other embedded database engine**

EntiDB implements its own storage engine from scratch.

### 2.3 No magic or implicit behavior

- **NO implicit server discovery**
- **NO implicit schema generation**
- **NO implicit migration guesswork**
- **NO hidden configuration**

All behavior must be explicit and predictable.

### 2.4 No panics/unwraps in production paths

- In all non-test, non-example, non-benchmark code paths (Rust core, storage backends, sync layers, and bindings/FFI surface), **do not** use `panic!`, `unwrap()`, or `expect()`.
- These are process-killing failures and are not acceptable in production-grade database code.
- Use typed errors (`Result`) and propagate failures explicitly.
- `panic!`/`unwrap`/`expect` are allowed only in tests, examples, and benchmarks.

---

## 3. Architectural constraints

### 3.1 Monorepo structure

```
entidb/
├─ crates/
│  ├─ entidb_core/          # Core engine: storage, txn, WAL, indexes
│  ├─ entidb_codec/         # Canonical CBOR encoding
│  ├─ entidb_storage/       # Storage backend trait + adapters
│  ├─ entidb_ffi/           # Stable C ABI boundary
│  ├─ entidb_sync_protocol/ # Sync protocol types (no I/O)
│  ├─ entidb_sync_engine/   # Sync state machine
│  ├─ entidb_sync_server/   # Reference HTTP server
│  ├─ entidb_cli/           # CLI tools
│  └─ entidb_testkit/       # Test utilities
├─ bindings/
│  ├─ dart/entidb_dart/     # Dart FFI binding
│  └─ python/entidb_py/     # Python pyo3 binding
├─ web/entidb_wasm/         # WASM build + OPFS backend
└─ docs/                    # Normative specifications
```

### 3.2 Dependency rules

- `entidb_storage` **MUST NOT** depend on `entidb_core` (no cycles)
- `entidb_core` depends only on `entidb_codec` and `entidb_storage`
- `entidb_sync_protocol` is pure types + codecs (no networking, no file I/O)
- Bindings depend on `entidb_core` via `entidb_ffi` (stable C ABI)

### 3.3 Binding parity

Rust, Dart, and Python bindings **MUST** exhibit identical observable behavior:

- Same API semantics
- Same error conditions
- Same transactional guarantees
- No binding-specific shortcuts

---

## 4. Implementation rules

### 4.1 Entities

Every entity **MUST** have:

- A stable, immutable `EntityId` (globally unique within database)
- `encode() -> bytes`: canonical CBOR encoding
- `decode(bytes) -> Entity`: deterministic decode

Entities belong to exactly one collection. Entity IDs **MUST NOT** be reused.

### 4.2 Transactions

- **ACID** transactions are mandatory
- **Single writer**, multiple readers (Phase 1)
- **Snapshot Isolation** is the minimum isolation level
- Changes are visible **only after commit**
- WAL **MUST** be flushed before commit acknowledgment

Forbidden anomalies:
- Dirty reads
- Non-repeatable reads
- Phantom writes

### 4.3 WAL (Write-Ahead Log)

WAL records:
- `BEGIN(txid)`
- `PUT(collection_id, entity_id, before_hash?, after_bytes)`
- `DELETE(collection_id, entity_id, before_hash?)`
- `COMMIT(txid)`
- `ABORT(txid)`
- `CHECKPOINT(marker)`

Rules:
- WAL is **append-only**
- Records **MUST NOT** be mutated after write
- Recovery replays only committed transactions
- WAL replay **MUST** be idempotent

### 4.4 Segments

- Sealed segments are **immutable**
- Compaction **MUST NOT** change logical state
- Latest committed version per `(collection_id, entity_id)` wins
- Tombstones suppress all earlier versions

### 4.5 Indexes

Index types:
- **HashIndex**: equality lookup
- **BTreeIndex**: range lookup, ordered traversal
- **FtsIndex** (Phase 2): token-based exact match

Rules:
- Users **MUST NOT** reference indexes by name during queries
- Index state **MUST** be derivable from segments and WAL
- Index corruption **MUST NOT** corrupt entity data
- Index updates are atomic with transaction commit

### 4.6 Canonical CBOR

- Maps **MUST** be sorted by key (bytewise)
- Integers **MUST** use shortest encoding
- Floats **MUST NOT** be used unless explicitly allowed
- Strings **MUST** be UTF-8
- Indefinite-length items: **FORBIDDEN**
- NaN values: **FORBIDDEN**

### 4.7 Storage backends

Storage backends are **opaque byte stores** only:

- `read_at(offset, len) -> bytes`
- `append(bytes) -> offset`
- `flush()`
- `size()`

EntiDB owns all file formats, WAL structure, and segment layout. Backends do not interpret data.

Native: `FileBackend` (OS file APIs)
Web: `OpfsBackend` (Origin Private File System), `IndexedDbBackend` (fallback)

### 4.8 FFI boundary

The `entidb_ffi` crate:
- Presents a **stable C ABI**
- Handles memory ownership (Rust owns buffers; bindings explicitly free)
- Converts errors into ABI-safe numeric codes

---

## 5. Crash safety requirements

- After any crash, the database **MUST** recover to the last committed state
- Recovery **MUST NOT** require heuristics or user intervention
- Crash before COMMIT → transaction discarded
- Crash after COMMIT → transaction applied exactly once
- Any checksum failure **MUST** abort open (no heuristic repair)

---

## 6. Sync layer rules

### 6.1 Architecture

- **Pull-then-push** synchronization
- Server is authoritative
- Sync server uses **same EntiDB core** (not an external database)

### 6.2 Change feed

- Emits only committed operations
- Preserves commit order
- Includes: collection, entity_id, op_type, after_bytes, commit sequence number

### 6.3 Sync operations

- Transport: HTTPS
- Encoding: canonical CBOR
- Applying same operation multiple times **MUST NOT** change final state (idempotent)

---

## 7. Testing requirements

### 7.1 Acceptance criteria

All implementations **MUST** satisfy:

| ID | Criterion |
|----|-----------|
| AC-01 | Deterministic behavior: identical operations produce identical bytes |
| AC-02 | Crash safety: recovery to last committed state after any crash |
| AC-03 | No partial state observable by readers |
| AC-04 | Durability: committed data survives power loss |
| AC-05 | No external database dependencies |
| AC-06 | Concurrent readers observe consistent snapshots |
| AC-07 | Commit order defines visibility order |
| AC-08 | Entity identity is stable and immutable |
| AC-09 | Collection isolation: no cross-collection interference |
| AC-10 | Index usage does not change results |
| AC-11 | Full scans are detectable via telemetry |
| AC-12 | Canonical CBOR encoding identical across languages |
| AC-13 | Only committed changes appear in sync stream |
| AC-14 | Replication is idempotent |
| AC-15 | Binding semantic equivalence |
| AC-16 | No SQL/DSL exposed in any binding |
| AC-17 | Web builds meet same durability criteria |
| AC-18 | Browser storage used strictly as byte store |
| AC-19 | No forbidden features introduced |

### 7.2 Test vectors

- CBOR test vectors **MUST** pass identically across Rust, Dart, Python
- Golden tests for file format
- Fuzz harnesses for codec and storage

---

## 8. Implementation sequence

Agents **MUST** follow this ordered build plan:

### Phase 1: Foundation
1. Define invariants and file-format versioning
2. Implement `entidb_storage` (in-memory + file backends)
3. Implement `entidb_codec` (canonical CBOR)

### Phase 2: Durable KV
4. WAL writer + recovery
5. SegmentManager (append-only)
6. EntityStore (put/get/delete raw CBOR)

### Phase 3: Transactions
7. Transaction manager (begin/commit/abort, snapshots)
8. Checkpoint + WAL truncation

### Phase 4: Typed API
9. Rust typed facade (`EntityId`, `Codec<T>`, `Collection<T>`)
10. Explicit scan vs index access API

### Phase 5: Indexes
11. Hash index
12. BTree index

### Phase 6: Sync Prerequisites
13. Change feed emission
14. Logical oplog

### Phase 7: Sync
15. `entidb_sync_protocol`
16. Sync engine state machine
17. Reference sync server

### Phase 8: Bindings
18. `entidb_ffi` (stable C ABI)
19. Dart binding
20. Python binding

### Phase 9: Web
21. WASM build
22. OPFS backend
23. Web examples

### Phase 10: Hardening
24. Compaction
25. Encryption
26. Backups
27. Migrations (optional)

---

## 9. File format reference

### Storage layout
```
entidb/
├─ MANIFEST          # Metadata (atomic write-then-rename)
├─ WAL/
│  ├─ wal-000001.log
│  └─ wal-000002.log
├─ SEGMENTS/
│  ├─ seg-000001.dat
│  └─ seg-000002.dat
└─ LOCK              # Advisory lock for single-writer
```

### WAL record envelope
```
| magic (4) | version (2) | type (1) | length (4) | payload (N) | crc32 (4) |
```

### Segment record
```
| record_len (4) | collection_id (4) | entity_id (16) | flags (1) | sequence (8) | payload (N) | checksum (4) |
```

* `sequence` (8 bytes): Commit sequence number; latest wins during compaction.

Flags: `0x01` = tombstone, `0x02` = encrypted

---

## 10. Quick reference: What to do and what not to do

### ✅ DO

- Use Rust iterators, Dart `where`, Python comprehensions for filtering
- Implement storage from scratch using the storage backend trait
- Ensure all bindings have identical semantics
- Write tests that verify behavior across all three languages
- Use canonical CBOR for all persistence
- Make scans explicit in the API
- Flush WAL before acknowledging commit

### ❌ DO NOT

- Add any SQL, query builder, or DSL
- Import SQLite, RocksDB, sled, or any database crate
- Create binding-specific features or shortcuts
- Allow partial transaction visibility
- Trust browser storage to be durable (EntiDB owns durability)
- Introduce implicit or magical behavior
- Modify sealed segments or committed WAL records

---

## 11. Documentation hierarchy

When conflicts arise, precedence is (highest to lowest):

1. [invariants.md](docs/invariants.md) — absolute rules
2. [architecture.md](docs/architecture.md) — system design
3. [acceptance_criteria.md](docs/acceptance_criteria.md) — verification requirements
4. [file_format.md](docs/file_format.md) — binary format spec
5. [transactions.md](docs/transactions.md) — transaction semantics
6. [sync_protocol.md](docs/sync_protocol.md) — synchronization protocol
7. [access_paths.md](docs/access_paths.md) — index selection
8. [cbor_cannonical.md](docs/cbor_cannonical.md) — encoding rules
9. [bindings_contract.md](docs/bindings_contract.md) — FFI contract

---

## 12. Validation checklist

Before completing any task, agents **MUST** verify:

- [ ] No SQL, query builder, or DSL introduced
- [ ] No external database dependency added
- [ ] Binding parity maintained (if applicable)
- [ ] WAL append-only invariant preserved
- [ ] Segment immutability preserved
- [ ] Canonical CBOR rules followed
- [ ] Transaction atomicity guaranteed
- [ ] Crash recovery remains correct
- [ ] Tests pass across all affected components

---

*This document is normative. Agents must treat it as authoritative.*
