/// EntiDB Dart Example - Todo Application
///
/// This example demonstrates:
/// - Opening a database
/// - Basic CRUD operations
/// - Filtering with Dart `where` clause
/// - Transaction usage

import 'dart:typed_data';
import 'package:entidb_dart/entidb_dart.dart';

/// A simple todo item entity
class Todo {
  final EntityId id;
  final String title;
  final bool completed;
  final int priority;
  final int createdAt;

  Todo({
    required this.id,
    required this.title,
    this.completed = false,
    this.priority = 0,
    this.createdAt = 0,
  });

  /// Create from database bytes (simple encoding for demo)
  factory Todo.fromBytes(EntityId id, Uint8List bytes) {
    // Simple decoding: title|completed|priority|createdAt
    final str = String.fromCharCodes(bytes);
    final parts = str.split('|');
    return Todo(
      id: id,
      title: parts.isNotEmpty ? parts[0] : '',
      completed: parts.length > 1 && parts[1] == 'true',
      priority: parts.length > 2 ? int.tryParse(parts[2]) ?? 0 : 0,
      createdAt: parts.length > 3 ? int.tryParse(parts[3]) ?? 0 : 0,
    );
  }

  /// Convert to database bytes (simple encoding for demo)
  Uint8List toBytes() {
    final str = '$title|$completed|$priority|$createdAt';
    return Uint8List.fromList(str.codeUnits);
  }

  /// Create a copy with modifications
  Todo copyWith({
    EntityId? id,
    String? title,
    bool? completed,
    int? priority,
    int? createdAt,
  }) {
    return Todo(
      id: id ?? this.id,
      title: title ?? this.title,
      completed: completed ?? this.completed,
      priority: priority ?? this.priority,
      createdAt: createdAt ?? this.createdAt,
    );
  }
}

void main() {
  print('📁 Creating in-memory database');

  // Open an in-memory database
  final db = Database.openMemory();
  print('✅ Database opened successfully');

  // Get the todos collection
  final todosCollection = db.collection('todos');

  // Create some todos
  final todos = [
    Todo(
      id: EntityId.generate(),
      title: 'Learn EntiDB',
      completed: false,
      priority: 1,
      createdAt: 1700000000,
    ),
    Todo(
      id: EntityId.generate(),
      title: 'Build an app',
      completed: false,
      priority: 2,
      createdAt: 1700000100,
    ),
    Todo(
      id: EntityId.generate(),
      title: 'Write tests',
      completed: true,
      priority: 1,
      createdAt: 1700000200,
    ),
    Todo(
      id: EntityId.generate(),
      title: 'Deploy to production',
      completed: false,
      priority: 3,
      createdAt: 1700000300,
    ),
  ];

  // Insert todos in a transaction
  print('\n📝 Inserting ${todos.length} todos...');
  db.transaction((tx) {
    for (final todo in todos) {
      tx.put(todosCollection, todo.id, todo.toBytes());
    }
    return null;
  });
  print('✅ Todos inserted');

  // Read all todos using list()
  print('\n📋 All todos:');
  final allTodos = db
      .list(todosCollection)
      .map((record) => Todo.fromBytes(record.$1, record.$2))
      .toList();

  for (final todo in allTodos) {
    final status = todo.completed ? '✓' : '○';
    print('  $status [P${todo.priority}] ${todo.title}');
  }

  // Filter incomplete high-priority todos using Dart `where`
  print('\n⚡ High-priority incomplete todos:');
  final urgent = allTodos.where((t) => !t.completed && t.priority == 1);

  for (final todo in urgent) {
    print('  ○ ${todo.title}');
  }

  // Update a todo
  print("\n✏️  Completing 'Learn EntiDB'...");
  db.transaction((tx) {
    final todo = allTodos.firstWhere(
      (t) => t.title == 'Learn EntiDB',
      orElse: () => throw Exception('Todo not found'),
    );
    tx.put(todosCollection, todo.id, todo.copyWith(completed: true).toBytes());
    return null;
  });

  // Count completed vs incomplete
  final updatedTodos = db
      .list(todosCollection)
      .map((record) => Todo.fromBytes(record.$1, record.$2))
      .toList();
  final completed = updatedTodos.where((t) => t.completed).length;
  final incomplete = updatedTodos.where((t) => !t.completed).length;

  print('\n📊 Summary:');
  print('  Completed: $completed');
  print('  Incomplete: $incomplete');

  // Delete completed todos
  print('\n🗑️  Deleting completed todos...');
  db.transaction((tx) {
    final toDelete =
        updatedTodos.where((t) => t.completed).map((t) => t.id).toList();
    for (final id in toDelete) {
      tx.delete(todosCollection, id);
    }
    return null;
  });

  final remaining = db.count(todosCollection);
  print('✅ Remaining todos: $remaining');

  // Close the database
  db.close();
  print('\n👋 Database closed');
}
