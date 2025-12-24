# EntiDB Dart Todo Example

A simple todo application demonstrating EntiDB Dart bindings.

## Requirements

- Dart SDK 3.0+
- entidb_dart package (built from source)
- Native library (entidb_ffi) compiled for your platform

## Setup

1. Build the native library:
   ```bash
   cd ../.. && cargo build -p entidb_ffi --release
   ```

2. Run the example:
   ```bash
   dart pub get
   dart run main.dart
   ```

## Features Demonstrated

- Opening an in-memory database
- CRUD operations (Create, Read, Update, Delete)
- Transactions with closure pattern
- Iterator for efficient collection traversal
- Filtering using Dart's `where` method
- **No SQL** - pure Dart data manipulation

## Key Concepts

### Collections

```dart
final todosCollection = db.collection('todos');
```

### Transactions

```dart
db.transaction((tx) {
  tx.put(collection, entityId, bytes);
  return null;
});
```

### Filtering with Dart

```dart
final urgent = allTodos.where((t) => !t.completed && t.priority == 1);
```
