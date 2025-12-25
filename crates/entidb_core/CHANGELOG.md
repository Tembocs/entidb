# Changelog

All notable changes to the `entidb_core` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-XX-XX

### Added

- **Database Engine**
  - `Database` struct with open/close/checkpoint operations
  - In-memory and file-based storage support
  - Thread-safe concurrent access

- **Entity Storage**
  - `EntityId` 128-bit unique identifiers
  - `CollectionId` for entity grouping
  - Put/get/delete operations for raw CBOR payloads
  - Typed collection facade with `EntityCodec` trait

- **Transactions**
  - ACID transactions with WAL-based durability
  - Snapshot isolation for concurrent reads
  - Single writer, multiple readers model
  - Transaction manager with begin/commit/abort

- **Write-Ahead Log (WAL)**
  - Append-only WAL with idempotent replay
  - Crash recovery from WAL
  - Checkpoint and WAL truncation

- **Segment Management**
  - Immutable sealed segments
  - Segment compaction with tombstone handling
  - Latest version dominance rule

- **Indexing**
  - Hash indexes for O(1) equality lookups
  - BTree indexes for range queries
  - Full-text search index (FTS) foundation
  - Atomic index updates with transaction commit

- **Change Feed**
  - Subscriber-based change notifications
  - Polling API with cursor support
  - History buffer for catch-up scenarios

- **Backup & Restore**
  - Full database backup to bytes
  - Restore with merge semantics
  - Backup validation without restore

- **Compaction**
  - Segment merging with obsolete version removal
  - Optional tombstone removal
  - Space reclamation statistics

- **Statistics**
  - Operation counters (reads, writes, deletes)
  - Transaction statistics
  - Entity count tracking
