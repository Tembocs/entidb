# EntiDB Global Invariants (Normative)

This document defines **global invariants** that **MUST ALWAYS hold** in any correct implementation of EntiDB.

These invariants are **cross-cutting**: they apply regardless of language, platform (native or web), binding, or build mode. Any violation constitutes **database corruption or undefined behavior**.

This document is **normative and authoritative**.

---

## 1. Fundamental system invariants

### 1.1 Single source of truth

* The **canonical persisted state** of EntiDB **MUST** be derivable solely from:

  * the MANIFEST
  * sealed SEGMENTS
  * committed WAL records
* No in-memory-only state **MAY** affect persisted correctness.

### 1.2 Determinism

* Given identical inputs (operations, order, payload bytes), EntiDB **MUST** produce identical persisted bytes.
* Non-deterministic behavior (timestamps, random IDs, hash iteration order) **MUST NOT** influence storage layout.

### 1.3 Crash safety

* After any crash, power loss, or process termination, the database **MUST** be recoverable to the last committed state.
* Recovery **MUST NOT** require heuristics, user intervention, or best-effort guesses.

---

## 2. Identity and entity invariants

### 2.1 Entity identity

* Every entity **MUST** have a stable, immutable `EntityId`.
* `EntityId` **MUST NOT** be reused within a database.

### 2.2 Collection isolation

* An entity **MUST** belong to exactly one collection.
* Entity IDs **MUST NOT** collide across collections at the storage-key level.

### 2.3 Entity visibility

* At any snapshot, for a given `(collection_id, entity_id)`, **at most one logical version** is visible.

---

## 3. Transaction invariants

### 3.1 Atomicity

* A transaction’s effects are **all-or-nothing**.
* Partial application of a transaction **MUST NOT** be observable.

### 3.2 Isolation

* Readers **MUST** observe a consistent snapshot.
* Readers **MUST NOT** observe intermediate states of a writer.

### 3.3 Durability

* Once a commit is acknowledged, its effects **MUST** survive crashes.

### 3.4 Commit ordering

* Transactions **MUST** be totally ordered by commit sequence number.
* Commit order **MUST** define visibility order.

---

## 4. WAL invariants

### 4.1 Append-only

* WAL files **MUST** be append-only.
* WAL records **MUST NOT** be mutated after write.

### 4.2 Commit rule

* A transaction **MUST NOT** be considered committed unless a valid COMMIT record exists and has been flushed.

### 4.3 Replay correctness

* WAL replay **MUST** be idempotent.
* Replaying WAL multiple times **MUST NOT** change final state beyond the first replay.

---

## 5. Segment invariants

### 5.1 Immutability

* Sealed segments **MUST** be immutable.
* Any modification to a segment file **MUST** invalidate the database.

### 5.2 Logical dominance

* For any entity, the version with the highest commit sequence number **MUST** dominate older versions.

### 5.3 Tombstones

* A tombstone **MUST** suppress all earlier versions of the same entity.

---

## 6. Index invariants

### 6.1 Derivability

* Index state **MUST** be fully derivable from segments and WAL.
* Index corruption **MUST NOT** corrupt entity data.

### 6.2 Transactional consistency

* Index updates **MUST** be applied atomically with the transaction commit.

### 6.3 Rebuild correctness

* Rebuilding an index **MUST** produce the same lookup results as incremental maintenance.

---

## 7. Canonical encoding invariants

### 7.1 Canonical CBOR

* Persisted entity payloads **MUST** be canonical CBOR.
* Equivalent entities **MUST** produce identical CBOR bytes.

### 7.2 Hash stability

* Hashes **MUST** be computed over canonical bytes only.

---

## 8. Change feed and sync invariants

### 8.1 Commit-only emission

* Change feed events **MUST** be emitted only after commit.

### 8.2 Ordering

* Change feed **MUST** preserve commit order.

### 8.3 Idempotency

* Applying the same logical operation multiple times **MUST NOT** change final state.

---

## 9. Storage backend invariants

### 9.1 Byte-store abstraction

* Storage backends **MUST** be treated as opaque byte stores.
* No backend-specific semantics **MAY** leak into core logic.

### 9.2 Durability contract

* When a backend reports flush success, data **MUST** be durable according to backend guarantees.

---

## 10. Binding invariants

### 10.1 Semantic parity

* Rust, Dart, and Python bindings **MUST** expose identical observable behavior.

### 10.2 Safety

* Memory safety violations across FFI boundaries **MUST NOT** occur.

---

## 11. Web-specific invariants

### 11.1 Persistence realism

* Browser storage **MUST** be treated as unreliable.
* All recovery invariants **MUST** hold despite abrupt tab termination.

### 11.2 Isolation

* Web execution **MUST NOT** weaken transactional or durability guarantees.

---

## 12. Forbidden behaviors (absolute)

The following are **strictly forbidden**:

* Introducing SQL, SQL-like APIs, or DSLs
* Depending on any external database engine
* Making persistence optional or best-effort
* Allowing binding-specific semantics
* Allowing partial commits to be observable

Any implementation exhibiting these behaviors is **non-compliant**.
