# Changelog

All notable changes to the `entidb` Python package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-alpha.1] - 2025-12-25

### Added

- **Database Operations**
  - `Database.open()` / `Database.open_in_memory()` / `close()`
  - `checkpoint()` for durability
  - `stats()` for database statistics
  - Context manager support (`with` statement)

- **Entity Operations**
  - `put()` / `get()` / `delete()` operations
  - `scan()` for collection iteration
  - `collection()` for named collections
  - `EntityIterator` for lazy iteration

- **Transactions**
  - `transaction()` with callback-based API
  - Snapshot isolation support
  - Automatic commit/rollback

- **Indexing**
  - Hash index creation and querying
  - BTree index creation and range queries
  - Index insert/remove operations

- **Backup & Restore**
  - `backup()` / `restore()` operations
  - `validate_backup()` for backup verification
  - `backup_with_options()` for custom backups

- **Compaction**
  - `compact()` with configurable options
  - `CompactionStats` for operation results

- **Change Feed**
  - `poll_changes()` for polling changes since cursor
  - `latest_sequence` property
  - `ChangeEvent` and `ChangeType` classes

- **Schema Version**
  - `schema_version` getter/setter property
  - User-managed schema versioning for migrations

- **Types**
  - `EntityId` with UUID-like generation
  - `Collection` for entity grouping
  - `Transaction` for transactional operations
  - Various statistics classes (`DatabaseStats`, `RestoreStats`, etc.)
