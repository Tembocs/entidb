# EntiDB Global Acceptance Criteria (Normative)

This document defines **global, system-level acceptance criteria** for EntiDB.

Acceptance criteria specify **what must be demonstrably true** for EntiDB to be considered a **correct, complete, and compliant implementation** of the architecture and invariants defined elsewhere.

These criteria are intentionally **outcome-focused**, not implementation-specific. They apply across all languages, platforms, and deployment modes.

This document is **normative**.

---

## 1. Definition of acceptance

An EntiDB implementation is **ACCEPTED** if and only if **all criteria in this document are satisfied**.

Failure to meet any criterion indicates:

* architectural non-compliance, or
* data corruption risk, or
* semantic divergence across bindings.

---

## 2. Global correctness criteria

### 2.1 Deterministic behavior

**AC-01**
Given identical sequences of operations and identical entity payloads, EntiDB **MUST** produce identical persisted bytes and identical observable behavior.

Verification:

* Repeat the same operation log twice on fresh databases.
* Compare segment and WAL byte-level output.

---

### 2.2 Crash safety

**AC-02**
After any crash or forced termination at any instruction boundary, EntiDB **MUST** recover to the last committed state.

Verification:

* Inject crashes during:

  * WAL append
  * commit flush
  * compaction
* Validate post-recovery state against expected snapshot.

---

### 2.3 Absence of partial state

**AC-03**
At no time **MAY** partially committed data be observable by any reader.

Verification:

* Concurrent read during write transactions.
* Ensure readers only see pre-commit or post-commit state.

---

## 3. Persistence and storage criteria

### 3.1 Durability

**AC-04**
Once a transaction commit is acknowledged, its effects **MUST** survive power loss, process termination, and restart.

Verification:

* Commit data.
* Terminate process without graceful shutdown.
* Restart and verify data presence.

---

### 3.2 Storage independence

**AC-05**
EntiDB **MUST NOT** depend on any external database engine for persistence.

Verification:

* Inspect dependency graph.
* Ensure only byte-store backends are used.

---

## 4. Transaction and concurrency criteria

### 4.1 Isolation

**AC-06**
Concurrent readers **MUST** observe consistent snapshots.

Verification:

* Long-running read during concurrent write.
* Verify snapshot stability.

---

### 4.2 Commit ordering

**AC-07**
Transaction commit order **MUST** define global visibility order.

Verification:

* Interleave transactions.
* Verify monotonic commit sequence numbers.

---

## 5. Data model and entity criteria

### 5.1 Entity identity

**AC-08**
Each entity **MUST** have a stable, immutable identity for its lifetime.

Verification:

* Update entity repeatedly.
* Verify identity remains unchanged.

---

### 5.2 Collection isolation

**AC-09**
Entities from different collections **MUST NOT** interfere or collide.

Verification:

* Create identical IDs in separate collections.
* Validate isolation.

---

## 6. Indexing and access-path criteria

### 6.1 Correctness over performance

**AC-10**
Index usage **MUST NOT** change query results.

Verification:

* Compare results with and without indexes.

---

### 6.2 Scan transparency

**AC-11**
Full scans **MUST** be detectable and observable via telemetry or explicit API choice.

Verification:

* Trigger scan.
* Confirm visibility in diagnostics.

---

## 7. Encoding and format criteria

### 7.1 Canonical encoding

**AC-12**
Equivalent entities **MUST** encode to identical canonical CBOR bytes across languages.

Verification:

* Encode entity in Rust, Dart, Python.
* Compare byte arrays.

---

## 8. Change feed and synchronization criteria

### 8.1 Commit-only propagation

**AC-13**
Only committed changes **MUST** appear in the change feed and sync stream.

Verification:

* Abort transactions.
* Ensure no aborted ops propagate.

---

### 8.2 Idempotent replication

**AC-14**
Applying the same logical operation multiple times **MUST NOT** change final state.

Verification:

* Replay identical sync ops.
* Validate state stability.

---

## 9. Binding parity criteria

### 9.1 Semantic equivalence

**AC-15**
Rust, Dart, and Python bindings **MUST** exhibit identical observable behavior.

Verification:

* Execute identical test suite across bindings.

---

### 9.2 API integrity

**AC-16**
No binding **MAY** expose SQL, SQL-like, or DSL-based querying.

Verification:

* API inspection.

---

## 10. Web-specific criteria

### 10.1 Persistence reliability

**AC-17**
Web builds **MUST** meet the same durability and recovery criteria as native builds.

Verification:

* Simulate tab termination.
* Reload and verify data integrity.

---

### 10.2 Backend abstraction compliance

**AC-18**
Browser storage **MUST** be used strictly as a byte store, with no database semantics assumed.

Verification:

* Inspect backend implementation.

---

## 11. Prohibited behavior checks

### 11.1 Forbidden features

**AC-19**
An implementation **MUST NOT**:

* introduce SQL, SQL-like APIs, or DSLs
* depend on external database engines
* expose partial state
* diverge across bindings

Verification:

* Code review and dependency audit.

---

## 12. Acceptance conclusion

An EntiDB implementation is **ACCEPTED** only when **all criteria AC-01 through AC-19 are satisfied**.

Any failure constitutes **non-compliance** and **MUST** be corrected before release.
