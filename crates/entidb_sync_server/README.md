# entidb_sync_server

Reference HTTP sync server for EntiDB.

## Overview

This crate provides a reference implementation of the EntiDB sync server. It uses
EntiDB core for persistence (no external database) and implements the pull/push
protocol over HTTP with CBOR encoding.

## Features

- **EntiDB-backed**: Uses the same EntiDB core as clients
- **HTTP endpoints**: Pull and push operations via REST
- **Authentication**: HMAC-based request signing
- **Conflict policy**: Server-authoritative conflict resolution

## Endpoints

- `POST /pull` - Retrieve operations since client cursor
- `POST /push` - Submit local operations for server processing

## Server Authority

The sync server is **authoritative** for conflict resolution. When concurrent
modifications are detected, the server's policy determines the winner.

## Usage

```rust
use entidb_sync_server::SyncServer;

let server = SyncServer::new(database, config);
server.run("0.0.0.0:8080").await?;
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
