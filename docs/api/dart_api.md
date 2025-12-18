# EntiDB Dart API Reference

This document provides comprehensive API documentation for the EntiDB Dart bindings.

## Installation

Add to your `pubspec.yaml`:

```yaml
dependencies:
  entidb_dart:
    path: ../bindings/dart/entidb_dart  # Or from pub.dev when published
```

## Quick Start

```dart
import 'package:entidb_dart/entidb_dart.dart';

void main() {
  // Open an in-memory database
  final db = Database.openMemory();

  // Get or create a collection
  final users = db.collection('users');

  // Generate a unique entity ID
  final userId = EntityId.generate();

  // Store data
  db.put(users, userId, Uint8List.fromList(utf8.encode('{"name": "Alice"}')));

  // Retrieve data
  final data = db.get(users, userId);
  print(utf8.decode(data!)); // {"name": "Alice"}

  // Use transactions for atomic operations
  db.transaction((txn) {
    txn.put(users, EntityId.generate(), Uint8List.fromList([1, 2, 3]));
    txn.put(users, EntityId.generate(), Uint8List.fromList([4, 5, 6]));
  });

  db.close();
}
```

---

## Classes

### Database

The main entry point for interacting with EntiDB.

#### Constructors

##### `Database.open(String path, {...})`

Opens a file-based database at the given path.

**Parameters:**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `path` | `String` | required | Path to the database directory |
| `maxSegmentSize` | `int` | `64 * 1024 * 1024` | Maximum segment file size (64MB) |
| `syncOnCommit` | `bool` | `true` | Whether to sync to disk on every commit |
| `createIfMissing` | `bool` | `true` | Create database if it doesn't exist |

**Example:**
```dart
final db = Database.open(
  '/path/to/database',
  maxSegmentSize: 128 * 1024 * 1024,  // 128MB
  syncOnCommit: true,
);
```

##### `Database.openMemory()`

Opens an in-memory database. Fast but not persistent.

**Example:**
```dart
final db = Database.openMemory();
```

#### Properties

| Property | Type | Description |
|----------|------|-------------|
| `isOpen` | `bool` | Whether the database is currently open |

#### Static Properties

| Property | Type | Description |
|----------|------|-------------|
| `version` | `String` | The EntiDB library version |

#### Methods

##### `Collection collection(String name)`

Gets or creates a collection by name.

**Parameters:**
- `name`: The collection name (must be non-empty)

**Returns:** A `Collection` handle

**Example:**
```dart
final users = db.collection('users');
final products = db.collection('products');
```

##### `void put(Collection collection, EntityId entityId, Uint8List data)`

Stores an entity in a collection. Creates or updates.

**Parameters:**
- `collection`: The target collection
- `entityId`: The entity's unique identifier
- `data`: The entity data as CBOR bytes

**Example:**
```dart
db.put(users, userId, Uint8List.fromList([1, 2, 3]));
```

##### `Uint8List? get(Collection collection, EntityId entityId)`

Retrieves an entity from a collection.

**Parameters:**
- `collection`: The collection to query
- `entityId`: The entity's unique identifier

**Returns:** The entity data, or `null` if not found

**Example:**
```dart
final data = db.get(users, userId);
if (data != null) {
  print('Found: ${data.length} bytes');
}
```

##### `void delete(Collection collection, EntityId entityId)`

Deletes an entity from a collection.

**Parameters:**
- `collection`: The collection containing the entity
- `entityId`: The entity to delete

**Example:**
```dart
db.delete(users, userId);
```

##### `List<(EntityId, Uint8List)> list(Collection collection)`

Lists all entities in a collection.

**Parameters:**
- `collection`: The collection to list

**Returns:** List of (EntityId, data) tuples

**Example:**
```dart
final entities = db.list(users);
for (final (id, data) in entities) {
  print('Entity: $id, ${data.length} bytes');
}
```

##### `int count(Collection collection)`

Returns the number of entities in a collection.

**Parameters:**
- `collection`: The collection to count

**Returns:** The entity count

**Example:**
```dart
print('Users: ${db.count(users)}');
```

##### `T transaction<T>(T Function(Transaction txn) fn)`

Executes a function within a transaction.

All operations in the callback are atomic - they all succeed or all fail.

**Parameters:**
- `fn`: The transaction callback

**Returns:** The callback's return value

**Example:**
```dart
final result = db.transaction((txn) {
  txn.put(users, id1, data1);
  txn.put(users, id2, data2);
  return 'success';
});
```

##### `EntityIterator iter(Collection collection)`

Creates an iterator over a collection.

**Parameters:**
- `collection`: The collection to iterate

**Returns:** An `EntityIterator`

**Example:**
```dart
final iter = db.iter(users);
while (iter.moveNext()) {
  print('Entity: ${iter.currentId}');
}
iter.dispose();
```

##### `void checkpoint()`

Creates a checkpoint, flushing WAL to segments.

##### `void close()`

Closes the database and releases resources.

---

### EntityId

A 16-byte unique entity identifier.

#### Constructors

##### `EntityId.generate()`

Generates a new unique entity ID using UUID v4.

**Example:**
```dart
final id = EntityId.generate();
```

##### `EntityId.fromBytes(Uint8List bytes)`

Creates an entity ID from raw bytes.

**Parameters:**
- `bytes`: Exactly 16 bytes

**Throws:** `ArgumentError` if not exactly 16 bytes

**Example:**
```dart
final id = EntityId.fromBytes(Uint8List.fromList(List.filled(16, 0)));
```

##### `EntityId.zero()`

Creates a zero (null) entity ID.

#### Properties

| Property | Type | Description |
|----------|------|-------------|
| `bytes` | `Uint8List` | The raw 16-byte identifier |

#### Methods

##### `String toString()`

Returns a hex string representation.

**Example:**
```dart
print(id); // EntityId(550e8400e29b41d4a716446655440000)
```

#### Operators

- `==`: Compares two entity IDs for equality
- `hashCode`: Returns a hash code for use in maps/sets

---

### Collection

Represents a named collection of entities.

#### Properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | `String` | The collection name |
| `id` | `int` | The internal collection ID |

---

### Transaction

A database transaction for atomic operations.

#### Methods

##### `void put(Collection collection, EntityId entityId, Uint8List data)`

Stores an entity within this transaction.

##### `Uint8List? get(Collection collection, EntityId entityId)`

Gets an entity, seeing uncommitted writes in this transaction.

##### `void delete(Collection collection, EntityId entityId)`

Deletes an entity within this transaction.

---

### EntityIterator

An iterator over entities in a collection.

#### Methods

##### `bool moveNext()`

Advances to the next entity.

**Returns:** `true` if there is a next entity

##### `EntityId get currentId`

The current entity's ID.

##### `Uint8List get currentData`

The current entity's data.

##### `void dispose()`

Releases iterator resources. **Must be called when done.**

---

### EntiDbError

Base exception class for EntiDB errors.

#### Subclasses

| Class | Description |
|-------|-------------|
| `EntiDbNotFoundError` | Entity not found |
| `EntiDbInvalidError` | Invalid argument or state |
| `EntiDbIoError` | I/O operation failed |
| `EntiDbCorruptionError` | Data corruption detected |
| `EntiDbTransactionError` | Transaction error |

---

## Error Handling

```dart
try {
  final db = Database.open('/invalid/path', createIfMissing: false);
} on EntiDbIoError catch (e) {
  print('Failed to open: $e');
}

try {
  db.transaction((txn) {
    txn.put(users, id, data);
    throw Exception('Abort!');
  });
} catch (e) {
  // Transaction was automatically rolled back
}
```

---

## Best Practices

### 1. Always Close the Database

```dart
final db = Database.openMemory();
try {
  // ... use database
} finally {
  db.close();
}
```

### 2. Use Transactions for Multiple Operations

```dart
// Good: atomic
db.transaction((txn) {
  txn.put(accounts, from, newFromBalance);
  txn.put(accounts, to, newToBalance);
});

// Bad: not atomic
db.put(accounts, from, newFromBalance);
db.put(accounts, to, newToBalance);  // May fail leaving inconsistent state
```

### 3. Dispose Iterators

```dart
final iter = db.iter(users);
try {
  while (iter.moveNext()) {
    // process
  }
} finally {
  iter.dispose();
}
```

### 4. Use CBOR for Structured Data

```dart
import 'package:cbor/cbor.dart';

final user = {'name': 'Alice', 'age': 30};
final data = cbor.encode(CborMap(user));
db.put(users, id, Uint8List.fromList(data));
```

---

## Thread Safety

- `Database` instances are thread-safe for concurrent reads
- Write operations are serialized (single-writer)
- Each `Transaction` must be used from a single thread
- `EntityIterator` instances must be used from a single thread

---

## Platform Support

| Platform | Status |
|----------|--------|
| Windows | ✅ Supported |
| macOS | ✅ Supported |
| Linux | ✅ Supported |
| iOS | ✅ Supported |
| Android | ✅ Supported |
| Web | ✅ Supported (via WASM) |

---

## See Also

- [Architecture Documentation](../architecture.md)
- [Bindings Contract](../bindings_contract.md)
- [Test Vectors](../test_vectors/README.md)
