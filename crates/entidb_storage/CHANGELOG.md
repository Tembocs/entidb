# Changelog

All notable changes to the `entidb_storage` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-XX-XX

### Added

- `StorageBackend` trait for pluggable storage implementations
- `FileBackend` for native file system storage with:
  - Atomic append operations
  - Durable flush support
  - Advisory file locking
- `MemoryBackend` for testing and ephemeral storage
- `EncryptedBackend` wrapper for transparent encryption:
  - AES-256-GCM encryption
  - Block-based encryption with configurable block size
  - Secure key management interface
- Storage error types with comprehensive error handling
- Thread-safe implementations using `Arc` and `RwLock`
