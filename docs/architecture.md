# EntiDB Architecture (Rust Core with Dart + Python Bindings)

**Status:** Authoritative, implementation-grade specification intended to constrain agent behavior.

This document defines EntiDB from a **fresh, clean slate**. It is not a migration guide and must not be implemented by copying legacy structures or assumptions from prior implementations. The purpose is to define **what shall be built** and the **constraints that prevent drift**.

EntiDB is a **custom embedded entity database engine** with:

* A **Rust core** that implements storage, durability, transactions, indexing, change feed, and sync primitives.
* **Mandatory Dart and Python bindings** that expose identical semantics.
* **No query language** of any kind: **no SQL**, **no SQL-like APIs**, **no DSL**.
* **No dependency on any other database engine** for persistence.
* **Web support** through a storage-backend abstraction where the browser provides only a byte store.

---

## 0. Normative language and interpretation

This document uses **normative requirements**. Unless explicitly stated otherwise:

* **MUST / SHALL** denote mandatory, non-negotiable requirements.
* **MUST NOT / SHALL NOT** denote prohibited behavior.
* **MAY** denotes optional behavior.

Any descriptive or narrative language elsewhere in this document is to be interpreted as **normative**. If a component, unit, or behavior is described, it **MUST** be implemented exactly as described.

Any legacy knowledge, prior repositories, or historical implementations **MUST NOT** be used as an implementation reference. This document is the **sole source of architectural truth**.

---

## 1. Introduction

### 1.1 What EntiDB is

EntiDB is an **embedded, entity-based document database** that stores domain objects (entities) directly. The developer defines entity types and uses a typed, language-native API for persistence and retrieval.

## 2. Goals and non-goals

### 2.1 Goals

1. **Entity-first API**: developers interact with entities and typed collections.
2. **Language-native querying**: no SQL, no SQL-like builder, no DSL.
3. **Custom storage engine**: no dependency on another database for persistence.
4. **ACID transactions + WAL**: crash recovery and durability.
5. **Optional offline-first sync**: logical replication via protocol-defined operations.
6. **CBOR-native**: entity payloads are stored and transmitted as CBOR bytes.
7. **Web support**: first-class browser persistence via storage backend abstraction (OPFS/IndexedDB as byte stores).

### 2.2 Non-goals

* General-purpose analytics engine.
* Arbitrary user-provided query language.
* Server-managed “cloud-only” database distinct from EntiDB.
* Cross-process shared access in phase 1 (explicitly single-process, multi-thread within-process).

---

## 3. Issues we are explicitly avoiding

These are non-negotiable, enforced by architecture and API surface:

1. **SQL / SQL-like query builders / DSLs**: no textual or builder-style query language.
2. **External embedded DB dependencies**: no RocksDB/SQLite/LMDB/sled as persistence engines.
3. **Hidden magic**: no implicit server discovery, no implicit schema generation, no implicit migration guesswork.
4. **Leaky abstractions across bindings**: the semantics must be identical in Rust, Dart, Python.
5. **Web persistence illusions**: browser storage is a byte sink; EntiDB still owns file format, WAL, segments.

---

## 4. System overview

### 4.1 High-level architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Application (Dart/Flutter or Python or Rust)                             │
│                                                                         │
│  ┌───────────────┐   ┌──────────────────────────────────────────────┐   │
│  │ Bindings API  │──▶│ EntiDB Core (Rust)                            │   │
│  │ (Dart/Python) │   │ - Entities / Collections                      │   │
│  └───────────────┘   │ - Transactions + WAL                           │   │
│                      │ - Storage engine (custom)                       │   │
│                      │ - Indexes + access paths                        │   │
│                      │ - Change feed                                   │   │
│                      └───────────────┬─────────────────────────────────┘
│                                      │
│                                      ▼
│                          ┌───────────────────────────────┐
│                          │ Storage Backend (pluggable)    │
│                          │ - Native FS (files)            │
│                          │ - Web OPFS / IndexedDB (bytes) │
│                          └───────────────────────────────┘
└─────────────────────────────────────────────────────────────────────────┘

Optional sync:

┌──────────────────────────────┐         HTTPS + CBOR          ┌──────────────────────────────┐
│ EntiDB Sync Client (Dart/Py)  │  ─────────────────────────▶  │ EntiDB Sync Server            │
│ - observes change feed        │                              │ - runs EntiDB core on server   │
│ - logical oplog               │  ◀─────────────────────────  │ - conflict policy              │
└──────────────────────────────┘         pull then push         └──────────────────────────────┘
```

---

## 5. Core concepts and invariants

### 5.1 Entities

An **Entity** is a domain object with a stable identifier.

Required properties:

* `id`: globally unique within a database.
* `encode() -> bytes`: canonical CBOR encoding.
* `decode(bytes) -> Entity`: deterministic decode.

**Invariant:** The database stores canonical CBOR bytes. Bindings may present maps/objects, but the persisted payload is canonical CBOR.

### 5.2 Collections

A **Collection<T>** is a typed container of entities of type `T`.

**Invariant:** Collection boundaries are part of the keyspace and are stable.

### 5.3 Physical WAL vs Logical Oplog

* **WAL**: physical log for atomicity + crash recovery.
* **Sync Oplog**: derived logical replication stream based on committed operations.

This distinction is explicit in current documentation and remains.

### 5.4 “Single engine everywhere”

Server persistence uses the **same EntiDB core** as clients; the sync server is not backed by an external DB.

---

## 6. Monorepo organization (detailed)

### 6.1 Repository root

```
entidb/
├─ crates/
│  ├─ entidb_core/              # Pure Rust core: storage + txn + WAL + indexes
│  ├─ entidb_codec/             # Canonical CBOR + schema tags + stable encoding rules
│  ├─ entidb_storage/           # Storage backend trait + native/web adapters
│  ├─ entidb_sync_protocol/     # Protocol types + CBOR codecs (no I/O)
│  ├─ entidb_sync_engine/       # Sync state machine + oplog mgmt (Rust reference impl)
│  ├─ entidb_sync_server/       # Reference HTTP server (Rust)
│  ├─ entidb_cli/               # CLI for debugging/maintenance
│  └─ entidb_testkit/           # Golden tests, fuzz harnesses, property-based specs
│
├─ bindings/
│  ├─ dart/
│  │  ├─ entidb_dart/           # Dart package using dart:ffi
│  │  └─ build/                 # CI build scripts to produce native libs for platforms
│  └─ python/
│     ├─ entidb_py/             # Python package (pyo3/maturin)
│     └─ build/
│
├─ web/
│  ├─ entidb_wasm/              # WASM build + JS glue + OPFS backend
│  └─ examples/
│
├─ docs/
│  ├─ architecture.md           # This document
│  ├─ api/                      # Binding-facing API references
│  ├─ file_format.md            # Segment/WAL format (Appendix A item)
│  ├─ invariants.md
│  ├─ sync_protocol.md
│  └─ test_vectors/
│
├─ examples/
│  ├─ rust_basic/
│  ├─ flutter_basic/
│  ├─ python_basic/
│  └─ web_basic/
│
├─ Cargo.toml                   # Workspace
└─ LICENSE
```

### 6.2 Dependency rules

* `entidb_core` depends only on `entidb_codec` and `entidb_storage` and standard Rust crates.
* `entidb_storage` must not depend on `entidb_core` (no cycles).
* `entidb_sync_protocol` is pure types + codecs (no networking, no file I/O), mirroring current repo intent.
* Bindings depend on `entidb_core` via stable C-ABI boundary.

### 6.3 C-ABI boundary crate

Add `crates/entidb_ffi/` (not listed above) whose sole job is:

* Present a stable C ABI.
* Handle memory ownership.
* Convert errors into ABI-safe codes.

This isolates FFI concerns from core.

---

## 7. EntiDB core engine (Rust) — subsystems and responsibilities

The current Dart architecture describes modules such as collection/query/transaction/index/encryption/migrations/backup/WAL/pager.

We retain the *capabilities* while re-structuring to avoid query DSLs.

### 7.1 Subsystem: Engine lifecycle

**Unit:** `Database`

* `open(config, storage_backend)`
* `close()`
* `checkpoint()`
* `compact()`

Responsibilities:

* Load manifest.
* Recover from WAL.
* Validate file format versions.
* Start background maintenance (compaction) where applicable.

### 7.2 Subsystem: Manifest + metadata

**Unit:** `ManifestStore`

* Stores:

  * collection registry
  * index registry
  * schema versions / migration markers (optional)
  * encryption settings metadata (never keys)
* Must be append-only with atomic pointer swap (two-phase manifest).

### 7.3 Subsystem: Page/segment manager

**Unit:** `SegmentManager`

* Append-only immutable segments.
* Maintains active segment.
* Handles segment sealing.
* Supplies iterators over records.

**Unit:** `Compactor`

* Merges sealed segments.
* Drops obsolete versions.
* Rebuilds or validates indexes.

### 7.4 Subsystem: Entity store

**Unit:** `EntityStore`

* Keyspace: `(collection_id, entity_id)`.
* Provides:

  * `get_raw()` returning canonical CBOR bytes.
  * `put_raw()` upsert.
  * `delete()` tombstone.
* Does not know about typed entities; it stores bytes.

### 7.5 Subsystem: Typed collection facade

**Unit:** `Collection<T>`

* Exposes ergonomic API for the host language.
* Converts between `T` and CBOR bytes via `Codec<T>`.

---

## 8. Storage backend abstraction (native + web)

### 8.1 StorageBackend trait

A lowest-level, database-agnostic byte storage interface.

**Unit:** `StorageBackend`

* `read_at(offset, len) -> bytes`
* `append(bytes) -> offset`
* `write_at(offset, bytes)` (optional; may be emulated)
* `flush()`
* `size()`

### 8.2 Native backend

**Unit:** `FileBackend`

* Uses OS file APIs.
* Guarantees:

  * durable flush for WAL commits
  * atomic rename for manifest pointer swap

### 8.3 Web backend

**Unit:** `OpfsBackend`

* Uses Origin Private File System as a byte store.

**Unit:** `IndexedDbBackend`

* Fallback when OPFS not available.

**Invariant:** web backends are *byte stores only*; EntiDB owns WAL + segment formats.

---

## 9. Transaction system (ACID) and WAL

The existing implementation emphasizes WAL and configurable isolation.

### 9.1 Transaction goals

* Atomicity: all-or-nothing.
* Consistency: internal invariants preserved.
* Isolation: at minimum snapshot reads; serializable as optional.
* Durability: committed transactions survive crashes.

### 9.2 Concurrency model (Phase 1)

* **Single writer**, multiple readers.
* Writer holds commit lock; readers use snapshots.

### 9.3 WAL records

**Unit:** `WalRecord`

* `Begin(txid)`
* `Put(collection_id, entity_id, before_hash?, after_bytes)`
* `Delete(collection_id, entity_id, before_hash?)`
* `Commit(txid)`
* `Abort(txid)`
* `Checkpoint(marker)`

**Rules:**

* Commit requires WAL flush.
* Recovery replays committed txns; discards incomplete.

### 9.4 Checkpointing

**Unit:** `Checkpointer`

* Creates a stable recovery point.
* Allows WAL truncation.

---

## 10. Data model: entities, collections, codecs

### 10.1 Canonical CBOR

**Unit:** `CanonicalCbor`

* Defines deterministic encoding rules (map key ordering, numeric normalization policy).

### 10.2 Codec

**Unit:** `Codec<T>`

* `encode(&T) -> Vec<u8>`
* `decode(bytes) -> T`

Bindings implement their side’s mapping; core stores bytes.

---

## 11. Indexing and access paths (without query languages)

The current docs mention BTree, Hash, and Full-Text indexes.

We keep these, but indexes are not “queried via DSL.” They are internal access paths.

### 11.1 Index types

**Unit:** `HashIndex`

* Equality lookup.

**Unit:** `BTreeIndex`

* Ordered traversal and range lookup.

**Unit:** `FtsIndex` (phase 2)

* Token-based exact match; no ranking in early phases.

### 11.2 Index declaration (language-native)

Developers define indexes via typed API calls, not DSL strings.

Rust example (conceptual):

* `db.index::<User>().on(|u| &u.age).btree()`

Dart/Python expose the same concept in their idioms.

### 11.3 Index maintenance

* Index updates are part of the transaction commit.
* Recovery rebuild policy:

  * either replay WAL into indexes
  * or rebuild indexes from segments on open (configurable)

---

## 12. Querying model: language-native iteration (no SQL, no DSL)

### 12.1 Non-negotiable rule

**There is no query builder, no DSL, and no SQL-like API surface.**

### 12.2 API shape

The DB exposes iterators/streams and optional “find by index” primitives.

**Unit:** `CollectionIterator`

* Produces entities in deterministic order.

**Unit:** `AccessPath`

* If an index exists, the engine can provide an iterator for:

  * equality
  * range

### 12.3 How filtering works

Filtering is performed using the host language.

* Rust: iterator adapters (`filter`, `map`, `take`).
* Dart: `where`, `map`.
* Python: list comprehensions / generator expressions.

**Constraint:** Only access-path selection is engine-controlled; predicate evaluation is host-language.

### 12.4 Preventing foot-guns

To avoid accidental full scans:

* Provide explicit “scan vs indexed” APIs.
* Provide counters/telemetry to surface scans.
* Provide a configuration flag to forbid full scans in production.

---

## 13. Change feed and oplog: physical vs logical

### 13.1 Change feed

**Unit:** `ChangeFeed`

* Emits committed operations (after commit).
* Includes:

  * collection
  * entity_id
  * op_type
  * after_bytes (or tombstone)
  * commit sequence number

### 13.2 Logical oplog

**Unit:** `LogicalOplog`

* Stores replication operations derived from change feed.
* Separately compactable.

---

## 14. Synchronization layer

* pull-then-push
* explicit configuration
* server authority
* conflict policies

### 14.1 Sync engine responsibilities

**Unit:** `SyncEngine`

* Maintains state machine: `idle → connecting → pulling → pushing → synced → idle` with error retry.
* Persists:

  * device id
  * db id
  * server cursor
  * last pushed op id

### 14.2 Applying remote operations

**Unit:** `RemoteApplier`

* Applies a batch inside one EntiDB transaction.
* Validates entity versions.
* Emits conflicts.

### 14.3 Conflict resolution

**Unit:** `ConflictPolicy`

* Server-side policy is authoritative.
* Client-side may surface conflicts for manual resolution.

---

## 15. Sync protocol specification (CBOR)

Protocol properties are retained:

* Transport: HTTPS
* Encoding: canonical CBOR
* Direction: outbound client initiated
* Identity: dbId, deviceId, opId, serverCursor

### 15.1 Operation payload

A `SyncOperation` carries entity payload as raw EntiDB CBOR bytes.

---

## 16. Sync server architecture

**Unit:** `SyncHttpServer`

* Auth middleware.
* Cursor management.
* Pull endpoint: return ops since cursor.
* Push endpoint: accept ops, detect conflicts, commit transactionally.

**Invariant:** Server persists data using EntiDB core, not an external database.

---

## 17. Bindings: Dart and Python (must-have)

### 17.1 Common binding requirements

* Identical semantics across languages.
* No language exposes SQL/DSL.
* Memory-safe boundary:

  * Rust owns buffers; bindings copy or use pinned regions with explicit free.

### 17.2 Dart binding

**Unit:** `entidb_dart`

* `dart:ffi` binding to `entidb_ffi`.
* Provides:

  * `Database.open()`
  * `collection<T>()`
  * `transaction(fn)`
  * `iter()` and explicit indexed lookups

### 17.3 Python binding

**Unit:** `entidb_py`

* `pyo3` + `maturin`.
* Provides:

  * context-managed `Database`
  * collection iterators
  * transactional context manager

---

## 18. Integration points (apps, tooling, CLI)

**Unit:** `entidb_cli`

* `entidb inspect <path>`
* `entidb verify <path>`
* `entidb compact <path>`
* `entidb dump-oplog <path>`

---

## 19. Security model (encryption, auth, integrity)

The current repo includes optional encryption (AES-GCM) and auth concerns for server.

### 19.1 Encryption at rest

**Unit:** `CryptoManager`

* AES-256-GCM.
* Per-database master key (provided by app).
* Key rotation strategy defined in Appendix A.

### 19.2 Integrity

* WAL records include checksums.
* Segments include block checksums.

---

## 20. Implementation sequence (ordered build plan; no timeline)

This section is intentionally prescriptive. Each step defines what the subsystem must do before moving to the next.

### 20.1 Foundation

1. **Define invariants and file-format versioning**

   * Create `docs/file_format.md` and version constants.
   * Define canonical CBOR rules.
2. **Implement `entidb_storage`**

   * In-memory backend (for tests).
   * File backend (native).
   * Contract tests for read/append/flush semantics.
3. **Implement `entidb_codec`**

   * Canonical CBOR encode/decode.
   * Golden vectors and fuzz tests.

### 20.2 Minimal durable KV for entities

4. **Implement WAL writer + recovery**

   * WAL append, flush on commit.
   * Recovery replay.
5. **Implement SegmentManager (append-only)**

   * Append records.
   * Read by offset.
6. **Implement EntityStore**

   * Put/get/delete for raw CBOR.
   * Maintain primary index (entity_id → latest record pointer).

### 20.3 Transactions

7. **Transaction manager**

   * Begin/commit/abort.
   * Atomic visibility of commit.
   * Snapshot reads.
8. **Checkpoint + WAL truncation**

   * Define checkpoint record.
   * Truncation safety.

### 20.4 Typed API

9. **Rust typed facade**

   * `EntityId`, `Codec<T>`, `Collection<T>`.
   * `iter()` and `get(id)`.
10. **Explicit scan vs index access**

    * Introduce API that makes scans explicit.

### 20.5 Indexes

11. **Hash index**

    * Build, update on commit.
    * Equality access path.
12. **BTree index**

    * Range access path.

### 20.6 Change feed + sync prerequisites

13. **Change feed emission**

    * Emit only committed ops.
14. **Logical oplog (local)**

    * Persist derived operations.
    * Compaction.

### 20.7 Sync protocol + engine + server

15. **Implement `entidb_sync_protocol`**

    * CBOR message types.
    * Test vectors.
16. **Implement sync engine state machine**

    * pull-then-push.
    * cursor management.
17. **Implement reference sync server**

    * pull/push endpoints.
    * auth.
    * conflict policy.

### 20.8 Bindings

18. **Define stable C ABI** (`entidb_ffi`)

    * error codes, buffers.
19. **Dart binding**

    * Open, collections, iterators, transactions.
20. **Python binding**

    * Same capabilities; idiomatic contexts.

### 20.9 Web

21. **WASM build**

    * Worker-based runtime.
22. **OPFS backend**

    * Byte-store implementation.
23. **Web example**

    * Basic CRUD + persistence.

### 20.10 Hardening

24. **Compaction**

    * Segment merge.
25. **Encryption**

    * Encrypt record blocks.
26. **Backups**

    * Snapshot + restore.
27. **Migrations (optional)**

    * Metadata-driven; never codegen.

---

## 21. Appendix A — Five additional specs to lock down

These five items must exist as separate docs and tests; they are designed to prevent agent drift.

1. **File format specification** (`docs/file_format.md`)

   * Segment record layout, headers, checksums, versioning.
   * WAL record layout, checkpointing rules.
   * Compaction invariants.

2. **Transaction and isolation specification** (`docs/transactions.md`)

   * Exact isolation level semantics.
   * Single-writer rule.
   * Snapshot visibility rules.

3. **Index selection and scan policy** (`docs/access_paths.md`)

   * How access paths are chosen.
   * How scans are made explicit.
   * Telemetry requirements.

4. **Canonical CBOR rules + test vectors** (`docs/cbor_canonical.md` + `docs/test_vectors/`)

   * Deterministic encoding rules.
   * Cross-language vector parity.

5. **FFI + binding contract** (`docs/bindings_contract.md`)

   * ABI stability requirements.
   * Memory ownership, error mapping.
   * Feature parity test suite across Rust/Dart/Python.
