# Transaction and Isolation Specification (Normative)

This document defines **exact transactional semantics** for EntiDB.

---

## 1. Transaction model

* EntiDB **MUST** implement ACID transactions.
* Only one write transaction **MUST** be active at any time.
* Multiple readers **MAY** exist concurrently.

---

## 2. Isolation level

### 2.1 Guaranteed isolation

* Snapshot Isolation is **MANDATORY**.

Readers:

* Observe a consistent snapshot at transaction start.
* Never see partial commits.

Writers:

* See their own writes.
* Do not see concurrent writes.

### 2.2 Forbidden anomalies

* Dirty reads: **FORBIDDEN**
* Non-repeatable reads: **FORBIDDEN**
* Phantom writes: **FORBIDDEN**

---

## 3. Transaction lifecycle

```
BEGIN → MUTATE → COMMIT | ABORT
```

### 3.1 BEGIN

* Allocates txid.
* Captures snapshot pointer.

### 3.2 COMMIT

* Writes COMMIT record to WAL.
* Flushes WAL.
* Makes transaction visible atomically.

### 3.3 ABORT

* Writes ABORT record.
* Discards uncommitted changes.

---

## 4. Visibility rules

* Changes become visible **only after commit**.
* Commit is atomic with respect to readers.

---

## 5. Failure semantics

* Crash before COMMIT ⇒ transaction discarded.
* Crash after COMMIT ⇒ transaction applied exactly once.

---

## 6. Nested transactions

* Nested transactions **MUST NOT** be supported.
* Savepoints **MAY** be added in future revisions.
