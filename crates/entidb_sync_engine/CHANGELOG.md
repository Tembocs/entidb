# Changelog

All notable changes to the `entidb_sync_engine` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-alpha.1] - 2025-12-25

### Added

- **Sync Engine State Machine**
  - State transitions: Idle → Connecting → Pulling → Pushing → Synced
  - Error handling and retry logic
  - Progress callbacks

- **Pull Operations**
  - Fetch remote changes since cursor
  - Apply remote operations locally
  - Conflict detection during apply

- **Push Operations**
  - Collect local changes since last push
  - Send to remote server
  - Handle server conflicts

- **Persistence**
  - Device ID storage
  - Database ID management
  - Server cursor tracking
  - Last pushed operation ID

- **Conflict Resolution**
  - Pluggable conflict resolution strategies
  - Last-write-wins default policy
  - Manual conflict surfacing
