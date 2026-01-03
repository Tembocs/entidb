# Ergonomics Improvement Plan

**Status:** Implementation-ready  
**Date:** 2026-01-03  
**Priority:** High — blocking developer onboarding and binding parity

---

## Executive Summary

This plan addresses five ergonomic gaps that make EntiDB difficult to adopt. The fixes establish a **unified typed-collection-first API** across Rust, Dart, and Python, with automatic canonical CBOR encoding and auto-maintained indexes. Manual/low-level APIs remain available but are de-emphasized.

**Key decisions made:**
1. Single "blessed" API surface across all three languages with consistent method names
2. Deprecate manual index mutation APIs in favor of auto-maintained indexes (breaking change, but necessary for correctness)
3. Typed collections become the primary documented path

---

## Issue 1: Docs and Examples Don't Match API Reality

**Severity:** Critical  
**Impact:** First-run onboarding fails; developers copy examples that don't compile

### Current State

| Location | Problem |
|----------|---------|
| [docs/quickstart.md](../quickstart.md) | Uses JSON bytes (`br#"{...}"#`) instead of CBOR; Dart example shows `await txn.put(users, userId, {'name': ...})` but Dart API requires `Uint8List` |
| [docs/api/dart_api.md](../api/dart_api.md) | Shows maps being passed directly to `put` |
| [docs/api/python_api.md](../api/python_api.md) | Shows dicts being passed to `put` |
| [README.md](../../README.md) | Rust example signature may be out of date |

### Implementation Tasks

#### 1.1 Fix Documentation (4-6 hours)

- [ ] **Update [docs/quickstart.md](../quickstart.md)**  
  - Replace JSON bytes with typed collection + canonical CBOR codec usage
  - Rust: Show `EntityCodec` impl (or future `#[derive(Entity)]`) with `to_canonical_cbor`
  - Dart: Show `TypedCollection` with canonical codec helper
  - Python: Show typed collection once implemented (see Issue 2)

- [ ] **Update [docs/api/dart_api.md](../api/dart_api.md)**  
  - Primary examples use `TypedCollection<T>` with `CanonicalCborCodec`
  - Document raw-byte `put` as "advanced/low-level"

- [ ] **Update [docs/api/python_api.md](../api/python_api.md)**  
  - Primary examples use `TypedCollection` (after implementation)
  - Document raw-byte `put` as "advanced/low-level"

- [ ] **Update [docs/api_reference.md](../api_reference.md)**  
  - Reflect typed-collection-first approach

- [ ] **Update [README.md](../../README.md)**  
  - Verify all code snippets compile/run against current API

#### 1.2 Add CI Doc-Snippet Testing (2-3 hours)

- [ ] **Rust doctests** — Already work via `cargo test --doc`; verify all examples marked `ignore` have tracking issues or are tested elsewhere

- [ ] **Dart snippet tests** — Create `test/doc_snippets_test.dart` that extracts and runs code from markdown (or use manual mirroring)

- [ ] **Python snippet tests** — Create `tests/test_doc_snippets.py` using `doctest` module or manual mirroring

### Acceptance Criteria

- [ ] All quickstart examples compile and run without modification
- [ ] CI fails if doc examples break
- [ ] No JSON/UTF-8 bytes in primary documentation paths

---

## Issue 2: Typed Collections Not Discoverable or Parity-Complete

**Severity:** High  
**Impact:** Rust `Collection<T>` is unreachable from `Database`; Python lacks typed collections entirely

### Current State

| Language | Typed Collection | Reachable from Database? |
|----------|------------------|-------------------------|
| Rust | `Collection<T>` in [typed.rs](../../crates/entidb_core/src/collection/typed.rs) | **No** — no `Database::typed_collection` method |
| Dart | `TypedCollection<T>` in [typed_collection.dart](../../bindings/dart/entidb_dart/lib/src/typed_collection.dart) | **Yes** — via extension |
| Python | ❌ None | N/A |

### Implementation Tasks

#### 2.1 Rust: Add `Database::typed_collection<T>` (2-3 hours)

**File:** [crates/entidb_core/src/database.rs](../../crates/entidb_core/src/database.rs)

```rust
/// Returns a typed collection handle for type-safe entity operations.
///
/// This is the recommended API for most use cases. The collection
/// is created if it doesn't exist.
///
/// # Example
///
/// ```rust,ignore
/// let users: Collection<User> = db.typed_collection("users")?;
/// users.put(&user)?;
/// let found = users.get(user.id)?;
/// ```
pub fn typed_collection<T: EntityCodec>(&self, name: &str) -> CoreResult<Collection<T>> {
    let collection_id = self.collection(name)?;
    Ok(Collection::new(
        collection_id,
        name.to_string(),
        Arc::clone(&self.txn_manager),
        Arc::clone(&self.segments),
    ))
}
```

- [ ] Add method to `Database`
- [ ] Add integration test in [crates/entidb_core/src/collection/typed.rs](../../crates/entidb_core/src/collection/typed.rs)
- [ ] Export `Collection` and `EntityCodec` in [lib.rs](../../crates/entidb_core/src/lib.rs) (already done, verify)

#### 2.2 Python: Add `TypedCollection` Class (4-6 hours)

**File:** [bindings/python/entidb_py/src/lib.rs](../../bindings/python/entidb_py/src/lib.rs)

```rust
/// A type-safe collection with automatic CBOR encoding/decoding.
#[pyclass]
pub struct TypedCollection {
    db: Arc<CoreDatabase>,
    collection_id: u32,
    name: String,
    encoder: PyObject,  // Callable: T -> bytes
    decoder: PyObject,  // Callable: bytes -> T
}

#[pymethods]
impl TypedCollection {
    fn put(&self, py: Python, entity_id: &EntityId, value: PyObject) -> PyResult<()> { ... }
    fn get(&self, py: Python, entity_id: &EntityId) -> PyResult<Option<PyObject>> { ... }
    fn delete(&self, entity_id: &EntityId) -> PyResult<()> { ... }
    fn scan_all(&self, py: Python) -> PyResult<Vec<(EntityId, PyObject)>> { ... }
    fn iter(&self, py: Python) -> PyResult<TypedEntityIterator> { ... }
}
```

- [ ] Add `TypedCollection` class
- [ ] Add `Database.typed_collection(name, encode, decode)` factory method
- [ ] Add unit tests
- [ ] Add example in [examples/python_todo/main.py](../../examples/python_todo/main.py)

#### 2.3 Dart: Verify and Document (1 hour)

- [ ] Verify `TypedCollectionExtension` is exported and documented
- [ ] Make `TypedCollection` the primary example in [examples/dart_todo/main.dart](../../examples/dart_todo/main.dart)

### Acceptance Criteria

- [ ] All three languages have: `db.typed_collection(name, ...)` returning a typed handle
- [ ] Typed handle has: `put(entity)`, `get(id)`, `delete(id)`, `scan_all()`, `iter()`
- [ ] Examples in all three languages use typed collections as primary path

---

## Issue 3: Manual Index Maintenance Is Error-Prone

**Severity:** High  
**Impact:** Users must manually sync index mutations with entity writes; violates "no index name in queries" rule

### Current State

- `hash_index_insert`, `hash_index_remove`, `btree_index_insert`, `btree_index_remove`, `fts_index_text`, `fts_remove_entity` — all require manual calls separate from `put`/`delete`
- [database.rs](../../crates/entidb_core/src/database.rs) lines 1759, 1862 show deprecated legacy methods
- No atomic guarantee that index stays in sync with entity

### Implementation Tasks

#### 3.1 Design Declarative Index API (Design doc, 2 hours)

Target API across all languages:

```rust
// Rust
let users = db.typed_collection::<User>("users")?;
let email_idx = users.create_hash_index(|u| &u.email)?;
let age_idx = users.create_btree_index(|u| u.age)?;

// Lookup returns typed iterator
let alice = email_idx.get_one("alice@example.com")?;
let adults = age_idx.range(18..)?;
```

```dart
// Dart
final users = db.typedCollection<User>('users', userCodec);
final emailIdx = users.createHashIndex((u) => u.email);

final alice = emailIdx.getOne('alice@example.com');
```

```python
# Python
users = db.typed_collection('users', encode_user, decode_user)
email_idx = users.create_hash_index(lambda u: u['email'])

alice = email_idx.get_one('alice@example.com')
```

- [ ] Write design doc in `docs/todo/index_api_design.md`
- [ ] Define `IndexHandle<K, T>` type that stores field extractor
- [ ] Define auto-maintenance hook in commit path

#### 3.2 Implement Auto-Maintained Indexes in Rust Core (8-12 hours)

**Files:**
- [crates/entidb_core/src/index/mod.rs](../../crates/entidb_core/src/index/mod.rs)
- [crates/entidb_core/src/collection/typed.rs](../../crates/entidb_core/src/collection/typed.rs)
- [crates/entidb_core/src/transaction.rs](../../crates/entidb_core/src/transaction.rs)

- [ ] Add `IndexSpec` registration on `Collection<T>`
- [ ] Hook into `TransactionManager::commit` to extract field values and update indexes atomically
- [ ] Persist index specs in manifest
- [ ] Rebuild indexes on recovery

#### 3.3 Deprecate Manual Index APIs (2 hours)

- [ ] Add `#[deprecated]` to `hash_index_insert`, `hash_index_remove`, `btree_index_insert`, `btree_index_remove` in core
- [ ] Add deprecation warnings in Dart/Python bindings
- [ ] Update docs to remove manual index examples

#### 3.4 Expose Index Handles in Bindings (4-6 hours each)

- [ ] Dart: `HashIndexHandle`, `BTreeIndexHandle` with typed lookups
- [ ] Python: `HashIndexHandle`, `BTreeIndexHandle` with typed lookups

### Acceptance Criteria

- [ ] Index updates happen atomically with `put`/`delete`
- [ ] Users never call index mutation methods directly
- [ ] Index lookups use typed handles, not string-based field names
- [ ] Legacy manual APIs deprecated with warnings

---

## Issue 4: Full Scans Are Hidden; Python Iterator Not Streaming

**Severity:** High  
**Impact:** Performance foot-guns; Python memory blowup on large collections

### Current State

| Language | `list()` | `iter()` | Streaming? |
|----------|----------|----------|------------|
| Rust Core | `Database::list` → `Vec` | `Collection::iter` → materializes all | ❌ |
| Dart | `db.list(collection)` → `List` | `db.iter()` → streaming via FFI cursor | ✅ |
| Python | `db.list()` → list | `db.iter()` → wraps `list` internally | ❌ |

### Implementation Tasks

#### 4.1 Implement Streaming Cursor in Rust Core (4-6 hours)

**File:** [crates/entidb_core/src/segment/manager.rs](../../crates/entidb_core/src/segment/manager.rs)

```rust
pub struct SegmentCursor {
    // Holds position in segment file, yields one record at a time
}

impl Iterator for SegmentCursor {
    type Item = CoreResult<(EntityId, Vec<u8>)>;
}
```

- [ ] Implement `SegmentCursor` that reads segment file incrementally
- [ ] Add `SegmentManager::iter_cursor(collection_id)` → `SegmentCursor`
- [ ] Update `Collection::iter` to use cursor

#### 4.2 Expose Streaming Iterator in Python FFI (3-4 hours)

**File:** [bindings/python/entidb_py/src/lib.rs](../../bindings/python/entidb_py/src/lib.rs)

- [ ] Change `EntityIterator` to hold FFI cursor handle instead of `Vec`
- [ ] Implement true `__next__` that fetches one record at a time
- [ ] Add `remaining()` estimate if available

#### 4.3 Rename `list` to `scan_all` for Explicitness (2 hours)

- [ ] Rust: Alias `list` → `scan_all`, deprecate `list`
- [ ] Dart: Alias `list` → `scanAll`, deprecate `list`
- [ ] Python: Alias `list` → `scan_all`, deprecate `list`
- [ ] Update all docs to use `scan_all`

#### 4.4 Add Scan Telemetry (2 hours)

- [ ] Increment counter in `DatabaseStats` when full scan performed
- [ ] Add `Config::warn_on_scan` flag that logs warning on scan

### Acceptance Criteria

- [ ] Python `iter()` does not load all records into memory
- [ ] All languages have explicit `scan_all()` method
- [ ] `list()` deprecated with warning
- [ ] Telemetry tracks scan operations

---

## Issue 5: Canonical CBOR Is Hard to Use Correctly

**Severity:** Medium  
**Impact:** Non-canonical data can enter storage; verbose boilerplate in every entity

### Current State

- Rust `EntityCodec` requires manual `Value::Map` construction ([codec.rs](../../crates/entidb_core/src/collection/codec.rs))
- Examples use JSON bytes violating CBOR requirement ([rust_todo/main.rs](../../examples/rust_todo/main.rs))
- Dart relies on user-provided codec with no canonical guarantee ([typed_collection.dart](../../bindings/dart/entidb_dart/lib/src/typed_collection.dart))
- Python has no codec helper at all

### Implementation Tasks

#### 5.1 Rust: Serde-Based Canonical CBOR Codec (4-6 hours)

**File:** [crates/entidb_codec/src/serde.rs](../../crates/entidb_codec/src/serde.rs) (new)

```rust
use serde::{Serialize, Deserialize};

/// Wrapper that implements EntityCodec for any serde-compatible type.
pub struct CborEntity<T> {
    pub id: EntityId,
    pub data: T,
}

impl<T: Serialize + DeserializeOwned> EntityCodec for CborEntity<T> {
    fn entity_id(&self) -> EntityId { self.id }
    fn encode(&self) -> CoreResult<Vec<u8>> {
        to_canonical_cbor_serde(&self.data)
    }
    fn decode(id: EntityId, bytes: &[u8]) -> CoreResult<Self> {
        Ok(Self { id, data: from_cbor_serde(bytes)? })
    }
}
```

- [ ] Add `entidb_codec::serde` module with serde feature flag
- [ ] Implement `to_canonical_cbor_serde` using ciborium with sorted keys
- [ ] Add unit tests verifying canonical output
- [ ] Update [examples/rust_todo/main.rs](../../examples/rust_todo/main.rs) to use `CborEntity<TodoData>`

#### 5.2 Dart: Canonical CBOR Codec Helper (3-4 hours)

**File:** [bindings/dart/entidb_dart/lib/src/canonical_cbor_codec.dart](../../bindings/dart/entidb_dart/lib/src/canonical_cbor_codec.dart) (new)

```dart
class CanonicalCborCodec<T> implements Codec<T> {
  final T Function(Map<String, dynamic>) fromMap;
  final Map<String, dynamic> Function(T) toMap;
  
  @override
  Uint8List encode(T value) {
    final map = toMap(value);
    return canonicalCborEncode(map); // Sorts keys, uses shortest integer form
  }
  
  @override
  T decode(Uint8List bytes) {
    final map = cborDecode(bytes);
    return fromMap(map);
  }
}
```

- [ ] Implement `canonicalCborEncode` that sorts map keys
- [ ] Export from `entidb_dart.dart`
- [ ] Update [examples/dart_todo/main.dart](../../examples/dart_todo/main.dart) to use it

#### 5.3 Python: Canonical CBOR Helper (3-4 hours)

**File:** [bindings/python/entidb_py/src/lib.rs](../../bindings/python/entidb_py/src/lib.rs) (add to existing)

```python
# Expose to Python
def canonical_cbor_encode(value: dict) -> bytes: ...
def canonical_cbor_decode(data: bytes) -> dict: ...
```

- [ ] Add `canonical_cbor_encode`/`decode` functions using Rust `entidb_codec`
- [ ] Add example codec in [examples/python_todo/main.py](../../examples/python_todo/main.py)

### Acceptance Criteria

- [ ] Each language has a one-liner canonical CBOR codec
- [ ] All examples use canonical codec, not JSON bytes
- [ ] Unit tests verify cross-language canonical parity (same input → same bytes)

---

## Issue 6: Remove Deprecated APIs

**Severity:** Medium  
**Impact:** API surface bloat; confusion about which methods to use

### Implementation Tasks

- [ ] Remove deprecated hash/btree/fts manual methods from public surface (after 3.3)
- [ ] Remove any other deprecated methods flagged in codebase
- [ ] Audit FFI exports for unused/deprecated entries
- [ ] Update CHANGELOG noting removals

---

## Implementation Order

```
Phase 1: Foundation (Week 1)
├── 2.1 Rust: Database::typed_collection
├── 4.1 Rust: Streaming cursor
└── 5.1 Rust: Serde-based CBOR codec

Phase 2: Binding Parity (Week 2)
├── 2.2 Python: TypedCollection
├── 4.2 Python: Streaming iterator
├── 5.2 Dart: Canonical CBOR codec
└── 5.3 Python: Canonical CBOR helpers

Phase 3: Index Overhaul (Week 3)
├── 3.1 Design declarative index API
├── 3.2 Implement auto-maintained indexes
├── 3.3 Deprecate manual index APIs
└── 3.4 Expose index handles in bindings

Phase 4: Polish (Week 4)
├── 1.1 Fix documentation
├── 1.2 Add CI doc-snippet testing
├── 4.3 Rename list → scan_all
├── 4.4 Add scan telemetry
└── 6.* Remove deprecated APIs
```

---

## Dependencies and Risks

| Risk | Mitigation |
|------|------------|
| Breaking change to index APIs | Major version bump; migration guide |
| Serde adds dependency weight | Feature-gated; optional for core |
| Streaming cursor complexity | Start with simple sequential read; optimize later |
| Cross-language parity testing | Add test vectors for canonical CBOR in CI |

---

## Estimated Effort Summary

| Phase | Effort | Dependencies |
|-------|--------|--------------|
| Phase 1 (Foundation) | ~10-15 hours | None |
| Phase 2 (Binding Parity) | ~10-14 hours | Phase 1 |
| Phase 3 (Index Overhaul) | ~16-22 hours | Phase 1 |
| Phase 4 (Polish) | ~10-13 hours | Phases 1-3 |
| **Total** | **~46-64 hours** | |

---

## Original Raw Findings (Preserved)

<details>
<summary>Click to expand original analysis notes</summary>

- Critical: Docs and examples don't match the actual API shapes, which makes first-run onboarding fail (Rust `Database::open` signature in `README.md` vs `crates/entidb_core/src/database.rs`; Dart/Python docs show `put` with maps/dicts but bindings require raw bytes in `bindings/dart/entidb_dart/lib/src/database.dart` and `bindings/python/entidb_py/src/lib.rs`).
- High: Typed collections exist in Rust but are not reachable from `Database`, while Dart has `typedCollection` and Python does not, breaking discoverability and parity.
- High: Index usage is manual and non-atomic at the API layer; users must call `hash_index_insert/remove` and FTS indexing separately from entity writes.
- High: Full scans are hidden and Python "iterator" isn't actually streaming; `Database::list` returns full vectors, `Collection::iter` materializes everything, and Python `iter` wraps `list`.
- Medium: Canonical CBOR is required but ergonomically hard; examples store JSON bytes and typed codecs are manual and verbose.
- Rust examples/tests and `EntityCodec` docs require manual CBOR map building and field parsing.
- The public `Database::put` APIs in Rust/Dart/Python all accept raw bytes, forcing developers to do encoding themselves.
- Dart typed collections rely on user-provided codecs and docs show generic `cbor.encode`, which may not be canonical.
- Quickstarts and API docs still use JSON/UTF-8 bytes, which violates canonical CBOR.
- Python lacks a typed collection API entirely.
- Also remove all deprecated APIs from the native core, bindings, and WASM surface to keep the public API minimal and consistent.

</details>
