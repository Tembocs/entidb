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

  group('Checkpoint', () {
    test('checkpoint succeeds on clean database', () {
      final db = Database.openMemory();
      try {
        // Should not throw
        db.checkpoint();
      } finally {
        db.close();
      }
    });

    test('checkpoint after writes', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        // Add some data
        for (var i = 0; i < 10; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        // Checkpoint should succeed
        db.checkpoint();

        // Data should still be accessible
        expect(db.count(users), equals(10));
      } finally {
        db.close();
      }
    });
  });

  group('Backup and Restore', () {
    test('backup creates data', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        // Add some data
        final id = EntityId.generate();
        db.put(users, id, Uint8List.fromList([1, 2, 3, 4, 5]));

        // Create backup
        final backupData = db.backup();
        expect(backupData, isNotEmpty);
      } finally {
        db.close();
      }
    });

    test('backup and restore roundtrip', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        // Add some data
        final id1 = EntityId.generate();
        final id2 = EntityId.generate();
        db.put(users, id1, Uint8List.fromList([1, 2, 3]));
        db.put(users, id2, Uint8List.fromList([4, 5, 6]));

        // Create backup
        final backupData = db.backup();

        // Create new database and restore
        final db2 = Database.openMemory();
        try {
          final stats = db2.restore(backupData);
          expect(stats.entitiesRestored, equals(2));

          // Verify data
          final users2 = db2.collection('users');
          expect(db2.get(users2, id1), equals(Uint8List.fromList([1, 2, 3])));
          expect(db2.get(users2, id2), equals(Uint8List.fromList([4, 5, 6])));
        } finally {
          db2.close();
        }
      } finally {
        db.close();
      }
    });

    test('backup with options includes tombstones', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        // Add and delete some data
        final id = EntityId.generate();
        db.put(users, id, Uint8List.fromList([1, 2, 3]));
        db.delete(users, id);

        // Backup without tombstones
        final backupWithout = db.backupWithOptions(includeTombstones: false);

        // Backup with tombstones
        final backupWith = db.backupWithOptions(includeTombstones: true);

        // Backup with tombstones should be larger or equal
        expect(backupWith.length, greaterThanOrEqualTo(backupWithout.length));
      } finally {
        db.close();
      }
    });

    test('restore returns valid stats', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        // Add data to multiple collections
        for (var i = 0; i < 5; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        final notes = db.collection('notes');
        for (var i = 0; i < 3; i++) {
          db.put(notes, EntityId.generate(), Uint8List.fromList([i + 10]));
        }

        // Create backup and restore to new db
        final backupData = db.backup();

        final db2 = Database.openMemory();
        try {
          final stats = db2.restore(backupData);
          expect(stats.entitiesRestored, equals(8));
          expect(stats.tombstonesApplied, greaterThanOrEqualTo(0));
          expect(stats.backupSequence, greaterThan(0));
        } finally {
          db2.close();
        }
      } finally {
        db.close();
      }
    });

    test('validate backup returns info', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');

        // Add data
        for (var i = 0; i < 5; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        // Create backup
        final backupData = db.backup();

        // Validate backup
        final info = db.validateBackup(backupData);
        expect(info.recordCount, greaterThanOrEqualTo(5));
        expect(info.valid, isTrue);
        expect(info.sequence, greaterThan(0));
        expect(info.size, equals(backupData.length));
      } finally {
        db.close();
      }
    });

    test('validate backup detects invalid data', () {
      final db = Database.openMemory();
      try {
        // Invalid backup data
        final invalidData = Uint8List.fromList([0, 1, 2, 3, 4, 5]);

        // Should throw for invalid backup
        expect(
          () => db.validateBackup(invalidData),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        db.close();
      }
    });

    test('restore to non-empty database', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();
        db.put(users, id, Uint8List.fromList([1, 2, 3]));

        final backupData = db.backup();

        // Create new database with existing data
        final db2 = Database.openMemory();
        try {
          final existingUsers = db2.collection('users');
          db2.put(existingUsers, EntityId.generate(),
              Uint8List.fromList([9, 8, 7]));

          // Restore should add data
          final stats = db2.restore(backupData);
          expect(stats.entitiesRestored, equals(1));

          // Should now have both entities
          expect(db2.count(existingUsers), equals(2));
        } finally {
          db2.close();
        }
      } finally {
        db.close();
      }
    });
  });

  group('Database Properties', () {
    test('committedSeq returns valid sequence', () {
      final db = Database.openMemory();
      try {
        final initialSeq = db.committedSeq;
        expect(initialSeq, greaterThanOrEqualTo(0));

        // Write some data
        final users = db.collection('users');
        db.put(users, EntityId.generate(), Uint8List.fromList([1]));

        // Sequence should advance
        final newSeq = db.committedSeq;
        expect(newSeq, greaterThan(initialSeq));
      } finally {
        db.close();
      }
    });

    test('entityCount returns correct count', () {
      final db = Database.openMemory();
      try {
        expect(db.entityCount, equals(0));

        final users = db.collection('users');
        for (var i = 0; i < 5; i++) {
          db.put(users, EntityId.generate(), Uint8List.fromList([i]));
        }

        expect(db.entityCount, equals(5));

        final notes = db.collection('notes');
        for (var i = 0; i < 3; i++) {
          db.put(notes, EntityId.generate(), Uint8List.fromList([i]));
        }

        expect(db.entityCount, equals(8));
      } finally {
        db.close();
      }
    });

    test('entityCount includes tombstones until compaction', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        final id = EntityId.generate();

        db.put(users, id, Uint8List.fromList([1, 2, 3]));
        expect(db.entityCount, equals(1));

        db.delete(users, id);
        // Entity count may still include tombstone
        expect(db.entityCount, greaterThanOrEqualTo(0));

        // But the entity should not be retrievable
        expect(db.get(users, id), isNull);
      } finally {
        db.close();
      }
    });
  });

  group('Hash Index', () {
    test('create and lookup', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createHashIndex(users, 'email_idx');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();
        final key1 = Uint8List.fromList([1, 2, 3]);
        final key2 = Uint8List.fromList([4, 5, 6]);

        db.hashIndexInsert(users, 'email_idx', key1, id1);
        db.hashIndexInsert(users, 'email_idx', key2, id2);

        final results = db.hashIndexLookup(users, 'email_idx', key1);
        expect(results.length, equals(1));
        expect(results.first, equals(id1));

        expect(db.hashIndexLen(users, 'email_idx'), equals(2));
      } finally {
        db.close();
      }
    });

    test('non-unique index allows duplicates', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createHashIndex(users, 'tag_idx', unique: false);

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();
        final sameKey = Uint8List.fromList([1, 2, 3]);

        db.hashIndexInsert(users, 'tag_idx', sameKey, id1);
        db.hashIndexInsert(users, 'tag_idx', sameKey, id2);

        final results = db.hashIndexLookup(users, 'tag_idx', sameKey);
        expect(results.length, equals(2));
        expect(results, containsAll([id1, id2]));
      } finally {
        db.close();
      }
    });

    test('unique index rejects duplicates', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createHashIndex(users, 'email_idx', unique: true);

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();
        final sameKey = Uint8List.fromList([1, 2, 3]);

        db.hashIndexInsert(users, 'email_idx', sameKey, id1);

        expect(
          () => db.hashIndexInsert(users, 'email_idx', sameKey, id2),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        db.close();
      }
    });

    test('remove entry', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createHashIndex(users, 'idx');

        final id = EntityId.generate();
        final key = Uint8List.fromList([1, 2, 3]);

        db.hashIndexInsert(users, 'idx', key, id);
        expect(db.hashIndexLen(users, 'idx'), equals(1));

        db.hashIndexRemove(users, 'idx', key, id);
        expect(db.hashIndexLen(users, 'idx'), equals(0));
        expect(db.hashIndexLookup(users, 'idx', key), isEmpty);
      } finally {
        db.close();
      }
    });

    test('drop index', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createHashIndex(users, 'idx');

        final id = EntityId.generate();
        db.hashIndexInsert(users, 'idx', Uint8List.fromList([1]), id);

        db.dropHashIndex(users, 'idx');

        // After dropping, trying to use should fail
        expect(
          () => db.hashIndexLen(users, 'idx'),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        db.close();
      }
    });
  });

  group('BTree Index', () {
    test('create and lookup', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createBTreeIndex(users, 'age_idx');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();
        // Use big-endian for proper ordering
        final key25 = Uint8List.fromList([0, 0, 0, 25]);
        final key30 = Uint8List.fromList([0, 0, 0, 30]);

        db.btreeIndexInsert(users, 'age_idx', key25, id1);
        db.btreeIndexInsert(users, 'age_idx', key30, id2);

        final results = db.btreeIndexLookup(users, 'age_idx', key25);
        expect(results.length, equals(1));
        expect(results.first, equals(id1));

        expect(db.btreeIndexLen(users, 'age_idx'), equals(2));
      } finally {
        db.close();
      }
    });

    test('range query', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createBTreeIndex(users, 'score_idx');

        final id10 = EntityId.generate();
        final id20 = EntityId.generate();
        final id30 = EntityId.generate();
        final id40 = EntityId.generate();

        // Big-endian keys for proper ordering
        db.btreeIndexInsert(
            users, 'score_idx', Uint8List.fromList([0, 0, 0, 10]), id10);
        db.btreeIndexInsert(
            users, 'score_idx', Uint8List.fromList([0, 0, 0, 20]), id20);
        db.btreeIndexInsert(
            users, 'score_idx', Uint8List.fromList([0, 0, 0, 30]), id30);
        db.btreeIndexInsert(
            users, 'score_idx', Uint8List.fromList([0, 0, 0, 40]), id40);

        // Range [15, 35] should return 20 and 30
        final results = db.btreeIndexRange(
          users,
          'score_idx',
          minKey: Uint8List.fromList([0, 0, 0, 15]),
          maxKey: Uint8List.fromList([0, 0, 0, 35]),
        );

        expect(results.length, equals(2));
        expect(results, containsAll([id20, id30]));
      } finally {
        db.close();
      }
    });

    test('unbounded range queries', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createBTreeIndex(users, 'idx');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();
        final id3 = EntityId.generate();

        db.btreeIndexInsert(users, 'idx', Uint8List.fromList([0, 1]), id1);
        db.btreeIndexInsert(users, 'idx', Uint8List.fromList([0, 2]), id2);
        db.btreeIndexInsert(users, 'idx', Uint8List.fromList([0, 3]), id3);

        // No bounds - get all
        final all = db.btreeIndexRange(users, 'idx');
        expect(all.length, equals(3));

        // Only min bound
        final fromMiddle = db.btreeIndexRange(
          users,
          'idx',
          minKey: Uint8List.fromList([0, 2]),
        );
        expect(fromMiddle.length, equals(2));
        expect(fromMiddle, containsAll([id2, id3]));

        // Only max bound
        final toMiddle = db.btreeIndexRange(
          users,
          'idx',
          maxKey: Uint8List.fromList([0, 2]),
        );
        expect(toMiddle.length, equals(2));
        expect(toMiddle, containsAll([id1, id2]));
      } finally {
        db.close();
      }
    });

    test('drop index', () {
      final db = Database.openMemory();
      try {
        final users = db.collection('users');
        db.createBTreeIndex(users, 'idx');

        final id = EntityId.generate();
        db.btreeIndexInsert(users, 'idx', Uint8List.fromList([1]), id);

        db.dropBTreeIndex(users, 'idx');

        expect(
          () => db.btreeIndexLen(users, 'idx'),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        db.close();
      }
    });
  });

  // ==========================================================================
  // FTS (Full-Text Search) Index Tests
  // ==========================================================================
  group('FTS Index', () {
    test('create FTS index', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        // Index should exist and be empty
        expect(db.ftsIndexLen(docs, 'content'), equals(0));
      } finally {
        db.close();
      }
    });

    test('create FTS index with custom config', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndexWithConfig(
          docs,
          'content',
          minTokenLength: 2,
          maxTokenLength: 100,
          caseSensitive: true,
        );

        expect(db.ftsIndexLen(docs, 'content'), equals(0));
      } finally {
        db.close();
      }
    });

    test('index and search text', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();

        db.ftsIndexText(docs, 'content', id1, 'Hello world from Rust');
        db.ftsIndexText(docs, 'content', id2, 'Hello Python programming');

        // Search for "hello" - should find both
        final results = db.ftsSearch(docs, 'content', 'hello');
        expect(results.length, equals(2));
        expect(results, contains(id1));
        expect(results, contains(id2));

        // Search for "rust" - should find only id1
        final rustResults = db.ftsSearch(docs, 'content', 'rust');
        expect(rustResults.length, equals(1));
        expect(rustResults, contains(id1));

        // Search for "python" - should find only id2
        final pythonResults = db.ftsSearch(docs, 'content', 'python');
        expect(pythonResults.length, equals(1));
        expect(pythonResults, contains(id2));
      } finally {
        db.close();
      }
    });

    test('search with AND semantics', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();

        db.ftsIndexText(docs, 'content', id1, 'Hello world');
        db.ftsIndexText(docs, 'content', id2, 'Hello Rust');

        // "hello world" - only id1 has both
        final results = db.ftsSearch(docs, 'content', 'hello world');
        expect(results.length, equals(1));
        expect(results, contains(id1));

        // "hello rust" - only id2 has both
        final rustResults = db.ftsSearch(docs, 'content', 'hello rust');
        expect(rustResults.length, equals(1));
        expect(rustResults, contains(id2));

        // "world rust" - neither has both
        final noneResults = db.ftsSearch(docs, 'content', 'world rust');
        expect(noneResults.length, equals(0));
      } finally {
        db.close();
      }
    });

    test('search with OR semantics', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();

        db.ftsIndexText(docs, 'content', id1, 'apple orange');
        db.ftsIndexText(docs, 'content', id2, 'banana orange');

        // "apple banana" with OR - both should match (id1 has apple, id2 has banana)
        final results = db.ftsSearchAny(docs, 'content', 'apple banana');
        expect(results.length, equals(2));
        expect(results, contains(id1));
        expect(results, contains(id2));

        // "grape" - neither should match
        final noResults = db.ftsSearchAny(docs, 'content', 'grape');
        expect(noResults.length, equals(0));
      } finally {
        db.close();
      }
    });

    test('prefix search', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();

        db.ftsIndexText(docs, 'content', id1, 'programming in Rust');
        db.ftsIndexText(docs, 'content', id2, 'program management');

        // "prog" prefix - should find both
        final results = db.ftsSearchPrefix(docs, 'content', 'prog');
        expect(results.length, equals(2));
        expect(results, contains(id1));
        expect(results, contains(id2));

        // "rust" prefix - should find only id1
        final rustResults = db.ftsSearchPrefix(docs, 'content', 'rust');
        expect(rustResults.length, equals(1));
        expect(rustResults, contains(id1));

        // "xyz" prefix - no matches
        final noResults = db.ftsSearchPrefix(docs, 'content', 'xyz');
        expect(noResults.length, equals(0));
      } finally {
        db.close();
      }
    });

    test('remove entity from FTS index', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();

        db.ftsIndexText(docs, 'content', id1, 'hello world');
        db.ftsIndexText(docs, 'content', id2, 'hello rust');

        // Both should be found
        expect(db.ftsSearch(docs, 'content', 'hello').length, equals(2));

        // Remove id1
        db.ftsRemoveEntity(docs, 'content', id1);

        // Now only id2 should be found
        final results = db.ftsSearch(docs, 'content', 'hello');
        expect(results.length, equals(1));
        expect(results, contains(id2));

        // "world" should find nothing
        expect(db.ftsSearch(docs, 'content', 'world').length, equals(0));
      } finally {
        db.close();
      }
    });

    test('reindex entity replaces old content', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id = EntityId.generate();

        // Index initial text
        db.ftsIndexText(docs, 'content', id, 'Hello world');
        expect(db.ftsSearch(docs, 'content', 'hello').length, equals(1));
        expect(db.ftsSearch(docs, 'content', 'world').length, equals(1));

        // Reindex with different text
        db.ftsIndexText(docs, 'content', id, 'Goodbye Rust');

        // Old terms should not match
        expect(db.ftsSearch(docs, 'content', 'hello').length, equals(0));
        expect(db.ftsSearch(docs, 'content', 'world').length, equals(0));

        // New terms should match
        expect(db.ftsSearch(docs, 'content', 'goodbye').length, equals(1));
        expect(db.ftsSearch(docs, 'content', 'rust').length, equals(1));
      } finally {
        db.close();
      }
    });

    test('case insensitivity (default)', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id = EntityId.generate();
        db.ftsIndexText(docs, 'content', id, 'HELLO World RuSt');

        // All variations should match
        expect(db.ftsSearch(docs, 'content', 'hello').length, equals(1));
        expect(db.ftsSearch(docs, 'content', 'HELLO').length, equals(1));
        expect(db.ftsSearch(docs, 'content', 'Hello').length, equals(1));
        expect(db.ftsSearch(docs, 'content', 'rust').length, equals(1));
        expect(db.ftsSearch(docs, 'content', 'RUST').length, equals(1));
      } finally {
        db.close();
      }
    });

    test('case sensitivity with config', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndexWithConfig(
          docs,
          'content',
          caseSensitive: true,
        );

        final id = EntityId.generate();
        db.ftsIndexText(docs, 'content', id, 'Hello World');

        // Exact case should match
        expect(db.ftsSearch(docs, 'content', 'Hello').length, equals(1));

        // Different case should NOT match
        expect(db.ftsSearch(docs, 'content', 'hello').length, equals(0));
        expect(db.ftsSearch(docs, 'content', 'HELLO').length, equals(0));
      } finally {
        db.close();
      }
    });

    test('unique token count', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id1 = EntityId.generate();
        final id2 = EntityId.generate();

        // "hello world hello" - unique: hello, world
        db.ftsIndexText(docs, 'content', id1, 'hello world hello');
        // "hello rust" - adds rust
        db.ftsIndexText(docs, 'content', id2, 'hello rust');

        // Total unique tokens: hello, world, rust = 3
        expect(db.ftsUniqueTokenCount(docs, 'content'), equals(3));
      } finally {
        db.close();
      }
    });

    test('clear FTS index', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        for (var i = 0; i < 5; i++) {
          final id = EntityId.generate();
          db.ftsIndexText(docs, 'content', id, 'document $i');
        }

        expect(db.ftsIndexLen(docs, 'content'), equals(5));

        db.ftsClear(docs, 'content');

        expect(db.ftsIndexLen(docs, 'content'), equals(0));
        expect(db.ftsSearch(docs, 'content', 'document').length, equals(0));
      } finally {
        db.close();
      }
    });

    test('drop FTS index', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id = EntityId.generate();
        db.ftsIndexText(docs, 'content', id, 'hello world');

        db.dropFtsIndex(docs, 'content');

        // Operations on dropped index should throw
        expect(
          () => db.ftsSearch(docs, 'content', 'hello'),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        db.close();
      }
    });

    test('nonexistent index throws error', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        final id = EntityId.generate();

        expect(
          () => db.ftsIndexText(docs, 'nonexistent', id, 'text'),
          throwsA(isA<EntiDbError>()),
        );

        expect(
          () => db.ftsSearch(docs, 'nonexistent', 'query'),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        db.close();
      }
    });

    test('empty query returns empty results', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'content');

        final id = EntityId.generate();
        db.ftsIndexText(docs, 'content', id, 'Hello world');

        // Empty query should return empty results
        expect(db.ftsSearch(docs, 'content', '').length, equals(0));
        expect(db.ftsSearchAny(docs, 'content', '').length, equals(0));
      } finally {
        db.close();
      }
    });

    test('multiple indexes per collection', () {
      final db = Database.openMemory();
      try {
        final docs = db.collection('documents');
        db.createFtsIndex(docs, 'title');
        db.createFtsIndex(docs, 'body');

        final id = EntityId.generate();
        db.ftsIndexText(docs, 'title', id, 'Rust Programming Guide');
        db.ftsIndexText(docs, 'body', id, 'Learn Rust today with examples');

        // "guide" in title, not in body
        expect(db.ftsSearch(docs, 'title', 'guide').length, equals(1));
        expect(db.ftsSearch(docs, 'body', 'guide').length, equals(0));

        // "examples" in body, not in title
        expect(db.ftsSearch(docs, 'body', 'examples').length, equals(1));
        expect(db.ftsSearch(docs, 'title', 'examples').length, equals(0));
      } finally {
        db.close();
      }
    });
  });
}
