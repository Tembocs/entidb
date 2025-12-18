# EntiDB Python API Reference

This document provides comprehensive API documentation for the EntiDB Python bindings.

## Installation

```bash
pip install entidb
```

Or build from source:

```bash
cd bindings/python/entidb_py
pip install maturin
maturin develop
```

## Quick Start

```python
from entidb import Database, EntityId

# Open an in-memory database
with Database.open_memory() as db:
    # Get or create a collection
    users = db.collection("users")

    # Generate a unique entity ID
    user_id = EntityId()

    # Store data
    db.put(users, user_id, b'{"name": "Alice"}')

    # Retrieve data
    data = db.get(users, user_id)
    print(data)  # b'{"name": "Alice"}'

    # List all entities
    for entity_id, entity_data in db.list(users):
        print(f"{entity_id}: {entity_data}")
```

---

## Classes

### Database

The main entry point for interacting with EntiDB.

#### Class Methods

##### `Database.open(path, **kwargs) -> Database`

Opens a file-based database at the given path.

**Parameters:**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `path` | `str` | required | Path to the database directory |
| `max_segment_size` | `int` | `67108864` | Maximum segment file size (64MB) |
| `sync_on_commit` | `bool` | `True` | Whether to sync to disk on every commit |
| `create_if_missing` | `bool` | `True` | Create database if it doesn't exist |

**Returns:** `Database` instance

**Raises:** `IOError` if the database cannot be opened

**Example:**
```python
db = Database.open(
    "/path/to/database",
    max_segment_size=128 * 1024 * 1024,  # 128MB
    sync_on_commit=True,
)
```

##### `Database.open_memory() -> Database`

Opens an in-memory database. Fast but not persistent.

**Returns:** `Database` instance

**Example:**
```python
db = Database.open_memory()
```

#### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_open` | `bool` | Whether the database is currently open |

#### Methods

##### `collection(name: str) -> Collection`

Gets or creates a collection by name.

**Parameters:**
- `name`: The collection name (must be non-empty)

**Returns:** A `Collection` handle

**Example:**
```python
users = db.collection("users")
products = db.collection("products")
```

##### `put(collection: Collection, entity_id: EntityId, data: bytes) -> None`

Stores an entity in a collection. Creates or updates.

**Parameters:**
- `collection`: The target collection
- `entity_id`: The entity's unique identifier
- `data`: The entity data as bytes

**Raises:** `IOError` on failure

**Example:**
```python
db.put(users, user_id, b"hello world")
```

##### `get(collection: Collection, entity_id: EntityId) -> Optional[bytes]`

Retrieves an entity from a collection.

**Parameters:**
- `collection`: The collection to query
- `entity_id`: The entity's unique identifier

**Returns:** The entity data as `bytes`, or `None` if not found

**Example:**
```python
data = db.get(users, user_id)
if data is not None:
    print(f"Found: {len(data)} bytes")
```

##### `delete(collection: Collection, entity_id: EntityId) -> None`

Deletes an entity from a collection.

**Parameters:**
- `collection`: The collection containing the entity
- `entity_id`: The entity to delete

**Example:**
```python
db.delete(users, user_id)
```

##### `list(collection: Collection) -> List[Tuple[EntityId, bytes]]`

Lists all entities in a collection.

**Parameters:**
- `collection`: The collection to list

**Returns:** List of `(EntityId, bytes)` tuples

**Example:**
```python
for entity_id, data in db.list(users):
    print(f"Entity: {entity_id}, {len(data)} bytes")
```

##### `count(collection: Collection) -> int`

Returns the number of entities in a collection.

**Parameters:**
- `collection`: The collection to count

**Returns:** The entity count

**Example:**
```python
print(f"Users: {db.count(users)}")
```

##### `transaction() -> Transaction`

Creates a new transaction for batched operations.

**Returns:** A `Transaction` object

**Example:**
```python
txn = db.transaction()
txn.put(users, id1, data1)
txn.put(users, id2, data2)
db.commit(txn)
```

##### `commit(txn: Transaction) -> None`

Commits a transaction atomically.

**Parameters:**
- `txn`: The transaction to commit

**Raises:** `RuntimeError` if transaction already committed

**Example:**
```python
txn = db.transaction()
txn.put(users, user_id, data)
db.commit(txn)
```

##### `close() -> None`

Closes the database and releases resources.

#### Context Manager

`Database` supports the context manager protocol:

```python
with Database.open_memory() as db:
    # ... use database
# Database is automatically closed
```

---

### EntityId

A 16-byte unique entity identifier.

#### Constructor

##### `EntityId()`

Creates a new unique entity ID using UUID v4.

**Example:**
```python
entity_id = EntityId()
```

#### Class Methods

##### `EntityId.from_bytes(data: bytes) -> EntityId`

Creates an entity ID from raw bytes.

**Parameters:**
- `data`: Exactly 16 bytes

**Raises:** `ValueError` if not exactly 16 bytes

**Example:**
```python
entity_id = EntityId.from_bytes(b"\x00" * 16)
```

#### Methods

##### `to_bytes() -> bytes`

Returns the raw 16-byte identifier.

**Returns:** 16 bytes

**Example:**
```python
raw = entity_id.to_bytes()
assert len(raw) == 16
```

##### `to_hex() -> str`

Returns a hexadecimal string representation.

**Returns:** 32-character hex string

**Example:**
```python
hex_str = entity_id.to_hex()
print(hex_str)  # "550e8400e29b41d4a716446655440000"
```

#### Special Methods

- `__repr__()`: Returns `EntityId(hex_string)`
- `__eq__(other)`: Compares two entity IDs
- `__hash__()`: Returns hash for use in dicts/sets

**Example:**
```python
id1 = EntityId()
id2 = EntityId.from_bytes(id1.to_bytes())
assert id1 == id2
assert hash(id1) == hash(id2)

# Use in dict
entities = {id1: "data"}
```

---

### Collection

Represents a named collection of entities.

#### Properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | `str` | The collection name |
| `id` | `int` | The internal collection ID |

#### Special Methods

- `__repr__()`: Returns `Collection(name, id=N)`

---

### Transaction

A database transaction for batched operations.

#### Methods

##### `put(collection: Collection, entity_id: EntityId, data: bytes) -> None`

Stores an entity within this transaction.

##### `delete(collection: Collection, entity_id: EntityId) -> None`

Deletes an entity within this transaction.

##### `get(collection: Collection, entity_id: EntityId) -> Optional[bytes]`

Gets an entity, seeing uncommitted writes in this transaction.

---

## Error Handling

```python
from entidb import Database, EntityId

# Handle open errors
try:
    db = Database.open("/invalid/path", create_if_missing=False)
except IOError as e:
    print(f"Failed to open: {e}")

# Transactions automatically rollback on exception
try:
    txn = db.transaction()
    txn.put(users, id1, data1)
    raise ValueError("Abort!")
    db.commit(txn)  # Never reached
except ValueError:
    pass  # Transaction was not committed
```

---

## Best Practices

### 1. Use Context Managers

```python
# Good: database is always closed
with Database.open_memory() as db:
    # ... use database

# Alternative: manual close
db = Database.open_memory()
try:
    # ... use database
finally:
    db.close()
```

### 2. Batch Operations in Transactions

```python
# Good: atomic and efficient
txn = db.transaction()
for i in range(1000):
    txn.put(items, EntityId(), data)
db.commit(txn)

# Bad: 1000 separate transactions
for i in range(1000):
    db.put(items, EntityId(), data)
```

### 3. Use CBOR for Structured Data

```python
import cbor2

user = {"name": "Alice", "age": 30}
data = cbor2.dumps(user)
db.put(users, user_id, data)

# Retrieve and decode
retrieved = db.get(users, user_id)
user = cbor2.loads(retrieved)
```

### 4. Handle None Returns

```python
data = db.get(users, user_id)
if data is None:
    print("User not found")
else:
    user = cbor2.loads(data)
```

---

## Type Hints

Full type hints are available:

```python
from typing import Optional, List, Tuple
from entidb import Database, EntityId, Collection, Transaction

def get_user(db: Database, users: Collection, user_id: EntityId) -> Optional[bytes]:
    return db.get(users, user_id)

def list_users(db: Database, users: Collection) -> List[Tuple[EntityId, bytes]]:
    return db.list(users)
```

---

## Thread Safety

- `Database` instances are thread-safe for concurrent reads
- Write operations are serialized (single-writer)
- Each `Transaction` must be used from a single thread
- Use locks if sharing transactions across threads (not recommended)

---

## Platform Support

| Platform | Status |
|----------|--------|
| Windows (x64) | ✅ Supported |
| macOS (x64) | ✅ Supported |
| macOS (ARM64) | ✅ Supported |
| Linux (x64) | ✅ Supported |
| Linux (ARM64) | ✅ Supported |

---

## Examples

### JSON Storage

```python
import json
from entidb import Database, EntityId

with Database.open_memory() as db:
    users = db.collection("users")
    
    # Store JSON
    user = {"name": "Alice", "email": "alice@example.com"}
    user_id = EntityId()
    db.put(users, user_id, json.dumps(user).encode())
    
    # Retrieve JSON
    data = db.get(users, user_id)
    user = json.loads(data.decode())
    print(user["name"])  # Alice
```

### File-Based Persistence

```python
from entidb import Database, EntityId
import os

db_path = os.path.expanduser("~/.myapp/database")

with Database.open(db_path) as db:
    settings = db.collection("settings")
    
    # Store setting
    db.put(settings, EntityId.from_bytes(b"theme\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"), b"dark")
    
# Data persists after close

# Reopen and retrieve
with Database.open(db_path) as db:
    settings = db.collection("settings")
    theme = db.get(settings, EntityId.from_bytes(b"theme\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"))
    print(theme)  # b"dark"
```

### Batch Processing

```python
from entidb import Database, EntityId

with Database.open_memory() as db:
    items = db.collection("items")
    
    # Batch insert
    txn = db.transaction()
    for i in range(10000):
        txn.put(items, EntityId(), f"item-{i}".encode())
    db.commit(txn)
    
    print(f"Inserted {db.count(items)} items")
```

---

## See Also

- [Architecture Documentation](../architecture.md)
- [Bindings Contract](../bindings_contract.md)
- [Test Vectors](../test_vectors/README.md)
