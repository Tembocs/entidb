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

    test('fromList creates ID from list', () {
      final list = List.filled(16, 123);
      final id = EntityId.fromList(list);
      expect(id.bytes, equals(Uint8List.fromList(list)));
    });

    test('zero creates zero ID', () {
      final id = EntityId.zero();
      expect(id.bytes, equals(Uint8List(16)));
      expect(id.isZero, isTrue);
    });

    test('equality works', () {
      final bytes = Uint8List.fromList(List.filled(16, 1));
      final id1 = EntityId.fromBytes(bytes);
      final id2 = EntityId.fromBytes(Uint8List.fromList(bytes));
      expect(id1, equals(id2));
      expect(id1.hashCode, equals(id2.hashCode));
    });

    test('compareTo works', () {
      final id1 = EntityId.fromList(List.filled(16, 1));
      final id2 = EntityId.fromList(List.filled(16, 2));
      expect(id1.compareTo(id2), lessThan(0));
      expect(id2.compareTo(id1), greaterThan(0));
      expect(id1.compareTo(id1), equals(0));
    });

    test('toHexString produces valid hex', () {
      final bytes = Uint8List.fromList(List.generate(16, (i) => i));
      final id = EntityId.fromBytes(bytes);
      expect(id.toHexString(), equals('000102030405060708090a0b0c0d0e0f'));
    });

    test('toString includes hex', () {
      final id = EntityId.fromList(List.filled(16, 0xab));
      expect(id.toString(), contains('abababab'));
    });

    test('fromBytes throws for wrong length', () {
      expect(
        () => EntityId.fromBytes(Uint8List(15)),
        throwsA(isA<ArgumentError>()),
      );
      expect(
        () => EntityId.fromBytes(Uint8List(17)),
        throwsA(isA<ArgumentError>()),
      );
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

        final iter = db.iter(users);
        var count = 0;
        try {
          iter.forEach((id, data) {
            count++;
          });
        } finally {
          iter.dispose();
        }
        expect(count, equals(5));
      } finally {
        db.close();
      }
    });

    test('iterator remaining count', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        for (var i = 0; i < 3; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        final iter = db.iter(users);
        try {
          expect(iter.remaining, equals(3));
          expect(iter.hasNext, isTrue);

          iter.moveNext();
          expect(iter.remaining, equals(2));

          iter.moveNext();
          iter.moveNext();
          expect(iter.remaining, equals(0));
          expect(iter.hasNext, isFalse);
        } finally {
          iter.dispose();
        }
      } finally {
        db.close();
      }
    });
  });

  group('Codec', () {
    test('StringCodec encodes and decodes', () {
      const codec = StringCodec.instance;
      const original = 'Hello, EntiDB!';

      final encoded = codec.encode(original);
      final decoded = codec.decode(encoded);

      expect(decoded, equals(original));
    });

    test('BytesCodec is passthrough', () {
      const codec = BytesCodec.instance;
      final original = Uint8List.fromList([1, 2, 3, 4, 5]);

      final encoded = codec.encode(original);
      final decoded = codec.decode(encoded);

      expect(decoded, equals(original));
    });

    test('FunctionCodec works with custom functions', () {
      final codec = FunctionCodec<int>(
        encode: (value) => Uint8List.fromList([
          value & 0xFF,
          (value >> 8) & 0xFF,
          (value >> 16) & 0xFF,
          (value >> 24) & 0xFF,
        ]),
        decode: (bytes) =>
            bytes[0] | (bytes[1] << 8) | (bytes[2] << 16) | (bytes[3] << 24),
      );

      const original = 12345678;
      final encoded = codec.encode(original);
      final decoded = codec.decode(encoded);

      expect(decoded, equals(original));
    });
  });

  group('TypedCollection', () {
    test('put and get with codec', () {
      final db = Database.openMemory();
      try {
        final users = db.typedCollection<String>('users', StringCodec.instance);

        final id = EntityId.generate();
        users.put(id, 'Alice');

        final result = users.get(id);
        expect(result, equals('Alice'));
      } finally {
        db.close();
      }
    });

    test('delete works', () {
      final db = Database.openMemory();
      try {
        final users = db.typedCollection<String>('users', StringCodec.instance);

        final id = EntityId.generate();
        users.put(id, 'Bob');
        expect(users.get(id), isNotNull);

        users.delete(id);
        expect(users.get(id), isNull);
      } finally {
        db.close();
      }
    });

    test('list returns all entities', () {
      final db = Database.openMemory();
      try {
        final users = db.typedCollection<String>('users', StringCodec.instance);

        final names = ['Alice', 'Bob', 'Charlie'];
        for (final name in names) {
          users.put(EntityId.generate(), name);
        }

        final results = users.list().map((r) => r.$2).toList();
        expect(results.length, equals(3));
        expect(results, containsAll(names));
      } finally {
        db.close();
      }
    });

    test('count returns correct count', () {
      final db = Database.openMemory();
      try {
        final users = db.typedCollection<String>('users', StringCodec.instance);

        expect(users.count(), equals(0));

        for (var i = 0; i < 5; i++) {
          users.put(EntityId.generate(), 'User $i');
        }

        expect(users.count(), equals(5));
      } finally {
        db.close();
      }
    });

    test('iterate yields all entities', () {
      final db = Database.openMemory();
      try {
        final users = db.typedCollection<String>('users', StringCodec.instance);

        final names = ['Alice', 'Bob', 'Charlie'];
        for (final name in names) {
          users.put(EntityId.generate(), name);
        }

        final results = <String>[];
        for (final (_, name) in users.iterate()) {
          results.add(name);
        }

        expect(results.length, equals(3));
        expect(results, containsAll(names));
      } finally {
        db.close();
      }
    });
  });

  group('Error handling', () {
    test('operations on closed database throw', () {
      final db = Database.openMemory();
      final users = db.collection('users');
      db.close();

      expect(
        () => db.get(users, EntityId.generate()),
        throwsA(isA<EntiDbInvalidError>()),
      );
    });

    test('collection caching works', () {
      final db = Database.openMemory();
      try {
        final users1 = db.collection('users');
        final users2 = db.collection('users');

        expect(identical(users1, users2), isTrue);
      } finally {
        db.close();
      }
    });
  });
}
