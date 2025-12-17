# File Format Specification (Normative)

This document is **normative**. Any implementation of EntiDB **MUST** conform exactly to the formats and invariants described here.

---

## 1. Versioning and compatibility

### 1.1 Global format version

* Every EntiDB storage directory **MUST** contain a single authoritative format version.
* Version is encoded as a tuple: `(major, minor)`.
* **Major** version mismatch ⇒ database **MUST NOT** open.
* **Minor** version mismatch ⇒ database **MAY** open read-only or perform controlled upgrade.

### 1.2 Forward and backward guarantees

* WAL and segment readers **MUST** ignore unknown record types with higher minor versions.
* Unknown fields **MUST NOT** alter interpretation of known fields.

---

## 2. Storage layout

```
entidb/
├─ MANIFEST
├─ WAL/
│  ├─ wal-000001.log
│  ├─ wal-000002.log
├─ SEGMENTS/
│  ├─ seg-000001.dat
│  ├─ seg-000002.dat
└─ LOCK
```

### 2.1 MANIFEST

* Stores metadata only.
* Written atomically using write-then-rename.
* Contains:

  * format version
  * collection registry
  * index registry
  * encryption metadata (never keys)

### 2.2 LOCK

* Advisory lock.
* Enforces single-writer invariant.

---

## 3. WAL format

### 3.1 WAL invariants

* WAL is **append-only**.
* WAL **MUST** be flushed to durable storage before commit acknowledgment.

### 3.2 WAL record envelope

```
| magic (4) | version (2) | type (1) | length (4) | payload (N) | crc32 (4) |
```

### 3.3 WAL record types

* `BEGIN(txid)`
* `PUT(collection_id, entity_id, before_hash?, after_bytes)`
* `DELETE(collection_id, entity_id, before_hash?)`
* `COMMIT(txid)`
* `ABORT(txid)`
* `CHECKPOINT(marker)`

### 3.4 Recovery rules

* Only transactions with a valid `COMMIT` record **MUST** be applied.
* Partial or aborted transactions **MUST NOT** affect state.

---

## 4. Segment format

### 4.1 Segment invariants

* Segments are immutable after sealing.
* Records are append-only.

### 4.2 Segment record

```
| record_len (4) | collection_id (u32) | entity_id (128bit) | flags (1) | payload (N) | checksum (4) |
```

Flags:

* `0x01` = tombstone
* `0x02` = encrypted

### 4.3 Canonical payload

* Payload **MUST** be canonical CBOR bytes.

---

## 5. Compaction rules

* Compaction **MUST NOT** change logical state.
* Latest committed version per `(collection_id, entity_id)` wins.
* Tombstones older than retention window **MAY** be dropped.

---

## 6. Corruption handling

* Any checksum failure **MUST** abort open.
* Recovery **MUST NOT** attempt heuristic repair.
