# EntiDB Production-Readiness TODO (31 Dec 2025)

This document captures the highest-impact production readiness findings from a deep repo review, and the single best proposed fix for each (no alternative options listed).

Scope: Rust core + storage + bindings surface, focusing on crash safety, determinism, robustness, and security properties described in the normative docs.

## 1) Panics/unwraps in production code paths

**Finding**

- Public-facing code paths still contain `panic!`, `unwrap()`, and `expect()`.
- The most concerning instance is `Database::collection()` which panics if manifest persistence fails (disk full / permissions / fsync failure), and is used in tests/examples.

**Why it matters**

- Panics in library code are process-killing failures and are not acceptable for production-grade storage engines. These failures should be typed, recoverable errors.

**Best solution**

- Enforce a hard policy: `panic!`/`unwrap`/`expect` are forbidden in all non-test, non-example, non-benchmark code.
- Keep `Database::collection()` as deprecated but make it internally call `create_collection()` and *never* panic; if it cannot return `Result` for backward-compat, it must return a stable sentinel error value *or* be removed at the next semver major.
  - Immediate best fix (while preserving API): keep `Database::collection()` deprecated and ensure it does not panic; use `create_collection()` in all internal code paths and update examples/tests to prefer it.

## 2) Checkpoint crash-window can amplify segment growth

**Finding**

- Checkpoint ordering is correct (segments synced → checkpoint WAL record → manifest save → WAL truncation).
- However, recovery replays WAL operations for all committed transactions without skipping transactions at/below `manifest.last_checkpoint`.
- If the process crashes after manifest save but before WAL truncation, reopening may re-append already-checkpointed committed operations into segments. This is logically idempotent (latest-wins MVCC) but can bloat storage repeatedly.

**Why it matters**

- Repeated power loss/crash events can cause unbounded segment growth, which becomes an operational reliability issue (disk pressure) and can degrade performance.

**Best solution**

- During recovery replay, skip applying any committed transaction with `commit_seq <= checkpoint_seq`.
  - The manifest’s `last_checkpoint` is the authoritative lower bound of “already materialized into segments”.
  - This makes recovery idempotent *and* prevents crash-amplified segment duplication.

## 3) Encryption-at-rest: deterministic AES-GCM nonce reuse hazard

**Finding**

- `EncryptedBackend` uses deterministic nonces derived from `(key, block_number)` to satisfy determinism (AC-01).
- The backend supports `truncate(0)` and then writing again, which will reuse block numbers with the same key.
- With AES-GCM, nonce reuse under the same key across different plaintext is catastrophic for confidentiality and integrity.

**Why it matters**

- This is a security vulnerability if `EncryptedBackend` is used in production scenarios where truncation/reuse can happen (not just theoretical).

**Best solution**

- Redesign encryption to avoid nonce-reuse risk under truncation and rewrites:
  - Replace AES-GCM with a misuse-resistant AEAD (e.g., AES-GCM-SIV) *and* include an epoch/stream identifier stored in the header that changes on truncate/reinit.
  - Ensure that after any truncate/reinit, the nonce derivation changes (epoch increments), guaranteeing nonces are never reused for different plaintext under the same key.
- Additionally, document and enforce that encrypted backends must not be used for workloads that rewrite the same block number unless the scheme is misuse-resistant.

## 4) Index definition persistence gap

**Finding**

- Indexes are rebuilt from segments on open, but index *definitions* created via the new `IndexEngine` API are not persisted to the manifest yet (there is an ignored test documenting this gap).

**Why it matters**

- After restart, the database can lose knowledge of what indexes should exist. This becomes a correctness/performance and operability issue.

**Best solution**

- Persist index definitions in the manifest at index creation/removal time (transactionally with respect to the operation) and reload them on open.
- Make index definitions part of the durable metadata contract.

## 5) Transaction conflict detection is TODO (`before_hash`)

**Finding**

- WAL records support `before_hash?` but current commit logic writes `before_hash: None` (TODO in code).

**Why it matters**

- Today’s single-writer model reduces risk, but conflict detection is important for future sync/conflict semantics and for preventing silent lost-update patterns as concurrency features expand.

**Best solution**

- Implement optimistic conflict detection:
  - Store a stable hash over canonical entity bytes for the “before” version.
  - On commit, verify that the expected `before_hash` matches the current visible version at the transaction’s snapshot.
  - If mismatch, abort commit with a typed conflict error.

## 6) Recovery strictness vs truncation behavior

**Finding**

- WAL iteration treats truncated tail records as clean end-of-log and treats CRC mismatch as corruption (error).

**Why it matters**

- This is good practice; however, the behavior should be explicitly documented so operators know what failures are fatal (checksum mismatch) vs tolerated (clean truncation at end).

**Best solution**

- Document the recovery policy clearly (fatal vs tolerated conditions) and add a targeted test that validates:
  - Truncated tail is tolerated and does not apply partial txn.
  - CRC mismatch aborts open.

## “Go/No-Go” checklist for starting implementation

- [ ] Enforce no `panic!`/`unwrap`/`expect` in non-test code (repo policy + CI gate).
- [ ] Recovery skips re-applying txns with `commit_seq <= last_checkpoint`.
- [ ] Encryption backend redesigned to prevent nonce reuse under truncation/rewrites.
- [ ] Index definitions persisted in manifest and restored on open.
- [ ] Conflict detection implemented (before-hash + typed conflict error).
- [ ] Recovery policy documented + regression tests.
