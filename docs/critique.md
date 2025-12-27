# EntiDB Repository Critique

## Findings (ordered by severity)

### Critical

1) Snapshot isolation and single-writer guarantees are not enforced.
- A snapshot sequence is captured at begin, but reads ignore it and always read the latest segment state. Multiple writers can run concurrently because the write lock is only held during commit.
- This violates docs/transactions.md and docs/invariants.md (no dirty/non-repeatable reads, single writer).
- Evidence: `crates/entidb_core/src/transaction/manager.rs:75`, `crates/entidb_core/src/transaction/manager.rs:96`, `crates/entidb_core/src/transaction/manager.rs:186`.

2) Commit durability is not guaranteed; WAL flush is not an fsync.
- Commits call `wal.flush()` which delegates to `StorageBackend::flush()`. The FileBackend flush uses `File::flush()` which does not guarantee data is on disk. This violates the WAL flush-before-ack invariant and AC-04 durability.
- Evidence: `crates/entidb_core/src/transaction/manager.rs:129`, `crates/entidb_core/src/wal/writer.rs:79`, `crates/entidb_storage/src/file.rs:125`.

3) EncryptedBackend is functionally incorrect and violates determinism/security guarantees.
- The encryption is XOR-based with time/address-derived nonces and a non-cryptographic tag, but the docs describe AES-GCM. This breaks determinism (AC-01) and security expectations.
- Read offsets are treated as logical offsets even though ciphertext is longer than plaintext; reads after offset 0 will not map correctly.
- Evidence: `crates/entidb_storage/src/encrypted.rs:118`, `crates/entidb_storage/src/encrypted.rs:215`.

### High

4) ~~Segment file format in code does not match the spec.~~ **RESOLVED**
- ~~Code includes a sequence number in the segment record header; the spec omits this field. This is a hard incompatibility with docs/file_format.md and test vectors.~~
- **Fix:** Documentation updated to include the sequence number field. The sequence is required for "latest version wins" semantics during compaction and visibility ordering.

5) Index invariants and access path rules are violated.
- Users must not reference indexes by name, and indexes should be automatically maintained and derivable. The public API requires index names and exposes manual insert/remove/lookups with no transactional coupling.
- Indexes are not persisted or registered in the manifest, which contradicts the architecture and index invariants.
- Evidence: `docs/access_paths.md:17`, `crates/entidb_core/src/database.rs:947`, `crates/entidb_core/src/database.rs:1099`, `crates/entidb_core/src/manifest.rs:18`.

6) ~~Change feed emits incorrect operation types.~~ **RESOLVED**
- ~~Updates are always emitted as inserts, so consumers cannot distinguish inserts vs updates. This breaks sync protocol expectations for op_type accuracy.~~
- **Fix:** The `PendingWrite::Put` variant now includes an `is_update: Option<bool>` field. At commit time, if `is_update` is `None`, the database checks entity existence at the transaction's snapshot sequence to determine the correct operation type. New entities emit `ChangeType::Insert`, existing entities emit `ChangeType::Update`, and deletes emit `ChangeType::Delete`. Added `put_with_op_type()` for callers who already know the operation type (e.g., sync layer). Comprehensive tests added to verify correct behavior.

7) ~~Compaction scans all segments, including the active segment, and does not coordinate with writers.~~ **RESOLVED**
- ~~The comment says sealed-only, but the implementation scans all segments and then replaces sealed segments. This can duplicate active data and race with ongoing writes.~~
- **Fix:** Added `scan_sealed()` method that explicitly excludes the active segment. Introduced a `compaction_lock` in `SegmentManager` to coordinate between compaction and segment sealing. The `compact_sealed()` method performs atomic compaction: it acquires the lock, scans sealed segments, applies compaction logic, and replaces segments atomically. `seal_and_rotate()` now also acquires this lock, preventing segments from being sealed during compaction. Comprehensive tests verify that active segment data is preserved during compaction.

### Medium

8) ~~Manifest encoding is non-deterministic.~~ **RESOLVED**
- ~~Manifest serialization iterates a HashMap without sorting, so identical operations can produce different bytes on disk (AC-01 violation).~~
- **Fix:** The `Manifest` struct now uses `BTreeMap<String, u32>` for collections (ensures bytewise key ordering). Index definitions are sorted by ID before encoding. A comprehensive `deterministic_encoding` test verifies that manifests with identical logical state produce byte-identical encodings regardless of construction order.

9) ~~Collection creation persistence is best-effort and silent on failure.~~ **RESOLVED**
- ~~If manifest save fails, collection creation still succeeds without reporting the failure, risking a mismatch between in-memory state and persisted metadata.~~
- ~~Evidence: `crates/entidb_core/src/database.rs:563`.~~
- **Fix:** Added `create_collection(name) -> CoreResult<CollectionId>` as the recommended API:
  - Returns an error with `ManifestPersistFailed` if the manifest cannot be saved
  - Rolls back in-memory state on failure (removes collection from manifest, restores `next_collection_id`)
  - Existing `collection()` method is deprecated and now panics on persistence failure instead of silently continuing
  - Added `collection_unchecked()` for backward compatibility with explicit warning about potential inconsistency
  - Comprehensive tests verify: idempotent creation, immediate persistence, rollback behavior, and in-memory database handling

### Low

10) WAL replay uses full in-memory loading only.
- `WalManager::read_all()` reads the entire WAL into memory; there is no streaming replay. This is a scaling risk for large WALs.
- Evidence: `crates/entidb_core/src/wal/writer.rs:95`.

## Testing gaps

- No tests assert snapshot isolation (repeatable reads) or that earlier snapshots ignore later commits.
- No tests verify WAL durability semantics with fsync/sync guarantees.
- ~~No tests assert manifest determinism (byte-for-byte stability across runs).~~ **ADDED: `deterministic_encoding` test**
- No tests assert index rebuild/derivability or that index updates are atomic with commit.

## Questions / assumptions

- Is EncryptedBackend intended to be production-ready or a placeholder? If placeholder, it should be gated or documented as non-compliant.
- Should snapshot isolation be implemented via MVCC (sequence filters) or via per-transaction view of the segment index?
- Is the index API intentionally exposing index names to users, or is it meant for internal/testing only? If internal, it should be hidden from public API and bindings.

## Strengths

- Clear architectural docs and acceptance criteria are present and detailed.
- WAL record encoding and CRC checks are implemented and tested.
- Segment compaction logic is deterministic and well-structured at the record level.
- The change feed API correctly emits Insert/Update/Delete operation types with comprehensive test coverage.

## Summary

The repository has strong documentation and a credible core layout, but the implementation currently violates several non-negotiable invariants (snapshot isolation, WAL durability, index semantics, and file format alignment). These are architectural issues, not polish items. If this is an alpha prototype, it needs explicit scoping in docs; otherwise, these gaps are release blockers.
