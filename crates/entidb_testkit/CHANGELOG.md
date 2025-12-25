# Changelog

All notable changes to the `entidb_testkit` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-alpha.1] - 2025-12-25

### Added

- **Test Utilities**
  - Temporary database creation helpers
  - Test data generators
  - Assertion helpers for EntiDB types

- **Golden Tests**
  - File format golden test framework
  - WAL record serialization tests
  - Segment record serialization tests

- **Fuzz Harnesses**
  - CBOR encoder/decoder fuzzing
  - Storage backend fuzzing
  - Transaction sequence fuzzing

- **Property-Based Testing**
  - Quickcheck/proptest integration
  - Invariant verification helpers
  - Roundtrip testing utilities

- **Test Vectors**
  - Cross-language CBOR test vectors
  - EntityId serialization vectors
  - Segment format vectors
