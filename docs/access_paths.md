# Index Selection and Access Path Policy (Normative)

This document defines how EntiDB selects access paths **without exposing any query language**.

---

## 1. Fundamental rule

* The engine, not the user, selects access paths.
* Users **MUST NOT** reference indexes by name during queries.

---

## 2. Access path types

### 2.1 Full scan

* Iterates entire collection.
* **MUST** be explicit in API.

### 2.2 Hash index access

* Equality-based lookup.
* Used when predicate is exact match.

### 2.3 BTree index access

* Range-based lookup.
* Used for ordered comparisons.

---

## 3. Selection rules

1. If an equality predicate exists and hash index exists → use hash index.
2. Else if range predicate exists and BTree exists → use BTree.
3. Else → full scan.

---

## 4. Scan safety

* Engine **MUST** expose telemetry for scans.
* Configuration **MAY** forbid scans in production mode.

---

## 5. Determinism

* Given identical schema and predicates, access path selection **MUST** be deterministic.
