# Changelog

All notable changes to the `entidb_ffi` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-alpha.1] - 2025-12-25

### Added

- **Stable C ABI**
  - `EntiDbHandle` opaque database handle
  - `EntiDbResult` error codes with thread-local error messages
  - Memory-safe buffer management

- **Database Operations**
  - `entidb_open` / `entidb_open_in_memory` / `entidb_close`
  - `entidb_checkpoint` for durability
  - `entidb_stats` for statistics

- **Entity Operations**
  - `entidb_put` / `entidb_get` / `entidb_delete`
  - `entidb_scan` for collection iteration
  - Collection management

- **Transaction Operations**
  - `entidb_begin` / `entidb_commit` / `entidb_abort`
  - Snapshot isolation support

- **Index Operations**
  - Hash and BTree index creation/deletion
  - Index insert/remove/lookup
  - Range queries for BTree indexes

- **Backup & Restore**
  - `entidb_backup` / `entidb_restore`
  - `entidb_validate_backup`
  - Backup with options (include tombstones)

- **Compaction**
  - `entidb_compact` with configurable options
  - Compaction statistics

- **Change Feed**
  - `entidb_poll_changes` for polling changes since cursor
  - `entidb_latest_sequence` for current sequence
  - `entidb_free_change_events` for memory cleanup

- **Schema Version**
  - `entidb_get_schema_version` / `entidb_set_schema_version`
  - User-managed schema versioning for migrations

- **Type Definitions**
  - `EntiDbEntityId` (128-bit)
  - `EntiDbCollectionId` (32-bit)
  - `EntiDbBuffer` for byte arrays
  - `EntiDbChangeEvent` / `EntiDbChangeEventList` for change feed
  - Various statistics structs
