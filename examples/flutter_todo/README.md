# Flutter Todo Example

A simple todo application demonstrating EntiDB usage in Flutter.

## Features

- ✅ Persistent storage using EntiDB
- ✅ Create, complete, and delete todos
- ✅ Host-language filtering (Dart `where`, not SQL!)
- ✅ Atomic transactions for bulk operations
- ✅ Material Design 3 UI

## Running the Example

```bash
cd examples/flutter_todo
flutter pub get
flutter run
```

## Key Concepts Demonstrated

### Opening a Database

```dart
final dir = await getApplicationDocumentsDirectory();
final dbPath = '${dir.path}/entidb_todo';
final db = Database.open(dbPath);
```

### Working with Collections

```dart
final todosCollection = db.collection('todos');
```

### CRUD Operations

```dart
// Create
db.put(todosCollection, todo.id, todo.toBytes());

// Read
for (final entry in db.iter(todosCollection)) {
  final todo = Todo.fromBytes(entry.id, entry.data);
}

// Update
db.put(todosCollection, updatedTodo.id, updatedTodo.toBytes());

// Delete
db.delete(todosCollection, todo.id);
```

### Host-Language Filtering (No SQL!)

```dart
// Filter using Dart's where clause
final completed = _todos.where((t) => t.completed);

// Sort using Dart's sort
todos.sort((a, b) => b.priority.compareTo(a.priority));
```

### Transactions

```dart
db.transaction((txn) {
  // All operations are atomic
  for (final todo in todosToDelete) {
    txn.delete(todosCollection, todo.id);
  }
});
```

## Platform Support

| Platform | Status |
|----------|--------|
| Android  | ✅ |
| iOS      | ✅ |
| macOS    | ✅ |
| Windows  | ✅ |
| Linux    | ✅ |

## Dependencies

- `entidb_flutter: ^2.0.0-alpha.2` - EntiDB Flutter plugin with bundled native libraries
- `path_provider` - For getting the application documents directory
