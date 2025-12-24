/// Database class - main entry point for EntiDB.
library;

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.dart';
import 'collection.dart';
import 'entity_id.dart';
import 'error.dart';
import 'iterator.dart';
import 'transaction.dart';

/// The main entry point for interacting with EntiDB.
///
/// A database can be opened either in-memory or file-based:
///
/// ```dart
/// // In-memory database (fast, not persistent)
/// final memDb = Database.openMemory();
///
/// // File-based database (persistent)
/// final fileDb = Database.open('/path/to/database');
/// ```
///
/// ## Collections
///
/// Entities are organized into named collections:
///
/// ```dart
/// final users = db.collection('users');
/// final products = db.collection('products');
/// ```
///
/// ## Basic Operations
///
/// ```dart
/// // Generate a unique ID
/// final id = EntityId.generate();
///
/// // Store data
/// db.put(users, id, Uint8List.fromList([1, 2, 3]));
///
/// // Retrieve data
/// final data = db.get(users, id);
///
/// // Delete
/// db.delete(users, id);
/// ```
///
/// ## Transactions
///
/// For atomic operations, use transactions:
///
/// ```dart
/// db.transaction((txn) {
///   txn.put(users, id1, data1);
///   txn.put(users, id2, data2);
///   // All operations commit atomically
/// });
/// ```
///
/// ## Resource Management
///
/// Always close the database when done:
///
/// ```dart
/// final db = Database.openMemory();
/// try {
///   // ... use database
/// } finally {
///   db.close();
/// }
/// ```
final class Database {
  Pointer<EntiDbHandle>? _handle;
  final Map<String, Collection> _collections = {};

  Database._(this._handle);

  /// Opens a file-based database at the given path.
  ///
  /// The database directory and files will be created if they don't exist
  /// (when [createIfMissing] is true).
  ///
  /// ## Parameters
  ///
  /// - [path]: Path to the database directory
  /// - [maxSegmentSize]: Maximum segment file size in bytes (default: 64MB)
  /// - [syncOnCommit]: Whether to sync to disk on every commit (default: true)
  /// - [createIfMissing]: Create database if it doesn't exist (default: true)
  ///
  /// ## Example
  ///
  /// ```dart
  /// final db = Database.open(
  ///   '/path/to/database',
  ///   maxSegmentSize: 128 * 1024 * 1024,  // 128MB
  ///   syncOnCommit: true,
  /// );
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  factory Database.open(
    String path, {
    int maxSegmentSize = 64 * 1024 * 1024,
    bool syncOnCommit = true,
    bool createIfMissing = true,
  }) {
    final configPtr = EntiDbConfig.allocate(
      path: path,
      maxSegmentSize: maxSegmentSize,
      syncOnCommit: syncOnCommit,
      createIfMissing: createIfMissing,
    );

    final handlePtr = calloc<Pointer<EntiDbHandle>>();

    try {
      final result = bindings.entidbOpen(configPtr, handlePtr);
      checkResult(result);

      return Database._(handlePtr.value);
    } finally {
      EntiDbConfig.free(configPtr);
      calloc.free(handlePtr);
    }
  }

  /// Opens an in-memory database.
  ///
  /// In-memory databases are fast but not persistent. Data is lost
  /// when the database is closed.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final db = Database.openMemory();
  /// // ... use database
  /// db.close();
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  factory Database.openMemory() {
    final handlePtr = calloc<Pointer<EntiDbHandle>>();

    try {
      final result = bindings.entidbOpenMemory(handlePtr);
      checkResult(result);

      return Database._(handlePtr.value);
    } finally {
      calloc.free(handlePtr);
    }
  }

  /// The EntiDB library version.
  static String get version {
    final ptr = bindings.entidbVersion();
    return ptr.toDartString();
  }

  /// Whether the database is currently open.
  bool get isOpen => _handle != null;

  /// Gets or creates a collection by name.
  ///
  /// Multiple calls with the same name return the same collection.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final users = db.collection('users');
  /// final products = db.collection('products');
  /// ```
  ///
  /// Throws [EntiDbError] if the database is closed.
  Collection collection(String name) {
    _ensureOpen();

    // Return cached collection if available
    if (_collections.containsKey(name)) {
      return _collections[name]!;
    }

    final namePtr = name.toNativeUtf8();
    final collIdPtr = EntiDbCollectionId.allocate();

    try {
      final result = bindings.entidbCollection(_handle!, namePtr, collIdPtr);
      checkResult(result);

      final collection = Collection.internal(name, collIdPtr.ref.id);
      _collections[name] = collection;
      return collection;
    } finally {
      calloc.free(namePtr);
      calloc.free(collIdPtr);
    }
  }

  /// Stores an entity in a collection.
  ///
  /// If an entity with the same ID exists, it is replaced.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final id = EntityId.generate();
  /// db.put(users, id, Uint8List.fromList([1, 2, 3]));
  /// ```
  ///
  /// For storing multiple entities atomically, use [transaction].
  ///
  /// Throws [EntiDbError] on failure.
  void put(Collection collection, EntityId entityId, Uint8List data) {
    _ensureOpen();

    final entityPtr = entityId.toFfi();
    final dataPtr = calloc<Uint8>(data.length);
    final collId = calloc<EntiDbCollectionId>();

    try {
      collId.ref.id = collection.id;
      for (var i = 0; i < data.length; i++) {
        dataPtr[i] = data[i];
      }

      final result = bindings.entidbPut(
        _handle!,
        collId.ref,
        entityPtr.ref,
        dataPtr,
        data.length,
      );
      checkResult(result);
    } finally {
      calloc.free(entityPtr);
      calloc.free(dataPtr);
      calloc.free(collId);
    }
  }

  /// Retrieves an entity from a collection.
  ///
  /// Returns `null` if the entity doesn't exist.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final data = db.get(users, userId);
  /// if (data != null) {
  ///   print('Found: ${data.length} bytes');
  /// }
  /// ```
  ///
  /// Throws [EntiDbError] on failure (other than not found).
  Uint8List? get(Collection collection, EntityId entityId) {
    _ensureOpen();

    final entityPtr = entityId.toFfi();
    final bufferPtr = calloc<EntiDbBuffer>();
    final collId = calloc<EntiDbCollectionId>();

    try {
      collId.ref.id = collection.id;

      final result = bindings.entidbGet(
        _handle!,
        collId.ref,
        entityPtr.ref,
        bufferPtr,
      );

      if (result == EntiDbResult.notFound) {
        return null;
      }

      checkResult(result);

      final buffer = bufferPtr.ref;
      if (buffer.isNull) {
        return null;
      }

      final data = Uint8List.fromList(buffer.toList());
      bindings.entidbFreeBuffer(buffer);
      return data;
    } finally {
      calloc.free(entityPtr);
      calloc.free(bufferPtr);
      calloc.free(collId);
    }
  }

  /// Deletes an entity from a collection.
  ///
  /// This is a no-op if the entity doesn't exist.
  ///
  /// ## Example
  ///
  /// ```dart
  /// db.delete(users, userId);
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  void delete(Collection collection, EntityId entityId) {
    _ensureOpen();

    final entityPtr = entityId.toFfi();
    final collId = calloc<EntiDbCollectionId>();

    try {
      collId.ref.id = collection.id;

      final result = bindings.entidbDelete(
        _handle!,
        collId.ref,
        entityPtr.ref,
      );
      checkResult(result);
    } finally {
      calloc.free(entityPtr);
      calloc.free(collId);
    }
  }

  /// Lists all entities in a collection.
  ///
  /// Returns a list of (EntityId, data) tuples.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final entities = db.list(users);
  /// for (final (id, data) in entities) {
  ///   print('Entity: $id, ${data.length} bytes');
  /// }
  /// ```
  ///
  /// For large collections, consider using [iter] instead.
  ///
  /// Throws [EntiDbError] on failure.
  List<(EntityId, Uint8List)> list(Collection collection) {
    final iterator = iter(collection);
    try {
      return iterator.toList();
    } finally {
      iterator.dispose();
    }
  }

  /// Returns the number of entities in a collection.
  ///
  /// ## Example
  ///
  /// ```dart
  /// print('Users: ${db.count(users)}');
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  int count(Collection collection) {
    _ensureOpen();

    final collId = calloc<EntiDbCollectionId>();
    final countPtr = calloc<IntPtr>();

    try {
      collId.ref.id = collection.id;

      final result = bindings.entidbCount(_handle!, collId.ref, countPtr);
      checkResult(result);

      return countPtr.value;
    } finally {
      calloc.free(collId);
      calloc.free(countPtr);
    }
  }

  /// Creates an iterator over a collection.
  ///
  /// Use this for efficient iteration over large collections.
  ///
  /// **Important**: Always call [EntityIterator.dispose] when done.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final iter = db.iter(users);
  /// try {
  ///   while (iter.moveNext()) {
  ///     print('ID: ${iter.currentId}');
  ///   }
  /// } finally {
  ///   iter.dispose();
  /// }
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  EntityIterator iter(Collection collection) {
    _ensureOpen();

    final collId = calloc<EntiDbCollectionId>();
    final iterPtr = calloc<Pointer<EntiDbIterator>>();

    try {
      collId.ref.id = collection.id;

      final result = bindings.entidbIterCreate(_handle!, collId.ref, iterPtr);
      checkResult(result);

      return EntityIterator.internal(iterPtr.value);
    } finally {
      calloc.free(collId);
      calloc.free(iterPtr);
    }
  }

  /// Executes a function within a transaction.
  ///
  /// All operations in the callback are atomic - they all succeed or all fail.
  /// If an exception is thrown, the transaction is automatically rolled back.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final result = db.transaction((txn) {
  ///   txn.put(users, id1, data1);
  ///   txn.put(users, id2, data2);
  ///   return 'success';
  /// });
  /// ```
  ///
  /// ## Rollback on Error
  ///
  /// ```dart
  /// try {
  ///   db.transaction((txn) {
  ///     txn.put(users, id, data);
  ///     throw Exception('Abort!');
  ///   });
  /// } catch (e) {
  ///   // Transaction was rolled back
  /// }
  /// ```
  ///
  /// Returns the callback's return value.
  ///
  /// Throws [EntiDbError] on transaction failure.
  T transaction<T>(T Function(Transaction txn) fn) {
    _ensureOpen();

    final txnPtr = calloc<Pointer<EntiDbTransaction>>();

    try {
      final beginResult = bindings.entidbTxnBegin(_handle!, txnPtr);
      checkResult(beginResult);

      final txn = Transaction.internal(_handle!, txnPtr.value);

      try {
        final result = fn(txn);

        if (txn.isActive) {
          txn.commit();
        }

        return result;
      } catch (e) {
        txn.abort();
        rethrow;
      }
    } finally {
      calloc.free(txnPtr);
    }
  }

  /// Closes the database and releases all resources.
  ///
  /// After calling this method, the database cannot be used.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final db = Database.openMemory();
  /// try {
  ///   // ... use database
  /// } finally {
  ///   db.close();
  /// }
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  void close() {
    if (_handle == null) return;

    final result = bindings.entidbClose(_handle!);
    _handle = null;
    _collections.clear();
    checkResult(result);
  }

  void _ensureOpen() {
    if (_handle == null) {
      throw const EntiDbInvalidError('Database is closed');
    }
  }
}
