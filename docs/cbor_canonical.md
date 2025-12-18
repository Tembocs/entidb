# Canonical CBOR Specification (Normative)

This document defines canonical CBOR rules for EntiDB.

---

## 1. Purpose

Canonical CBOR ensures:

* Deterministic storage
* Stable hashing
* Cross-language equivalence

---

## 2. Encoding rules

* Maps **MUST** be sorted by key (bytewise).
* Integers **MUST** use shortest encoding.
* Floats **MUST NOT** be used unless explicitly allowed.
* Strings **MUST** be UTF-8.

---

## 3. Forbidden constructs

* Indefinite-length items: **FORBIDDEN**
* NaN values: **FORBIDDEN**

---

## 4. Hash stability

* Hash of entity **MUST** be computed over canonical CBOR bytes.

---

## 5. Test vectors

* Each language binding **MUST** pass identical CBOR test vectors.
