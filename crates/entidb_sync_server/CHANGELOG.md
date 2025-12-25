# Changelog

All notable changes to the `entidb_sync_server` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-XX-XX

### Added

- **HTTP Server**
  - Reference sync server implementation
  - HTTPS support with TLS
  - CBOR request/response encoding

- **Endpoints**
  - `POST /pull` - Fetch changes since cursor
  - `POST /push` - Submit local changes
  - `GET /health` - Health check

- **Authentication**
  - Pluggable authentication middleware
  - Device/database authorization

- **Conflict Handling**
  - Server-side conflict detection
  - Configurable conflict policies
  - Conflict reporting to clients

- **Storage**
  - Uses EntiDB core for server-side persistence
  - Change feed for tracking modifications
  - Cursor management per device
