# EntiDB Python Bindings

Python bindings for EntiDB - an embedded entity database engine.

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

## Usage

```python
from entidb import Database, EntityId

# Open an in-memory database
db = Database.open_memory()

# Get or create a collection
users = db.collection("users")

# Generate a new entity ID
user_id = EntityId()

# Store data
db.put(users, user_id, b'{"name": "Alice"}')

# Retrieve data
data = db.get(users, user_id)
print(data)  # b'{"name": "Alice"}'

# List all entities
for entity_id, data in db.list(users):
    print(f"{entity_id}: {data}")

# Use transactions
txn = db.transaction()
txn.put(users, EntityId(), b"data1")
txn.put(users, EntityId(), b"data2")
db.commit(txn)

# Close the database
db.close()
```

## Context Manager

```python
with Database.open_memory() as db:
    users = db.collection("users")
    db.put(users, EntityId(), b"data")
```

## API Reference

### Database

- `Database.open_memory()` - Opens an in-memory database
- `db.collection(name)` - Gets or creates a collection
- `db.put(collection, entity_id, data)` - Stores an entity
- `db.get(collection, entity_id)` - Retrieves an entity
- `db.delete(collection, entity_id)` - Deletes an entity
- `db.list(collection)` - Lists all entities in a collection
- `db.count(collection)` - Counts entities in a collection
- `db.transaction()` - Creates a new transaction
- `db.commit(txn)` - Commits a transaction
- `db.close()` - Closes the database

### EntityId

- `EntityId()` - Generates a new unique ID
- `EntityId.from_bytes(bytes)` - Creates from 16 bytes
- `entity_id.to_bytes()` - Returns the 16-byte representation
- `entity_id.to_hex()` - Returns hex string representation

### Transaction

- `txn.put(collection, entity_id, data)` - Puts in transaction
- `txn.delete(collection, entity_id)` - Deletes in transaction
- `txn.get(collection, entity_id)` - Gets (sees uncommitted writes)
