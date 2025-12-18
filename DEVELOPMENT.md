# EntiDB Development Guide

This document defines **phase gates**, **test requirements**, and **CI requirements** for EntiDB implementation.

Each phase **MUST** be completed before proceeding to the next. Completion is verified by passing all acceptance criteria and tests.

---

## 1. Phase Completion Criteria

### Phase 1: Foundation

#### 1.1 Workspace Setup

**Deliverables:**
- [ ] `Cargo.toml` workspace with all crate stubs
- [ ] `.cargo/config.toml` with common settings
- [ ] `rustfmt.toml` and `clippy.toml` configurations
- [ ] CI workflow (GitHub Actions)
- [ ] `LICENSE` file (choose license)

**Acceptance:**
- [ ] `cargo build` succeeds
- [ ] `cargo test` runs (even with no tests)
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo fmt --check` passes

---

#### 1.2 `entidb_storage` Crate

**Deliverables:**
- [ ] `StorageBackend` trait defined
- [ ] `InMemoryBackend` implementation
- [ ] `FileBackend` implementation
- [ ] Contract test suite
- [ ] Crate README.md

**Public API:**
```rust
pub trait StorageBackend: Send + Sync {
    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>>;
    fn append(&mut self, data: &[u8]) -> Result<u64>;
    fn flush(&mut self) -> Result<()>;
    fn size(&self) -> Result<u64>;
    fn sync(&mut self) -> Result<()>;
}
```

**Acceptance Criteria:**
- [ ] `InMemoryBackend` passes all contract tests
- [ ] `FileBackend` passes all contract tests
- [ ] Read-after-write consistency verified
- [ ] Flush guarantees tested
- [ ] Error handling for I/O failures
- [ ] No dependency on `entidb_core` (prevents cycles)

**Test Requirements:**
- [ ] Contract tests: append, read_at, flush, size
- [ ] Edge cases: empty read, zero-length append, read past EOF
- [ ] Concurrent read tests (Send + Sync verification)
- [ ] File backend: file creation, persistence across reopen

---

#### 1.3 `entidb_codec` Crate

**Deliverables:**
- [ ] Canonical CBOR encoder
- [ ] Canonical CBOR decoder
- [ ] `Value` enum for dynamic CBOR values
- [ ] Test vectors matching spec
- [ ] Crate README.md

**Public API:**
```rust
pub fn encode_canonical<T: Serialize>(value: &T) -> Result<Vec<u8>>;
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T>;
pub fn encode_value(value: &Value) -> Result<Vec<u8>>;
pub fn decode_value(bytes: &[u8]) -> Result<Value>;
```

**Canonical CBOR Rules (from `cbor_canonical.md`):**
- [ ] Maps sorted by key (bytewise)
- [ ] Integers use shortest encoding
- [ ] No floats unless explicitly allowed
- [ ] Strings are UTF-8
- [ ] No indefinite-length items
- [ ] No NaN values

**Acceptance Criteria:**
- [ ] Encode/decode roundtrip for all CBOR types
- [ ] Canonical encoding verified (identical inputs → identical bytes)
- [ ] Test vectors from spec pass
- [ ] Determinism: same input always produces same bytes
- [ ] Invalid CBOR rejected with clear errors

**Test Requirements:**
- [ ] Roundtrip tests for: integers, strings, bytes, arrays, maps, booleans, null
- [ ] Map key sorting verification
- [ ] Integer shortest encoding verification
- [ ] Reject indefinite-length encoding
- [ ] Reject NaN values
- [ ] Cross-platform consistency (if possible)

---

### Phase 2: Durable KV

#### 2.1 WAL Writer + Recovery

**Deliverables:**
- [ ] `WalWriter` for append-only log
- [ ] `WalReader` for recovery
- [ ] WAL record types (BEGIN, PUT, DELETE, COMMIT, ABORT, CHECKPOINT)
- [ ] CRC32 checksums
- [ ] Recovery replay logic

**Acceptance Criteria:**
- [ ] WAL is append-only (no mutations)
- [ ] Records have correct checksums
- [ ] Recovery replays only committed transactions
- [ ] Partial/corrupted records detected and skipped
- [ ] Idempotent replay (multiple replays = same state)

**Test Requirements:**
- [ ] Write and read back records
- [ ] Checksum validation
- [ ] Corrupted record detection
- [ ] Crash recovery simulation (truncated writes)
- [ ] Transaction atomicity (commit or nothing)

---

#### 2.2 SegmentManager

**Deliverables:**
- [ ] `Segment` struct for immutable data files
- [ ] `SegmentManager` for segment lifecycle
- [ ] Segment record format
- [ ] Segment sealing logic

**Acceptance Criteria:**
- [ ] Segments are append-only until sealed
- [ ] Sealed segments are immutable
- [ ] Records include collection_id, entity_id, flags, payload, checksum
- [ ] Latest version per entity wins during reads

**Test Requirements:**
- [ ] Append and read records
- [ ] Segment sealing
- [ ] Multi-segment iteration
- [ ] Tombstone handling

---

#### 2.3 EntityStore

**Deliverables:**
- [ ] `EntityStore` for raw CBOR entity storage
- [ ] Primary index (entity_id → location)
- [ ] Basic CRUD: `put_raw`, `get_raw`, `delete`

**Acceptance Criteria:**
- [ ] Put/get roundtrip works
- [ ] Delete creates tombstone
- [ ] Latest version visible
- [ ] Tombstones suppress earlier versions

**Test Requirements:**
- [ ] CRUD operations
- [ ] Overwrite behavior
- [ ] Delete and re-read
- [ ] Multi-entity operations

---

### Phase 3: Transactions

#### 3.1 Transaction Manager

**Deliverables:**
- [ ] `Transaction` struct
- [ ] `begin()`, `commit()`, `abort()` lifecycle
- [ ] Snapshot isolation for readers
- [ ] Single-writer enforcement

**Acceptance Criteria:**
- [ ] Atomic commit (all or nothing)
- [ ] Readers see consistent snapshots
- [ ] No dirty reads
- [ ] No partial commits visible
- [ ] WAL flushed before commit ack

**Test Requirements:**
- [ ] Basic transaction lifecycle
- [ ] Concurrent reader during write
- [ ] Abort rollback verification
- [ ] Crash before commit = no changes
- [ ] Crash after commit = changes visible

---

#### 3.2 Checkpoint + WAL Truncation

**Deliverables:**
- [ ] Checkpoint creation
- [ ] WAL truncation after checkpoint
- [ ] Recovery from checkpoint

**Acceptance Criteria:**
- [ ] Checkpoint captures consistent state
- [ ] WAL can be truncated after checkpoint
- [ ] Recovery uses checkpoint + WAL tail

**Test Requirements:**
- [ ] Create checkpoint and verify
- [ ] Truncate and recover
- [ ] Checkpoint during active transactions

---

### Phase 4: Typed API

#### 4.1 Typed Collection Facade

**Deliverables:**
- [ ] `Collection<T>` generic wrapper
- [ ] `EntityId` type
- [ ] `Codec<T>` trait for entity serialization
- [ ] Typed `get`, `put`, `delete`, `iter`

**Acceptance Criteria:**
- [ ] Type-safe entity operations
- [ ] Codec roundtrip preserves data
- [ ] Collection isolation

**Test Requirements:**
- [ ] Typed CRUD operations
- [ ] Multiple entity types
- [ ] Collection isolation

---

#### 4.2 Explicit Scan vs Index API

**Deliverables:**
- [ ] `scan()` method for explicit full scans
- [ ] `get(id)` for primary key access
- [ ] Access path telemetry

**Acceptance Criteria:**
- [ ] Scans are explicit (not hidden)
- [ ] Telemetry emitted for access paths
- [ ] Scan policy enforcement (allow/warn/forbid)

**Test Requirements:**
- [ ] Scan behavior
- [ ] Telemetry capture
- [ ] Policy enforcement

---

### Phase 5: Indexes

#### 5.1 Hash Index

**Deliverables:**
- [ ] `HashIndex` implementation
- [ ] Index declaration API
- [ ] Transactional index updates

**Acceptance Criteria:**
- [ ] O(1) equality lookup
- [ ] Index consistent with entities
- [ ] Atomic updates with transactions

**Test Requirements:**
- [ ] Index creation
- [ ] Equality lookup
- [ ] Update and delete handling
- [ ] Rebuild produces same results

---

#### 5.2 BTree Index

**Deliverables:**
- [ ] `BTreeIndex` implementation
- [ ] Range query support
- [ ] Ordered iteration

**Acceptance Criteria:**
- [ ] O(log n) lookups
- [ ] Range queries work
- [ ] Ordering preserved

**Test Requirements:**
- [ ] Point lookup
- [ ] Range queries
- [ ] Prefix matching
- [ ] Ordered iteration

---

### Phase 6-10: Sync, Bindings, Web, Hardening

(Detailed criteria to be added when Phase 5 is complete)

---

## 2. Test Requirements Per Crate

### 2.1 Test Categories

Each crate **MUST** include:

| Category | Location | Purpose |
|----------|----------|---------|
| Unit tests | `src/*.rs` (`#[cfg(test)]`) | Test individual functions/structs |
| Integration tests | `tests/*.rs` | Test crate public API |
| Doc tests | `///` comments | Verify examples compile and run |
| Property tests | `tests/proptest_*.rs` | Randomized invariant checking |
| Golden tests | `tests/golden/*.rs` | Binary format stability |

### 2.2 Coverage Requirements

| Crate | Minimum Coverage | Critical Paths |
|-------|------------------|----------------|
| `entidb_storage` | 90% | All trait methods |
| `entidb_codec` | 95% | Encode/decode, canonical rules |
| `entidb_core` | 85% | WAL, transactions, EntityStore |
| `entidb_ffi` | 80% | All exported functions |

### 2.3 Test Naming Convention

```rust
#[test]
fn <module>_<operation>_<scenario>() {
    // e.g., backend_append_returns_correct_offset()
    // e.g., codec_encode_map_keys_sorted()
    // e.g., wal_recovery_skips_uncommitted()
}
```

### 2.4 Property-Based Testing

Use `proptest` for:
- CBOR encode/decode roundtrip
- WAL write/read consistency
- Transaction isolation
- Index consistency

```rust
proptest! {
    #[test]
    fn codec_roundtrip(value: Value) {
        let encoded = encode_value(&value)?;
        let decoded = decode_value(&encoded)?;
        prop_assert_eq!(value, decoded);
    }
}
```

### 2.5 Golden Tests

For binary format stability:

```rust
#[test]
fn wal_record_format_v1() {
    let record = WalRecord::Put { /* ... */ };
    let bytes = record.encode();
    
    // Golden file: tests/golden/wal_put_record_v1.bin
    let expected = include_bytes!("golden/wal_put_record_v1.bin");
    assert_eq!(bytes, expected);
}
```

---

## 3. CI Workflow Requirements

### 3.1 GitHub Actions Workflow

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -Dwarnings

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      
      - name: Format check
        run: cargo fmt --all -- --check
      
      - name: Clippy
        run: cargo clippy --all-targets --all-features
      
      - name: Build
        run: cargo build --all-targets
      
      - name: Test
        run: cargo test --all-targets

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-llvm-cov
      
      - name: Generate coverage
        run: cargo llvm-cov --all-features --lcov --output-path lcov.info
      
      - name: Upload coverage
        uses: codecov/codecov-action@v3
        with:
          files: lcov.info

  # Platform matrix (Phase 9+)
  # cross-platform:
  #   strategy:
  #     matrix:
  #       os: [ubuntu-latest, windows-latest, macos-latest]
  #   runs-on: ${{ matrix.os }}
  #   steps:
  #     - uses: actions/checkout@v4
  #     - uses: dtolnay/rust-toolchain@stable
  #     - run: cargo test
```

### 3.2 CI Gates

**All PRs MUST pass:**
- [ ] `cargo fmt --check` (formatting)
- [ ] `cargo clippy` with no warnings
- [ ] `cargo test` all tests pass
- [ ] Coverage does not decrease

**Before merge to main:**
- [ ] All acceptance criteria for current phase checked
- [ ] Documentation updated
- [ ] CHANGELOG.md entry added

### 3.3 Branch Protection Rules

Configure on GitHub:
- Require PR reviews (1+ approvals)
- Require status checks to pass
- Require branches to be up to date
- No direct pushes to main

---

## 4. Module Completion Checklist Template

Copy this checklist when completing each crate:

```markdown
## Crate: `entidb_<name>` Completion Checklist

### Code Quality
- [ ] All public types have doc comments
- [ ] All public functions have doc comments with examples
- [ ] No `unwrap()` or `expect()` in library code
- [ ] Proper error types defined
- [ ] `#![deny(missing_docs)]` enabled

### Testing
- [ ] Unit tests for all modules
- [ ] Integration tests for public API
- [ ] Doc tests pass
- [ ] Property tests where applicable
- [ ] Golden tests for binary formats
- [ ] Coverage target met

### CI/Tooling
- [ ] `cargo fmt` applied
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo doc` builds without warnings
- [ ] CI pipeline passes

### Documentation
- [ ] Crate README.md exists
- [ ] CHANGELOG.md entry added
- [ ] Architecture decisions documented

### Review
- [ ] Code review completed
- [ ] Acceptance criteria verified
- [ ] Ready for next phase
```

---

## 5. Development Workflow

### 5.1 Starting a New Phase

1. Create branch: `phase-N-description`
2. Copy completion checklist to PR description
3. Implement incrementally with tests
4. Update CHANGELOG.md
5. Request review when checklist complete

### 5.2 During Development

```bash
# Run frequently during development
cargo fmt
cargo clippy
cargo test

# Before committing
cargo test --all-targets
cargo doc --no-deps
```

### 5.3 Completing a Phase

1. Verify all checklist items
2. Run full test suite
3. Update documentation
4. Create PR with checklist
5. Address review feedback
6. Merge when approved

---

## 6. Current Status

| Phase | Status | Notes |
|-------|--------|-------|
| 1.1 Workspace Setup | ⬜ Not Started | |
| 1.2 `entidb_storage` | ⬜ Not Started | |
| 1.3 `entidb_codec` | ⬜ Not Started | |
| 2.1 WAL | ⬜ Not Started | |
| 2.2 SegmentManager | ⬜ Not Started | |
| 2.3 EntityStore | ⬜ Not Started | |
| 3.1 Transactions | ⬜ Not Started | |
| 3.2 Checkpoints | ⬜ Not Started | |
| 4.1 Typed API | ⬜ Not Started | |
| 4.2 Scan API | ⬜ Not Started | |
| 5.1 Hash Index | ⬜ Not Started | |
| 5.2 BTree Index | ⬜ Not Started | |

---

## References

- [AGENTS.md](AGENTS.md) — Implementation constraints
- [docs/architecture.md](docs/architecture.md) — System design
- [docs/invariants.md](docs/invariants.md) — Absolute rules
- [docs/file_format.md](docs/file_format.md) — Binary format spec
