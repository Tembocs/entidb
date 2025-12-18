import 'dart:typed_data';

import 'package:entidb_dart/entidb_dart.dart';
import 'package:test/test.dart';

/// Note: These tests require the native library to be built.
/// Run: `cargo build --release -p entidb_ffi` first.
void main() {
  group('EntityId', () {
    test('generate creates unique IDs', () {
      final id1 = EntityId.generate();
      final id2 = EntityId.generate();
      expect(id1, isNot(equals(id2)));
    });

    test('fromBytes creates ID from bytes', () {
      final bytes = Uint8List.fromList(List.filled(16, 42));
      final id = EntityId.fromBytes(bytes);
      expect(id.bytes, equals(bytes));
    });

    test('zero creates zero ID', () {
      final id = EntityId.zero();
      expect(id.bytes, equals(Uint8List(16)));
    });

    test('equality works', () {
      final bytes = Uint8List.fromList(List.filled(16, 1));
      final id1 = EntityId.fromBytes(bytes);
      final id2 = EntityId.fromBytes(Uint8List.fromList(bytes));
      expect(id1, equals(id2));
    });
  });

  group('Database', () {
    test('openMemory creates database', () {
      final db = Database.openMemory();
      expect(db.isOpen, isTrue);
      db.close();
      expect(db.isOpen, isFalse);
    });

    test('version returns string', () {
      expect(Database.version, isNotEmpty);
    });

    test('collection creates collection', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        expect(users.name, equals('users'));
        expect(users.id, greaterThanOrEqualTo(0));
      } finally {
        db.close();
      }
    });

    test('put and get entity', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();
        final data = Uint8List.fromList([1, 2, 3, 4, 5]);

        db.put(users, id, data);

        final result = db.get(users, id);
        expect(result, equals(data));
      } finally {
        db.close();
      }
    });

    test('get returns null for missing entity', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();

        final result = db.get(users, id);
        expect(result, isNull);
      } finally {
        db.close();
      }
    });

    test('delete entity', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();
        final data = Uint8List.fromList([1, 2, 3]);

        db.put(users, id, data);
        expect(db.get(users, id), isNotNull);

        db.delete(users, id);
        expect(db.get(users, id), isNull);
      } finally {
        db.close();
      }
    });

    test('count entities', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        expect(db.count(users), equals(0));

        for (var i = 0; i < 5; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        expect(db.count(users), equals(5));
      } finally {
        db.close();
      }
    });
  });

  group('Transaction', () {
    test('commit persists data', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();
        final data = Uint8List.fromList([1, 2, 3]);

        db.transaction((txn) {
          txn.put(users, id, data);
        });

        expect(db.get(users, id), equals(data));
      } finally {
        db.close();
      }
    });

    test('abort discards data', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();

        try {
          db.transaction((txn) {
            txn.put(users, id, Uint8List.fromList([1, 2, 3]));
            throw Exception('abort');
          });
        } catch (e) {
          // Expected
        }

        expect(db.get(users, id), isNull);
      } finally {
        db.close();
      }
    });

    test('transaction sees uncommitted writes', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();
        final data = Uint8List.fromList([1, 2, 3]);

        db.transaction((txn) {
          txn.put(users, id, data);

          final result = txn.get(users, id);
          expect(result, equals(data));
        });
      } finally {
        db.close();
      }
    });
  });

  group('EntityIterator', () {
    test('iterate empty collection', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final entities = db.list(users);
        expect(entities, isEmpty);
      } finally {
        db.close();
      }
    });

    test('iterate collection with data', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        for (var i = 0; i < 3; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        final entities = db.list(users);
        expect(entities.length, equals(3));
      } finally {
        db.close();
      }
    });

    test('forEach iterates all entities', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        for (var i = 0; i < 5; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        var count = 0;
        db.iterate(users).forEach((id, data) {
          count++;
        });
        expect(count, equals(5));
      } finally {
        db.close();
      }
    });
  });
}
