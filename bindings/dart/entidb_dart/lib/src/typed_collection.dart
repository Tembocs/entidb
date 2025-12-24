/// Typed collection wrapper for type-safe entity operations.
library;

import 'codec.dart';
import 'collection.dart';
import 'database.dart';
import 'entity_id.dart';

/// A type-safe wrapper around a collection.
///
/// Provides typed get/put/delete operations using a [Codec].
///
/// ## Example
///
/// ```dart
/// final userCodec = FunctionCodec<User>(
///   encode: (user) => cbor.encode(user.toMap()),
///   decode: (bytes) => User.fromMap(cbor.decode(bytes)),
/// );
///
/// final users = TypedCollection<User>(
///   db,
///   db.collection('users'),
///   userCodec,
/// );
///
/// // Type-safe operations
/// users.put(userId, user);
/// final user = users.get(userId);
/// ```
final class TypedCollection<T> {
  /// The database instance.
  final Database db;

  /// The underlying collection.
  final Collection collection;

  /// The codec for serialization.
  final Codec<T> codec;

  /// Creates a typed collection.
  const TypedCollection(this.db, this.collection, this.codec);

  /// The collection name.
  String get name => collection.name;

  /// The collection ID.
  int get id => collection.id;

  /// Stores an entity.
  void put(EntityId entityId, T value) {
    final data = codec.encode(value);
    db.put(collection, entityId, data);
  }

  /// Retrieves an entity.
  ///
  /// Returns `null` if not found.
  T? get(EntityId entityId) {
    final data = db.get(collection, entityId);
    if (data == null) return null;
    return codec.decode(data);
  }

  /// Deletes an entity.
  void delete(EntityId entityId) {
    db.delete(collection, entityId);
  }

  /// Lists all entities.
  List<(EntityId, T)> list() {
    return db.list(collection).map((record) {
      final (id, bytes) = record;
      return (id, codec.decode(bytes));
    }).toList();
  }

  /// Returns the number of entities.
  int count() => db.count(collection);

  /// Iterates over all entities.
  ///
  /// Returns an iterable that yields (EntityId, T) pairs.
  Iterable<(EntityId, T)> iterate() sync* {
    final iter = db.iter(collection);
    try {
      while (iter.moveNext()) {
        yield (iter.currentId, codec.decode(iter.currentData));
      }
    } finally {
      iter.dispose();
    }
  }
}

/// Extension to create typed collections from Database.
extension TypedCollectionExtension on Database {
  /// Creates a typed collection.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final users = db.typedCollection<User>('users', userCodec);
  /// users.put(id, User(name: 'Alice'));
  /// ```
  TypedCollection<T> typedCollection<T>(String name, Codec<T> codec) {
    return TypedCollection<T>(this, collection(name), codec);
  }
}
