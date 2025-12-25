# Changelog

All notable changes to the `entidb_sync_protocol` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-XX-XX

### Added

- **Protocol Types**
  - `SyncOperation` for representing sync operations
  - `OperationType` enum (Put, Delete)
  - `DeviceId` and `DatabaseId` identifiers
  - `ServerCursor` for tracking sync position

- **Change Feed Types**
  - `ChangeEvent` struct
  - `ChangeFeed` for buffering changes
  - `ChangeType` enum (Insert, Update, Delete)

- **CBOR Encoding**
  - Canonical CBOR encoding for all protocol types
  - Deterministic serialization
  - Test vectors for cross-language compatibility

- **Request/Response Types**
  - Pull request/response structures
  - Push request/response structures
  - Error response types
