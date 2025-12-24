/// EntiDB Dart Example - Todo Application
///
/// This example demonstrates:
/// - Opening a database
/// - Basic CRUD operations
/// - Filtering with Dart's `where` clause (no SQL!)
/// - Transaction usage with context manager pattern
///
/// Run with: dart run main.dart

import 'dart:typed_data';
import 'dart:convert';
import 'package:entidb_dart/entidb_dart.dart';

/// A simple todo item entity.
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

  /// Create a new todo with a generated ID.
  factory Todo.create(String title, {int priority = 0}) {
    return Todo(
      id: EntityId.generate(),
      title: title,
      priority: priority,
      createdAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
    );
  }

  /// Create from database bytes (JSON encoding for simplicity).
  factory Todo.fromBytes(EntityId id, Uint8List bytes) {
    final json = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
    return Todo(
      id: id,
      title: json['title'] as String,
      completed: json['completed'] as bool? ?? false,
      priority: json['priority'] as int? ?? 0,
      createdAt: json['created_at'] as int? ?? 0,
    );
  }

  /// Convert to database bytes (JSON encoding for simplicity).
  Uint8List toBytes() {
    final json = {
      'title': title,
      'completed': completed,
      'priority': priority,
      'created_at': createdAt,
    };
    return Uint8List.fromList(utf8.encode(jsonEncode(json)));
  }

  /// Create a copy with modifications.
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

  @override
  String toString() {
    final status = completed ? '✓' : '○';
    return '$status [P$priority] $title';
  }
}

void main() {
  print('📁 Creating in-memory database');

  // Open an in-memory database
  final db = Database.openMemory();
  print('✅ Database opened successfully');
  print('   Version: ${Database.version}');

  // Get the todos collection
  final todosCollection = db.collection('todos');
  print('   Collection: ${todosCollection.name} (id=${todosCollection.id})');

  // Create some todos
  final todos = [
    Todo.create('Learn EntiDB', priority: 1),
    Todo.create('Build an app', priority: 2),
    Todo(
      id: EntityId.generate(),
      title: 'Write tests',
      completed: true,
      priority: 1,
      createdAt: 1700000200,
    ),
    Todo.create('Deploy to production', priority: 3),
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
    print('  $todo');
  }

  // Filter incomplete high-priority todos using Dart `where` (NO SQL!)
  print('\n⚡ High-priority incomplete todos:');
  final urgent = allTodos.where((t) => !t.completed && t.priority == 1);

  for (final todo in urgent) {
    print('  ○ ${todo.title}');
  }

  // Demonstrate iterator usage (manual iteration)
  print('\n🔄 Using iterator:');
  final iterator = db.iter(todosCollection);
  var count = 0;
  try {
    while (iterator.moveNext()) {
      final (id, bytes) = iterator.current;
      final todo = Todo.fromBytes(id, bytes);
      count++;
      print('  ${todo.title} (id: ${id.toHexString().substring(0, 8)}...)');
    }
  } finally {
    iterator.dispose();
  }
  print('  Iterated $count todos');

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
  print('  Total count: ${db.count(todosCollection)}');

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
