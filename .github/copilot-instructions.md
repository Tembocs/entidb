# GitHub Copilot Instructions for EntiDB

EntiDB is a **custom embedded entity database engine** in Rust with Dart and Python bindings. See [AGENTS.md](../AGENTS.md) for complete specifications.

## Critical Constraints

**NEVER introduce:**
- SQL, query builders, or DSLs — use host-language filtering (Rust iterators, Dart `where`, Python comprehensions)
- External database dependencies (no RocksDB, SQLite, LMDB, sled)
- Implicit behavior or hidden configuration

## Architecture

```
crates/
├─ entidb_storage/   # Storage backend trait (NO dependency on entidb_core)
├─ entidb_codec/     # Canonical CBOR encoding
├─ entidb_core/      # Core engine (depends on storage + codec only)
├─ entidb_ffi/       # Stable C ABI for bindings
└─ entidb_sync_*/    # Sync protocol, engine, server

bindings/dart/       # Dart FFI binding
bindings/python/     # Python pyo3 binding
web/entidb_wasm/     # WASM + OPFS backend
```

## Key Implementation Rules

1. **Entities**: Immutable `EntityId`, canonical CBOR encode/decode, one collection per entity
2. **Transactions**: ACID, single writer, snapshot isolation, WAL flush before commit ack
3. **WAL**: Append-only, idempotent replay, only committed txns recovered
4. **Segments**: Immutable after sealing, latest version wins, tombstones suppress earlier
5. **Indexes**: Derivable from segments+WAL, atomic updates, never user-referenced by name
6. **Bindings**: Rust, Dart, Python must have identical observable behavior

## Canonical CBOR Rules

- Maps sorted by key (bytewise)
- Integers use shortest encoding
- No floats unless explicit, no NaN, no indefinite-length items
- Strings must be UTF-8

## Storage Backend Interface

Backends are opaque byte stores only:
```rust
trait StorageBackend {
    fn read_at(&self, offset: u64, len: usize) -> Vec<u8>;
    fn append(&mut self, bytes: &[u8]) -> u64;
    fn flush(&mut self);
    fn size(&self) -> u64;
}
```

## Documentation Precedence

1. [docs/invariants.md](../docs/invariants.md) — absolute rules
2. [docs/architecture.md](../docs/architecture.md) — system design
3. [docs/file_format.md](../docs/file_format.md) — binary format spec
4. [docs/transactions.md](../docs/transactions.md) — transaction semantics

## Before Completing Any Task

Verify:
- [ ] No SQL/DSL introduced
- [ ] No external database dependency
- [ ] WAL append-only preserved
- [ ] Segment immutability preserved
- [ ] Binding parity maintained

## Reliability guardrails (MUST)

- In all non-test, non-example, non-benchmark code paths (including bindings/FFI surface), do **not** use `panic!`, `unwrap()`, or `expect()`.
- Instead, return a typed error (`Result`) with enough context to diagnose the failure.
- `panic!`/`unwrap`/`expect` are allowed only in `#[cfg(test)]` code, tests, examples, and benchmarks.
