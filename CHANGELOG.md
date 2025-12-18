# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-XX-XX

### Added

- **Core Engine**
  - Entity storage with canonical CBOR encoding
  - ACID transactions with WAL-based durability
  - Snapshot isolation for concurrent reads
  - Single writer, multiple readers concurrency model

- **Storage Layer**
  - `StorageBackend` trait for pluggable storage
  - `FileBackend` for native file system storage
  - `MemoryBackend` for testing

- **Indexing**
  - Hash indexes for equality lookups
  - BTree indexes for range queries and ordered traversal

- **Encryption** (optional feature)
  - AES-256-GCM encryption at rest
  - Per-database master key support

- **Backup & Restore**
  - Full database backups
  - Point-in-time restore
  - Backup validation

- **Migrations**
  - Schema migration framework
  - Migration history tracking

- **CLI Tool** (`entidb`)
  - `inspect`: Database inspection
  - `verify`: Integrity verification
  - `compact`: Segment compaction
  - `dump-oplog`: Operation log export
  - `backup`: Backup management
  - `migrate`: Migration management

- **Bindings**
  - Dart FFI bindings (`entidb_dart`)
  - Python bindings via PyO3 (`entidb`)

- **Sync Protocol**
  - Pull-then-push synchronization
  - Conflict detection and resolution
  - Change feed emission

### Security

- Strict input validation on all CBOR decoding
- Checksum verification for WAL and segments
- No heuristic recovery from corruption

[Unreleased]: https://github.com/Tembocs/entidb/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Tembocs/entidb/releases/tag/v0.1.0
