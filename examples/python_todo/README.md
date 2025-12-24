# EntiDB Python Todo Example

A simple todo application demonstrating EntiDB Python bindings.

## Requirements

- Python 3.9+
- entidb package (built from source)

## Setup

1. Build the Python bindings:
   ```bash
   cd ../../bindings/python/entidb_py
   pip install maturin
   maturin develop
   ```

2. Run the example:
   ```bash
   python main.py
   ```

## Features Demonstrated

- Opening an in-memory database with context manager
- CRUD operations (Create, Read, Update, Delete)
- Transactions with context managers (auto-commit/abort)
- Iterator for efficient collection traversal
- Filtering using Python list comprehensions
- **No SQL** - pure Python data manipulation

## Key Concepts

### Context Managers

```python
with entidb.Database.open_memory() as db:
    # database auto-closes on exit
```

### Transactions

```python
with db.transaction() as txn:
    txn.put(collection, entity_id, data)
    # auto-commits on success, auto-aborts on exception
```

### Filtering with Python

```python
urgent = [t for t in all_todos if not t.completed and t.priority == 1]
```
