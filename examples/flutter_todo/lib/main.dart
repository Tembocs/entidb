// EntiDB Flutter Example - Todo Application
//
// This example demonstrates:
// - Opening a database with path_provider
// - Basic CRUD operations with Flutter UI
// - Filtering with Dart's `where` clause (no SQL!)
// - Transaction usage for atomic operations
//
// Run with: flutter run

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:entidb_flutter/entidb_flutter.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

void main() {
  runApp(const TodoApp());
}

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
}

class TodoApp extends StatelessWidget {
  const TodoApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'EntiDB Todo',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.blue),
        useMaterial3: true,
      ),
      home: const TodoListPage(),
    );
  }
}

class TodoListPage extends StatefulWidget {
  const TodoListPage({super.key});

  @override
  State<TodoListPage> createState() => _TodoListPageState();
}

class _TodoListPageState extends State<TodoListPage> {
  Database? _db;
  Collection? _todosCollection;
  List<Todo> _todos = [];
  bool _isLoading = true;
  String? _error;
  final _textController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _initDatabase();
  }

  Future<void> _initDatabase() async {
    try {
      // Get the application documents directory
      final dir = await getApplicationDocumentsDirectory();
      final dbPath = '${dir.path}${Platform.pathSeparator}entidb_todo';

      // Open the database
      _db = Database.open(dbPath);
      _todosCollection = _db!.collection('todos');

      await _loadTodos();
    } catch (e) {
      setState(() {
        _error = e.toString();
        _isLoading = false;
      });
    }
  }

  Future<void> _loadTodos() async {
    if (_db == null || _todosCollection == null) return;

    setState(() => _isLoading = true);

    try {
      // Iterate all entities and decode them
      final todos = <Todo>[];
      final iter = _db!.iter(_todosCollection!);
      try {
        while (iter.moveNext()) {
          final (id, data) = iter.current;
          final todo = Todo.fromBytes(id, data);
          todos.add(todo);
        }
      } finally {
        iter.dispose();
      }

      // Sort by priority (high first), then by creation time
      // This is host-language filtering - NO SQL!
      todos.sort((a, b) {
        final priorityCompare = b.priority.compareTo(a.priority);
        if (priorityCompare != 0) return priorityCompare;
        return a.createdAt.compareTo(b.createdAt);
      });

      setState(() {
        _todos = todos;
        _isLoading = false;
      });
    } catch (e) {
      setState(() {
        _error = e.toString();
        _isLoading = false;
      });
    }
  }

  Future<void> _addTodo(String title) async {
    if (_db == null || _todosCollection == null || title.isEmpty) return;

    final todo = Todo.create(title);
    _db!.put(_todosCollection!, todo.id, todo.toBytes());
    _textController.clear();
    await _loadTodos();
  }

  Future<void> _toggleTodo(Todo todo) async {
    if (_db == null || _todosCollection == null) return;

    final updated = todo.copyWith(completed: !todo.completed);
    _db!.put(_todosCollection!, updated.id, updated.toBytes());
    await _loadTodos();
  }

  Future<void> _deleteTodo(Todo todo) async {
    if (_db == null || _todosCollection == null) return;

    _db!.delete(_todosCollection!, todo.id);
    await _loadTodos();
  }

  Future<void> _clearCompleted() async {
    if (_db == null || _todosCollection == null) return;

    // Use transaction for atomic delete of multiple items
    _db!.transaction((txn) {
      // Filter completed todos - host-language filtering!
      final completed = _todos.where((t) => t.completed);
      for (final todo in completed) {
        txn.delete(_todosCollection!, todo.id);
      }
    });
    await _loadTodos();
  }

  @override
  void dispose() {
    _db?.close();
    _textController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('EntiDB Todo'),
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        actions: [
          IconButton(
            icon: const Icon(Icons.delete_sweep),
            onPressed: _todos.any((t) => t.completed) ? _clearCompleted : null,
            tooltip: 'Clear completed',
          ),
        ],
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    if (_error != null) {
      return Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Icon(Icons.error, size: 48, color: Colors.red),
            const SizedBox(height: 16),
            Text('Error: $_error'),
          ],
        ),
      );
    }

    if (_isLoading) {
      return const Center(child: CircularProgressIndicator());
    }

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(16),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _textController,
                  decoration: const InputDecoration(
                    hintText: 'Add a new todo...',
                    border: OutlineInputBorder(),
                  ),
                  onSubmitted: _addTodo,
                ),
              ),
              const SizedBox(width: 8),
              IconButton.filled(
                icon: const Icon(Icons.add),
                onPressed: () => _addTodo(_textController.text),
              ),
            ],
          ),
        ),
        Expanded(
          child: _todos.isEmpty
              ? const Center(
                  child: Text(
                    'No todos yet!\nAdd one above.',
                    textAlign: TextAlign.center,
                    style: TextStyle(color: Colors.grey),
                  ),
                )
              : ListView.builder(
                  itemCount: _todos.length,
                  itemBuilder: (context, index) {
                    final todo = _todos[index];
                    return ListTile(
                      leading: Checkbox(
                        value: todo.completed,
                        onChanged: (_) => _toggleTodo(todo),
                      ),
                      title: Text(
                        todo.title,
                        style: TextStyle(
                          decoration: todo.completed
                              ? TextDecoration.lineThrough
                              : null,
                          color: todo.completed ? Colors.grey : null,
                        ),
                      ),
                      subtitle: Text('Priority: ${todo.priority}'),
                      trailing: IconButton(
                        icon: const Icon(Icons.delete),
                        onPressed: () => _deleteTodo(todo),
                      ),
                    );
                  },
                ),
        ),
        Padding(
          padding: const EdgeInsets.all(16),
          child: Text(
            '${_todos.where((t) => !t.completed).length} items remaining',
            style: const TextStyle(color: Colors.grey),
          ),
        ),
      ],
    );
  }
}
