# FFI and Bindings Contract (Normative)

This document defines the binding contract between Rust core and Dart/Python.

---

## 1. ABI stability

* Rust core **MUST** expose a stable C ABI.
* ABI changes **MUST** be versioned.

---

## 2. Memory ownership

* Rust owns all buffers it allocates.
* Bindings **MUST** explicitly free buffers via exported functions.
* No implicit GC interaction is allowed.

---

## 3. Error handling

* Errors **MUST** be returned as numeric codes.
* Human-readable messages **MAY** be provided separately.

---

## 4. Semantic parity

* Dart and Python bindings **MUST** expose identical behavior.
* No binding-specific shortcuts are allowed.

---

## 5. Conformance testing

* A shared test suite **MUST** validate parity across Rust, Dart, and Python.
