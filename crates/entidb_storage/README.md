# entidb_storage

Storage backend trait and implementations for EntiDB.

## Overview

This crate provides the lowest-level storage abstraction for EntiDB. Storage backends are **opaque byte stores** - they do not interpret the data they store.

## Design Principles

- Backends are simple byte stores (read, append, flush)
- No knowledge of EntiDB file formats, WAL, or segments
- Must be `Send + Sync` for concurrent access
- EntiDB owns all file format interpretation

## Available Backends

| Backend | Use Case |
|---------|----------|
| `InMemoryBackend` | Testing, ephemeral storage |
| `FileBackend` | Persistent storage using OS file APIs |

## Usage

```rust
use entidb_storage::{StorageBackend, InMemoryBackend, FileBackend};
use std::path::Path;

// In-memory for tests
let mut memory = InMemoryBackend::new();
let offset = memory.append(b"hello").unwrap();
let data = memory.read_at(offset, 5).unwrap();

// File for persistence
let mut file = FileBackend::open(Path::new("data.bin")).unwrap();
file.append(b"persistent data").unwrap();
file.sync().unwrap();  // Ensure durability
```

## API

### StorageBackend Trait

```rust
pub trait StorageBackend: Send + Sync {
    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>>;
    fn append(&mut self, data: &[u8]) -> Result<u64>;
    fn flush(&mut self) -> Result<()>;
    fn size(&self) -> Result<u64>;
    fn sync(&mut self) -> Result<()>;
}
```

## License

MIT OR Apache-2.0
