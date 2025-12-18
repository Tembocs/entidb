# EntiDB

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

**EntiDB** is a custom embedded entity database engine written in Rust with Dart and Python bindings. It provides ACID transactions, WAL-based durability, and optional offline-first synchronization—all without SQL or external database dependencies.

## Key Features

- **Entity-First Design** — Store domain objects directly, not tables
- **No SQL, No DSL** — Query using native language constructs (Rust iterators, Dart `where`, Python comprehensions)
- **Custom Storage Engine** — Zero external database dependencies (no SQLite, RocksDB, LMDB)
- **ACID Transactions** — Full atomicity, consistency, isolation, and durability
- **WAL-Based Recovery** — Crash-safe with write-ahead logging
- **CBOR-Native** — Canonical CBOR encoding for storage and sync
- **Cross-Platform** — Native (Windows, Linux, macOS), Web (WASM + OPFS), Mobile (iOS, Android)
- **Multi-Language Bindings** — Rust, Dart/Flutter, Python with identical semantics

## Quick Start

### Rust

```rust
use entidb_core::{Database, Config, EntityId, CollectionId};
use entidb_storage::InMemoryBackend;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open database with in-memory backend
    let backend = InMemoryBackend::new();
    let config = Config::default();
    let db = Database::open(backend, config)?;

    // Define a collection
    let users = CollectionId::new("users");

    // Write in a transaction
    db.transaction(|tx| {
        let id = EntityId::new();
        let data = br#"{"name": "Alice", "email": "alice@example.com"}"#;
        tx.put(&users, id, data)?;
        Ok(())
    })?;

    // Read entities
    let all_users = db.list(&users)?;
    println!("Users: {:?}", all_users.len());

    Ok(())
}
```

### Dart/Flutter

```dart
import 'package:entidb_dart/entidb_dart.dart';

void main() async {
  final db = await Database.open('path/to/db');
  final users = db.collection<User>('users');

  // Write
  await db.transaction((tx) async {
    await users.put(User(name: 'Alice', email: 'alice@example.com'));
  });

  // Read with native filtering
  final admins = users.list().where((u) => u.isAdmin).toList();
}
```

### Python

```python
from entidb import Database, Collection

with Database.open("path/to/db") as db:
    users = db.collection("users")

    # Write
    with db.transaction() as tx:
        users.put({"name": "Alice", "email": "alice@example.com"})

    # Read with comprehensions
    admins = [u for u in users.list() if u.get("is_admin")]
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Application (Rust / Dart / Python)                              │
├─────────────────────────────────────────────────────────────────┤
│ EntiDB Core                                                     │
│  ├─ Entities & Collections                                      │
│  ├─ Transactions + WAL                                          │
│  ├─ Indexes (Hash, BTree)                                       │
│  └─ Change Feed                                                 │
├─────────────────────────────────────────────────────────────────┤
│ Storage Backend                                                 │
│  ├─ FileBackend (native)                                        │
│  ├─ OpfsBackend (web)                                           │
│  └─ InMemoryBackend (testing)                                   │
└─────────────────────────────────────────────────────────────────┘
```

### Crate Structure

| Crate | Description |
|-------|-------------|
| `entidb_core` | Core engine: storage, transactions, WAL, indexes |
| `entidb_codec` | Canonical CBOR encoding/decoding |
| `entidb_storage` | Storage backend trait and implementations |
| `entidb_ffi` | Stable C ABI for language bindings |
| `entidb_sync_protocol` | Sync protocol types (no I/O) |
| `entidb_sync_engine` | Sync state machine |
| `entidb_sync_server` | Reference HTTP sync server |
| `entidb_cli` | Command-line tools |
| `entidb_testkit` | Test utilities and fixtures |
| `entidb_bench` | Performance benchmarks |

## CLI

EntiDB includes a command-line tool for database management:

```bash
# Install
cargo install --path crates/entidb_cli

# Inspect database
entidb inspect ./my-database

# Verify integrity
entidb verify ./my-database

# Compact segments
entidb compact ./my-database --dry-run

# Dump WAL records
entidb dump-oplog ./my-database --limit 100 --json
```

## Documentation

- [Quick Start Guide](docs/quickstart.md) — Get up and running
- [API Reference](docs/api_reference.md) — Complete API documentation
- [CLI Guide](docs/cli_guide.md) — Command-line tool usage
- [Architecture](docs/architecture.md) — System design and internals
- [File Format](docs/file_format.md) — Binary format specification
- [Transactions](docs/transactions.md) — Transaction semantics
- [Invariants](docs/invariants.md) — System invariants and guarantees

## Building from Source

### Prerequisites

- Rust 1.75 or later
- For Dart bindings: Dart SDK 3.0+
- For Python bindings: Python 3.8+, maturin

### Build

```bash
# Clone repository
git clone https://github.com/Tembocs/entidb.git
cd entidb

# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run benchmarks
cargo bench -p entidb_bench
```

### Build Bindings

```bash
# Dart (requires building native library first)
cd bindings/dart/entidb_dart
dart pub get

# Python
cd bindings/python/entidb_py
maturin develop
```

## Design Principles

### No Query Languages

EntiDB deliberately avoids SQL, query builders, and DSLs. Filtering is performed using host-language constructs:

```rust
// Rust: Use iterators
let active = db.list(&users)?
    .into_iter()
    .filter(|u| u.is_active)
    .collect::<Vec<_>>();
```

```dart
// Dart: Use where
final active = users.list().where((u) => u.isActive).toList();
```

```python
# Python: Use comprehensions
active = [u for u in users.list() if u.is_active]
```

### Custom Storage Engine

EntiDB implements its own storage from scratch:

- **WAL (Write-Ahead Log)** — Append-only, crash-safe
- **Segments** — Immutable after sealing
- **Indexes** — Hash and BTree, rebuilt from segments

No RocksDB, SQLite, LMDB, or other embedded databases.

### Canonical CBOR

All entity payloads use deterministic CBOR encoding:

- Maps sorted by key (bytewise)
- Integers use shortest encoding
- No floats unless explicit
- No indefinite-length items

## Synchronization

EntiDB supports optional offline-first sync:

```rust
use entidb_sync_engine::SyncEngine;

let sync = SyncEngine::new(db, server_url);
sync.pull().await?;  // Pull remote changes
sync.push().await?;  // Push local changes
```

- **Pull-then-push** synchronization
- **Server authoritative** conflict resolution
- **Same EntiDB core** on server (not an external DB)

## Performance

Run benchmarks:

```bash
cargo bench -p entidb_bench

# Specific benchmarks
cargo bench -p entidb_bench --bench database
cargo bench -p entidb_bench --bench codec
cargo bench -p entidb_bench --bench storage
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please read [AGENTS.md](AGENTS.md) for architectural constraints and coding guidelines before submitting changes.
