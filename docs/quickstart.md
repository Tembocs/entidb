# EntiDB Quick Start Guide

Get started with EntiDB in under 5 minutes.

## Installation

### Rust

Add to your `Cargo.toml`:

```toml
[dependencies]
entidb_core = "0.1"
entidb_storage = "0.1"
entidb_codec = "0.1"
```

### Dart/Flutter

```yaml
dependencies:
  entidb_dart: ^2.0.0-alpha.1
```

### Python

```bash
pip install entidb
```

---

## Your First Database

### Rust

```rust
use entidb_core::Database;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open an in-memory database
    let db = Database::open_in_memory()?;
    
    // Create a collection
    let users = db.collection("users");
    
    // Insert data
    use entidb_core::EntityId;
    let user_id = EntityId::new();
    let user_data = br#"{"name": "Alice", "email": "alice@example.com"}"#.to_vec();
    
    db.transaction(|txn| {
        txn.put(users, user_id, user_data)?;
        Ok(())
    })?;
    
    // Read data
    let user = db.get(users, user_id)?;
    println!("User: {:?}", user);
    
    Ok(())
}
```

### Dart

```dart
import 'package:entidb_dart/entidb_dart.dart';

void main() async {
  // Open database
  final db = await Database.open('path/to/db');
  
  // Create a collection
  final users = db.collection('users');
  
  // Insert data
  final userId = EntityId.generate();
  await db.transaction((txn) async {
    await txn.put(users, userId, {'name': 'Alice', 'email': 'alice@example.com'});
  });
  
  // Read data
  final user = await db.get(users, userId);
  print('User: $user');
  
  // Close database
  await db.close();
}
```

### Python

```python
from entidb import Database, EntityId

# Open database
db = Database.open_in_memory()

# Create a collection
users = db.collection("users")

# Insert data
user_id = EntityId.new()
user_data = {"name": "Alice", "email": "alice@example.com"}

with db.transaction() as txn:
    txn.put(users, user_id, user_data)

# Read data
user = db.get(users, user_id)
print(f"User: {user}")

# Close database
db.close()
```

---

## Key Concepts

### 1. Entities

Entities are your data objects. Each has a unique ID:

```rust
let id = EntityId::new();  // Random UUID
```

### 2. Collections

Group related entities:

```rust
let users = db.collection("users");
let orders = db.collection("orders");
```

### 3. Transactions

All writes must happen in transactions:

```rust
db.transaction(|txn| {
    txn.put(collection, id, data)?;
    txn.delete(collection, other_id)?;
    Ok(())  // Commit
})?;
```

### 4. CRUD Operations

```rust
// Create / Update
db.transaction(|txn| {
    txn.put(collection, id, data)?;
    Ok(())
})?;

// Read
let entity = db.get(collection, id)?;

// Delete
db.transaction(|txn| {
    txn.delete(collection, id)?;
    Ok(())
})?;

// List all
let all = db.list(collection)?;
```

---

## Persistent Storage

For production use, use file-based storage:

```rust
use entidb_core::{Database, Config};
use entidb_storage::FileBackend;
use std::path::Path;

fn open_persistent_db(path: &Path) -> Result<Database, Box<dyn std::error::Error>> {
    let wal_path = path.join("wal.log");
    let segment_path = path.join("segments.dat");
    
    let wal = FileBackend::open_with_create_dirs(&wal_path)?;
    let segments = FileBackend::open_with_create_dirs(&segment_path)?;
    
    let db = Database::open_with_backends(
        Config::default(),
        Box::new(wal),
        Box::new(segments),
    )?;
    
    Ok(db)
}
```

---

## What's Next?

- [API Reference](api_reference.md) - Complete API documentation
- [Architecture Guide](architecture.md) - How EntiDB works internally
- [Transaction Semantics](transactions.md) - ACID guarantees explained
- [Sync Guide](sync_guide.md) - Set up offline-first sync

---

## Common Patterns

### Typed Entities with Serde

```rust
use serde::{Serialize, Deserialize};
use entidb_codec::{to_canonical_cbor, from_cbor};

#[derive(Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
}

// Encode
let user = User { name: "Alice".into(), email: "alice@example.com".into() };
let bytes = to_canonical_cbor(&user)?;

// Store
db.transaction(|txn| {
    txn.put(users, user_id, bytes)?;
    Ok(())
})?;

// Retrieve and decode
let bytes = db.get(users, user_id)?.unwrap();
let user: User = from_cbor(&bytes)?;
```

### Batch Operations

```rust
db.transaction(|txn| {
    for (id, data) in items {
        txn.put(collection, id, data)?;
    }
    Ok(())
})?;
```

### Error Handling

```rust
match db.transaction(|txn| {
    txn.put(collection, id, data)?;
    Ok(())
}) {
    Ok(()) => println!("Success!"),
    Err(e) => eprintln!("Error: {}", e),
}
```
