# Findings

- Critical: commit can return an error after the WAL commit is durable, leaving the user thinking the transaction failed while recovery will still apply it. This also allows partial segment writes if an append fails after the WAL commit is persisted. Location: `crates/entidb_core/src/transaction/manager.rs:141`.
- High: checkpoint truncates the WAL before the manifest is updated; a crash between `wal.clear()` and manifest save can regress `committed_seq`, hiding committed data on restart. Locations: `crates/entidb_core/src/transaction/manager.rs:310`, `crates/entidb_core/src/database.rs:840`.
- High: "read-only" transactions can write and multiple writers can overlap (lock only acquired at commit). With no conflict detection, this violates the single-writer guarantee and can cause write skew or lost updates. Locations: `crates/entidb_core/src/transaction/manager.rs:76`, `crates/entidb_core/src/transaction/state.rs:219`.
- Medium: if segment file creation fails, the system silently falls back to an in-memory backend, breaking durability and explicit behavior guarantees. Location: `crates/entidb_core/src/database.rs:226`.
- Low: WAL decoding accepts trailing bytes for fixed-size record types (no cursor/end validation), allowing some payload corruption to be treated as valid. Location: `crates/entidb_core/src/wal/record.rs:253`.

# Open Questions / Assumptions

- Is it intended that `Transaction::put/delete` is callable on `begin()` transactions, or should those APIs be restricted to `WriteTransaction` only?
- Should a failed segment append after WAL commit be treated as a fatal error (e.g., mark DB as unhealthy) rather than returning `Err` while the transaction is already committed?

# Testing Gaps

- No tests simulate crash between WAL truncation and manifest save (checkpoint durability invariant).
- No tests cover commit failures after WAL flush or write-write conflicts from overlapping writers.


# Proposed Solutions

- Commit error after WAL durability (Critical)
  - Change commit flow to make the post-WAL stage idempotent and recoverable.
  - Recommended approach: write a "COMMIT" record with sequence, flush WAL, then apply to segments and flush segments; if any segment append fails, return a "commit accepted, apply pending" error and mark DB as needing recovery. Recovery should re-apply committed WAL entries deterministically and complete the missing segment writes.
  - Add an explicit "commit completion" marker or persist a "replay required" flag in the manifest to make the state machine explicit. Do not acknowledge a hard failure after WAL commit unless the DB is marked unhealthy and requires recovery.
  - Add a test that forces segment append failure after WAL commit and validates recovery applies the transaction exactly once.

- Checkpoint truncation before manifest update (High)
  - Make manifest update the durable source of the checkpoint sequence before WAL truncation.
  - Recommended ordering: flush segments -> write checkpoint record -> flush WAL -> update manifest with last_checkpoint -> fsync manifest -> truncate WAL.
  - If manifest update fails, do not clear the WAL; surface the error and leave recovery intact.
  - Add a crash simulation test (or unit test using a failpoint) between manifest save and WAL truncation to confirm correctness.

- Writer exclusivity and conflict detection (High)
  - Enforce the single-writer rule at transaction start, not at commit: `begin()` must be read-only and `begin_write()` must be the only path to mutations.
  - Make `Transaction` immutable for writes (hide or gate `put/delete` behind a write-transaction capability), and ensure the write lock is held for the full write transaction lifetime.
  - Implement conflict detection based on the recorded read set or a version/hash check (before_hash), and fail conflicting commits deterministically.
  - Add tests for overlapping writers (write skew) that should deterministically fail or serialize.

- Silent fallback to in-memory segment backend (Medium)
  - Remove the fallback to in-memory for file backend creation errors; instead return a hard error and keep the database closed.
  - If a fallback is required for tests, make it opt-in via explicit configuration so behavior remains predictable.
  - Add a test that simulates file creation failure and ensures the open call fails with a clear error.

- WAL decoding ignores trailing bytes (Low)
  - Validate that the decode cursor reaches the end of the payload for fixed-size records (Begin/Commit/Abort/Checkpoint/Delete without after_bytes).
  - If trailing bytes exist, treat the record as corrupted to prevent silent acceptance of malformed data.
  - Add a test that appends extra bytes to a fixed-size record payload and ensures decode fails.

