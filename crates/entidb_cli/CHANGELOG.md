# Changelog

All notable changes to the `entidb_cli` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-alpha.1] - 2025-12-25

### Added

- **Commands**
  - `inspect` - Database inspection and statistics
  - `verify` - Integrity verification with checksum validation
  - `compact` - Segment compaction with configurable options
  - `dump-oplog` - Export operation log to JSON
  - `backup` - Create database backups
  - `restore` - Restore from backup files
  - `migrate` - Run schema migrations

- **Output Formats**
  - Human-readable text output (default)
  - JSON output with `--json` flag
  - Verbose mode with `--verbose`

- **Options**
  - Database path specification
  - Encryption key support
  - Dry-run mode for destructive operations
