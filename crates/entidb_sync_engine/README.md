# entidb_sync_engine

Sync state machine and engine for EntiDB.

## Overview

This crate implements the synchronization state machine for EntiDB's offline-first
architecture. It manages the pull-then-push protocol and handles conflict detection.

## Features

- **State machine**: Robust sync lifecycle (idle → connecting → pulling → pushing → synced)
- **Pull-then-push**: Always pull remote changes before pushing local changes
- **Cursor tracking**: Maintains server and local cursors for incremental sync
- **Conflict detection**: Identifies concurrent modifications for resolution

## Sync Flow

```
┌─────────┐     ┌───────────┐     ┌─────────┐     ┌────────┐
│  Idle   │────▶│ Connecting│────▶│ Pulling │────▶│ Pushing│
└─────────┘     └───────────┘     └─────────┘     └────────┘
     ▲                                                  │
     └──────────────────────────────────────────────────┘
                           (synced)
```

## Usage

```rust
use entidb_sync_engine::SyncEngine;

let engine = SyncEngine::new(database, config);
engine.sync().await?;
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
