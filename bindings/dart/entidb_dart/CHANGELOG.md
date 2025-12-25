# Changelog

All notable changes to the `entidb_dart` package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-alpha.1] - 2025-12-25

### Added

- **Database Operations**
  - `Database.open()` / `Database.openInMemory()` / `close()`
  - `checkpoint()` for durability
  - `stats()` for database statistics
  - `isOpen` property

- **Entity Operations**
  - `put()` / `get()` / `delete()` operations
  - `scan()` for collection iteration
  - `collection()` for named collections

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
  - `validateBackup()` for backup verification
  - `backupWithOptions()` for custom backups

- **Compaction**
  - `compact()` with configurable options
  - `CompactionStats` for operation results

- **Change Feed**
  - `pollChanges()` for polling changes since cursor
  - `latestSequence` property
  - `ChangeEvent` and `ChangeType` classes

- **Schema Version**
  - `schemaVersion` getter/setter
  - User-managed schema versioning for migrations

- **Types**
  - `EntityId` with UUID-like generation
  - `Collection` for entity grouping
  - `Transaction` for transactional operations
  - Various statistics classes
