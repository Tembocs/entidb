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

  // =========================================================================
  // Checkpoint, Compaction, Backup, and Restore
  // =========================================================================

  /// Creates a checkpoint.
  ///
  /// A checkpoint persists all committed data and truncates the WAL.
  /// After a checkpoint:
  /// - All committed transactions are durable in segments
  /// - The WAL is cleared
  /// - The manifest is updated with the checkpoint sequence
  ///
  /// ## Example
  ///
  /// ```dart
  /// db.checkpoint();
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  void checkpoint() {
    _ensureOpen();

    final result = bindings.entidbCheckpoint(_handle!);
    checkResult(result);
  }

  /// Compacts the database, removing obsolete versions and optionally tombstones.
  ///
  /// Compaction:
  /// - Removes obsolete entity versions (keeping only the latest)
  /// - Optionally removes tombstones (deleted entities)
  /// - Reclaims storage space
  ///
  /// ## Parameters
  ///
  /// - [removeTombstones]: If true, tombstones are removed; if false, preserved.
  ///
  /// ## Returns
  ///
  /// A [CompactionStats] object with information about the compaction.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final stats = db.compact(removeTombstones: true);
  /// print('Removed ${stats.obsoleteVersionsRemoved} obsolete versions');
  /// print('Saved ${stats.bytesSaved} bytes');
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  CompactionStats compact({bool removeTombstones = false}) {
    _ensureOpen();

    final statsPtr = EntiDbCompactionStats.allocate();

    try {
      final result =
          bindings.entidbCompact(_handle!, removeTombstones, statsPtr);
      checkResult(result);

      return CompactionStats._(statsPtr.ref);
    } finally {
      calloc.free(statsPtr);
    }
  }

  /// Returns a snapshot of database statistics.
  ///
  /// Statistics include counts of reads, writes, transactions, and other
  /// operations. This is useful for monitoring and diagnostics.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final stats = db.stats();
  /// print('Reads: ${stats.reads}, Writes: ${stats.writes}');
  /// print('Transactions committed: ${stats.transactionsCommitted}');
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  DatabaseStats stats() {
    _ensureOpen();

    final statsPtr = EntiDbStats.allocate();

    try {
      final result = bindings.entidbStats(_handle!, statsPtr);
      checkResult(result);

      return DatabaseStats._(statsPtr.ref);
    } finally {
      calloc.free(statsPtr);
    }
  }

  /// Creates a backup of the database.
  ///
  /// Returns the backup data as bytes that can be saved to a file.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final backupData = db.backup();
  /// File('backup.endb').writeAsBytesSync(backupData);
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  Uint8List backup() {
    _ensureOpen();

    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      final result = bindings.entidbBackup(_handle!, bufferPtr);
      checkResult(result);

      final buffer = bufferPtr.ref;
      if (buffer.isNull) {
        return Uint8List(0);
      }

      final data = Uint8List.fromList(buffer.toList());
      bindings.entidbFreeBuffer(buffer);
      return data;
    } finally {
      calloc.free(bufferPtr);
    }
  }

  /// Creates a backup with custom options.
  ///
  /// ## Parameters
  ///
  /// - [includeTombstones]: Whether to include deleted entities in the backup.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final backupData = db.backupWithOptions(includeTombstones: true);
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  Uint8List backupWithOptions({bool includeTombstones = false}) {
    _ensureOpen();

    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      final result = bindings.entidbBackupWithOptions(
        _handle!,
        includeTombstones,
        bufferPtr,
      );
      checkResult(result);

      final buffer = bufferPtr.ref;
      if (buffer.isNull) {
        return Uint8List(0);
      }

      final data = Uint8List.fromList(buffer.toList());
      bindings.entidbFreeBuffer(buffer);
      return data;
    } finally {
      calloc.free(bufferPtr);
    }
  }

  /// Restores entities from a backup into this database.
  ///
  /// This merges the backup data into the current database.
  /// Existing entities with the same ID will be overwritten.
  ///
  /// ## Parameters
  ///
  /// - [backupData]: The backup data bytes.
  ///
  /// ## Returns
  ///
  /// A [RestoreStats] object with information about the restore operation.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final backupData = File('backup.endb').readAsBytesSync();
  /// final stats = db.restore(backupData);
  /// print('Restored ${stats.entitiesRestored} entities');
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  RestoreStats restore(Uint8List backupData) {
    _ensureOpen();

    final dataPtr = calloc<Uint8>(backupData.length);
    final statsPtr = EntiDbRestoreStats.allocate();

    try {
      for (var i = 0; i < backupData.length; i++) {
        dataPtr[i] = backupData[i];
      }

      final result = bindings.entidbRestore(
        _handle!,
        dataPtr,
        backupData.length,
        statsPtr,
      );
      checkResult(result);

      return RestoreStats._(statsPtr.ref);
    } finally {
      calloc.free(dataPtr);
      calloc.free(statsPtr);
    }
  }

  /// Validates a backup without restoring it.
  ///
  /// Returns the backup metadata if valid.
  ///
  /// ## Parameters
  ///
  /// - [backupData]: The backup data bytes.
  ///
  /// ## Returns
  ///
  /// A [BackupInfo] object with metadata about the backup.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final info = db.validateBackup(backupData);
  /// if (info.valid) {
  ///   print('Backup has ${info.recordCount} records');
  /// }
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  BackupInfo validateBackup(Uint8List backupData) {
    _ensureOpen();

    final dataPtr = calloc<Uint8>(backupData.length);
    final infoPtr = EntiDbBackupInfo.allocate();

    try {
      for (var i = 0; i < backupData.length; i++) {
        dataPtr[i] = backupData[i];
      }

      final result = bindings.entidbValidateBackup(
        _handle!,
        dataPtr,
        backupData.length,
        infoPtr,
      );
      checkResult(result);

      return BackupInfo._(infoPtr.ref);
    } finally {
      calloc.free(dataPtr);
      calloc.free(infoPtr);
    }
  }

  /// Returns the current committed sequence number.
  ///
  /// ## Example
  ///
  /// ```dart
  /// print('Committed seq: ${db.committedSeq}');
  /// ```
  int get committedSeq {
    _ensureOpen();

    final seqPtr = calloc<Uint64>();

    try {
      final result = bindings.entidbCommittedSeq(_handle!, seqPtr);
      checkResult(result);
      return seqPtr.value;
    } finally {
      calloc.free(seqPtr);
    }
  }

  /// Returns the total entity count.
  ///
  /// ## Example
  ///
  /// ```dart
  /// print('Total entities: ${db.entityCount}');
  /// ```
  int get entityCount {
    _ensureOpen();

    final countPtr = calloc<IntPtr>();

    try {
      final result = bindings.entidbEntityCount(_handle!, countPtr);
      checkResult(result);
      return countPtr.value;
    } finally {
      calloc.free(countPtr);
    }
  }

  // ==========================================================================
  // Index Management
  // ==========================================================================

  /// Creates a hash index for O(1) equality lookups.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to create the index on.
  /// - [name]: The index name.
  /// - [unique]: Whether the index enforces unique keys.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final users = db.collection('users');
  /// db.createHashIndex(users, 'email', unique: true);
  /// ```
  void createHashIndex(Collection collection, String name,
      {bool unique = false}) {
    _ensureOpen();

    final namePtr = name.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result =
          bindings.entidbCreateHashIndex(_handle!, collId.ref, namePtr, unique);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Creates a BTree index for ordered and range lookups.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to create the index on.
  /// - [name]: The index name.
  /// - [unique]: Whether the index enforces unique keys.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final users = db.collection('users');
  /// db.createBTreeIndex(users, 'age', unique: false);
  /// ```
  void createBTreeIndex(Collection collection, String name,
      {bool unique = false}) {
    _ensureOpen();

    final namePtr = name.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result = bindings.entidbCreateBTreeIndex(
          _handle!, collId.ref, namePtr, unique);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Inserts a key-entity pair into a hash index.
  void hashIndexInsert(Collection collection, String indexName, Uint8List key,
      EntityId entityId) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final keyPtr = calloc<Uint8>(key.length);
    final entityIdPtr = EntiDbEntityId.allocate(entityId.bytes);

    try {
      for (var i = 0; i < key.length; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbHashIndexInsert(
          _handle!, collId.ref, namePtr, keyPtr, key.length, entityIdPtr.ref);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(keyPtr);
      calloc.free(entityIdPtr);
    }
  }

  /// Inserts a key-entity pair into a BTree index.
  ///
  /// Note: For proper ordering, use big-endian encoding for numeric keys.
  void btreeIndexInsert(Collection collection, String indexName, Uint8List key,
      EntityId entityId) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final keyPtr = calloc<Uint8>(key.length);
    final entityIdPtr = EntiDbEntityId.allocate(entityId.bytes);

    try {
      for (var i = 0; i < key.length; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbBTreeIndexInsert(
          _handle!, collId.ref, namePtr, keyPtr, key.length, entityIdPtr.ref);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(keyPtr);
      calloc.free(entityIdPtr);
    }
  }

  /// Removes a key-entity pair from a hash index.
  void hashIndexRemove(Collection collection, String indexName, Uint8List key,
      EntityId entityId) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final keyPtr = calloc<Uint8>(key.length);
    final entityIdPtr = EntiDbEntityId.allocate(entityId.bytes);

    try {
      for (var i = 0; i < key.length; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbHashIndexRemove(
          _handle!, collId.ref, namePtr, keyPtr, key.length, entityIdPtr.ref);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(keyPtr);
      calloc.free(entityIdPtr);
    }
  }

  /// Removes a key-entity pair from a BTree index.
  void btreeIndexRemove(Collection collection, String indexName, Uint8List key,
      EntityId entityId) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final keyPtr = calloc<Uint8>(key.length);
    final entityIdPtr = EntiDbEntityId.allocate(entityId.bytes);

    try {
      for (var i = 0; i < key.length; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbBTreeIndexRemove(
          _handle!, collId.ref, namePtr, keyPtr, key.length, entityIdPtr.ref);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(keyPtr);
      calloc.free(entityIdPtr);
    }
  }

  /// Looks up entities by key in a hash index.
  ///
  /// Returns a list of EntityIds matching the key.
  List<EntityId> hashIndexLookup(
      Collection collection, String indexName, Uint8List key) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final keyPtr = calloc<Uint8>(key.length);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      for (var i = 0; i < key.length; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbHashIndexLookup(
          _handle!, collId.ref, namePtr, keyPtr, key.length, bufferPtr);
      checkResult(result);

      return _parseEntityIds(bufferPtr);
    } finally {
      bindings.entidbFreeBuffer(bufferPtr.ref);
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(keyPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Looks up entities by key in a BTree index.
  ///
  /// Returns a list of EntityIds matching the key.
  List<EntityId> btreeIndexLookup(
      Collection collection, String indexName, Uint8List key) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final keyPtr = calloc<Uint8>(key.length);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      for (var i = 0; i < key.length; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbBTreeIndexLookup(
          _handle!, collId.ref, namePtr, keyPtr, key.length, bufferPtr);
      checkResult(result);

      return _parseEntityIds(bufferPtr);
    } finally {
      bindings.entidbFreeBuffer(bufferPtr.ref);
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(keyPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Performs a range query on a BTree index.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection the index belongs to.
  /// - [indexName]: The name of the index.
  /// - [minKey]: Optional minimum key (inclusive). Null for unbounded.
  /// - [maxKey]: Optional maximum key (inclusive). Null for unbounded.
  ///
  /// Returns a list of EntityIds in the range.
  List<EntityId> btreeIndexRange(
    Collection collection,
    String indexName, {
    Uint8List? minKey,
    Uint8List? maxKey,
  }) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final bufferPtr = calloc<EntiDbBuffer>();

    Pointer<Uint8> minKeyPtr = nullptr;
    Pointer<Uint8> maxKeyPtr = nullptr;
    int minKeyLen = 0;
    int maxKeyLen = 0;

    try {
      if (minKey != null) {
        minKeyPtr = calloc<Uint8>(minKey.length);
        minKeyLen = minKey.length;
        for (var i = 0; i < minKey.length; i++) {
          minKeyPtr[i] = minKey[i];
        }
      }

      if (maxKey != null) {
        maxKeyPtr = calloc<Uint8>(maxKey.length);
        maxKeyLen = maxKey.length;
        for (var i = 0; i < maxKey.length; i++) {
          maxKeyPtr[i] = maxKey[i];
        }
      }

      final result = bindings.entidbBTreeIndexRange(
        _handle!,
        collId.ref,
        namePtr,
        minKeyPtr,
        minKeyLen,
        maxKeyPtr,
        maxKeyLen,
        bufferPtr,
      );
      checkResult(result);

      return _parseEntityIds(bufferPtr);
    } finally {
      bindings.entidbFreeBuffer(bufferPtr.ref);
      calloc.free(namePtr);
      calloc.free(collId);
      if (minKeyPtr != nullptr) calloc.free(minKeyPtr);
      if (maxKeyPtr != nullptr) calloc.free(maxKeyPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Returns the number of entries in a hash index.
  int hashIndexLen(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final countPtr = calloc<IntPtr>();

    try {
      final result =
          bindings.entidbHashIndexLen(_handle!, collId.ref, namePtr, countPtr);
      checkResult(result);
      return countPtr.value;
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(countPtr);
    }
  }

  /// Returns the number of entries in a BTree index.
  int btreeIndexLen(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final countPtr = calloc<IntPtr>();

    try {
      final result =
          bindings.entidbBTreeIndexLen(_handle!, collId.ref, namePtr, countPtr);
      checkResult(result);
      return countPtr.value;
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(countPtr);
    }
  }

  /// Drops a hash index.
  ///
  /// Throws if the index doesn't exist after the call (for safety).
  void dropHashIndex(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result =
          bindings.entidbDropHashIndex(_handle!, collId.ref, namePtr);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Drops a BTree index.
  ///
  /// Throws if the index doesn't exist after the call (for safety).
  void dropBTreeIndex(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result =
          bindings.entidbDropBTreeIndex(_handle!, collId.ref, namePtr);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  // ==========================================================================
  // Full-Text Search (FTS) Index Operations
  // ==========================================================================

  /// Creates an FTS (Full-Text Search) index with default configuration.
  ///
  /// FTS indexes allow you to search text content for matching terms.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to create the index on.
  /// - [name]: The index name.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final docs = db.collection('documents');
  /// db.createFtsIndex(docs, 'content');
  /// ```
  void createFtsIndex(Collection collection, String name) {
    _ensureOpen();

    final namePtr = name.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result =
          bindings.entidbCreateFtsIndex(_handle!, collId.ref, namePtr);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Creates an FTS index with custom configuration.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to create the index on.
  /// - [name]: The index name.
  /// - [minTokenLength]: Minimum token length (shorter tokens are ignored).
  /// - [maxTokenLength]: Maximum token length (longer tokens are truncated).
  /// - [caseSensitive]: If true, searches are case-sensitive.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final docs = db.collection('documents');
  /// // Case-sensitive search, ignore words shorter than 2 chars
  /// db.createFtsIndexWithConfig(docs, 'content',
  ///     minTokenLength: 2, caseSensitive: true);
  /// ```
  void createFtsIndexWithConfig(
    Collection collection,
    String name, {
    int minTokenLength = 1,
    int maxTokenLength = 256,
    bool caseSensitive = false,
  }) {
    _ensureOpen();

    final namePtr = name.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result = bindings.entidbCreateFtsIndexWithConfig(
        _handle!,
        collId.ref,
        namePtr,
        minTokenLength,
        maxTokenLength,
        caseSensitive,
      );
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Indexes text content for an entity.
  ///
  /// This extracts tokens from the text and associates them with the entity.
  /// If the entity was previously indexed, the old text is replaced.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection containing the entity.
  /// - [indexName]: The name of the FTS index.
  /// - [entityId]: The entity ID to associate with the text.
  /// - [text]: The text content to index.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final docs = db.collection('documents');
  /// db.createFtsIndex(docs, 'content');
  /// db.ftsIndexText(docs, 'content', docId, 'Hello world from Rust');
  /// ```
  void ftsIndexText(
    Collection collection,
    String indexName,
    EntityId entityId,
    String text,
  ) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final textPtr = text.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final entityIdPtr = EntiDbEntityId.allocate(entityId.bytes);

    try {
      final result = bindings.entidbFtsIndexText(
        _handle!,
        collId.ref,
        namePtr,
        entityIdPtr.ref,
        textPtr,
      );
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(textPtr);
      calloc.free(collId);
      calloc.free(entityIdPtr);
    }
  }

  /// Removes an entity from an FTS index.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection containing the entity.
  /// - [indexName]: The name of the FTS index.
  /// - [entityId]: The entity ID to remove.
  void ftsRemoveEntity(
    Collection collection,
    String indexName,
    EntityId entityId,
  ) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final entityIdPtr = EntiDbEntityId.allocate(entityId.bytes);

    try {
      final result = bindings.entidbFtsRemoveEntity(
        _handle!,
        collId.ref,
        namePtr,
        entityIdPtr.ref,
      );
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(entityIdPtr);
    }
  }

  /// Searches an FTS index using AND semantics.
  ///
  /// All query terms must match for an entity to be returned.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to search.
  /// - [indexName]: The name of the FTS index.
  /// - [query]: Space-separated search terms.
  ///
  /// ## Returns
  ///
  /// A list of entity IDs that match all terms in the query.
  ///
  /// ## Example
  ///
  /// ```dart
  /// // Find documents containing both "hello" AND "world"
  /// final results = db.ftsSearch(docs, 'content', 'hello world');
  /// ```
  List<EntityId> ftsSearch(
    Collection collection,
    String indexName,
    String query,
  ) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final queryPtr = query.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      final result = bindings.entidbFtsSearch(
        _handle!,
        collId.ref,
        namePtr,
        queryPtr,
        bufferPtr,
      );
      checkResult(result);

      return _parseEntityIds(bufferPtr);
    } finally {
      if (bufferPtr.ref.data != nullptr) {
        bindings.entidbFreeBuffer(bufferPtr.ref);
      }
      calloc.free(namePtr);
      calloc.free(queryPtr);
      calloc.free(collId);
      calloc.free(bufferPtr);
    }
  }

  /// Searches an FTS index using OR semantics.
  ///
  /// Any query term may match for an entity to be returned.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to search.
  /// - [indexName]: The name of the FTS index.
  /// - [query]: Space-separated search terms.
  ///
  /// ## Returns
  ///
  /// A list of entity IDs that match any term in the query.
  ///
  /// ## Example
  ///
  /// ```dart
  /// // Find documents containing "hello" OR "world" (or both)
  /// final results = db.ftsSearchAny(docs, 'content', 'hello world');
  /// ```
  List<EntityId> ftsSearchAny(
    Collection collection,
    String indexName,
    String query,
  ) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final queryPtr = query.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      final result = bindings.entidbFtsSearchAny(
        _handle!,
        collId.ref,
        namePtr,
        queryPtr,
        bufferPtr,
      );
      checkResult(result);

      return _parseEntityIds(bufferPtr);
    } finally {
      if (bufferPtr.ref.data != nullptr) {
        bindings.entidbFreeBuffer(bufferPtr.ref);
      }
      calloc.free(namePtr);
      calloc.free(queryPtr);
      calloc.free(collId);
      calloc.free(bufferPtr);
    }
  }

  /// Searches an FTS index using prefix matching.
  ///
  /// Returns entities containing tokens that start with the given prefix.
  ///
  /// ## Parameters
  ///
  /// - [collection]: The collection to search.
  /// - [indexName]: The name of the FTS index.
  /// - [prefix]: The prefix to search for.
  ///
  /// ## Returns
  ///
  /// A list of entity IDs with tokens starting with the prefix.
  ///
  /// ## Example
  ///
  /// ```dart
  /// // Find documents with words starting with "prog" (program, programming, etc.)
  /// final results = db.ftsSearchPrefix(docs, 'content', 'prog');
  /// ```
  List<EntityId> ftsSearchPrefix(
    Collection collection,
    String indexName,
    String prefix,
  ) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final prefixPtr = prefix.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      final result = bindings.entidbFtsSearchPrefix(
        _handle!,
        collId.ref,
        namePtr,
        prefixPtr,
        bufferPtr,
      );
      checkResult(result);

      return _parseEntityIds(bufferPtr);
    } finally {
      if (bufferPtr.ref.data != nullptr) {
        bindings.entidbFreeBuffer(bufferPtr.ref);
      }
      calloc.free(namePtr);
      calloc.free(prefixPtr);
      calloc.free(collId);
      calloc.free(bufferPtr);
    }
  }

  /// Gets the number of entities indexed in an FTS index.
  int ftsIndexLen(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final countPtr = calloc<IntPtr>();

    try {
      final result = bindings.entidbFtsIndexLen(
        _handle!,
        collId.ref,
        namePtr,
        countPtr,
      );
      checkResult(result);

      return countPtr.value;
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(countPtr);
    }
  }

  /// Gets the number of unique tokens in an FTS index.
  int ftsUniqueTokenCount(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);
    final countPtr = calloc<IntPtr>();

    try {
      final result = bindings.entidbFtsUniqueTokenCount(
        _handle!,
        collId.ref,
        namePtr,
        countPtr,
      );
      checkResult(result);

      return countPtr.value;
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
      calloc.free(countPtr);
    }
  }

  /// Clears all entries from an FTS index.
  ///
  /// The index structure remains but all indexed content is removed.
  void ftsClear(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result = bindings.entidbFtsClear(_handle!, collId.ref, namePtr);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Drops an FTS index.
  void dropFtsIndex(Collection collection, String indexName) {
    _ensureOpen();

    final namePtr = indexName.toNativeUtf8();
    final collId = EntiDbCollectionId.allocate(collection.id);

    try {
      final result = bindings.entidbDropFtsIndex(_handle!, collId.ref, namePtr);
      checkResult(result);
    } finally {
      calloc.free(namePtr);
      calloc.free(collId);
    }
  }

  /// Helper to parse entity IDs from a buffer (16 bytes each).
  List<EntityId> _parseEntityIds(Pointer<EntiDbBuffer> bufferPtr) {
    final length = bufferPtr.ref.len;
    if (length == 0) return [];

    final count = length ~/ 16;
    final result = <EntityId>[];

    for (var i = 0; i < count; i++) {
      final bytes = <int>[];
      for (var j = 0; j < 16; j++) {
        bytes.add(bufferPtr.ref.data[i * 16 + j]);
      }
      result.add(EntityId.fromBytes(Uint8List.fromList(bytes)));
    }

    return result;
  }

  // ==========================================================================
  // Change Feed
  // ==========================================================================

  /// Polls for changes since the given cursor.
  ///
  /// Returns a list of [ChangeEvent]s that occurred after the cursor position.
  /// Use [latestSequence] to get the current cursor position.
  ///
  /// ## Parameters
  ///
  /// - [cursor]: The sequence number to start from (exclusive).
  /// - [limit]: Maximum number of events to return (default 100).
  ///
  /// ## Example
  ///
  /// ```dart
  /// var cursor = 0;
  /// final events = db.pollChanges(cursor, limit: 50);
  /// for (final event in events) {
  ///   print('Change: ${event.changeType} on ${event.collectionId}');
  ///   cursor = event.sequence;
  /// }
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  List<ChangeEvent> pollChanges(int cursor, {int limit = 100}) {
    _ensureOpen();

    final eventsPtr = calloc<EntiDbChangeEventList>();

    try {
      final result =
          bindings.entidbPollChanges(_handle!, cursor, limit, eventsPtr);
      checkResult(result);

      final eventList = eventsPtr.ref;
      final events = <ChangeEvent>[];

      for (var i = 0; i < eventList.count; i++) {
        events.add(ChangeEvent._(eventList[i]));
      }

      bindings.entidbFreeChangeEvents(eventList);
      return events;
    } finally {
      calloc.free(eventsPtr);
    }
  }

  /// Returns the latest sequence number in the change feed.
  ///
  /// This can be used as a starting cursor for [pollChanges].
  ///
  /// ## Example
  ///
  /// ```dart
  /// final latest = db.latestSequence;
  /// print('Latest sequence: $latest');
  /// ```
  int get latestSequence {
    _ensureOpen();

    final seqPtr = calloc<Uint64>();

    try {
      final result = bindings.entidbLatestSequence(_handle!, seqPtr);
      checkResult(result);
      return seqPtr.value;
    } finally {
      calloc.free(seqPtr);
    }
  }

  // ==========================================================================
  // Schema Version
  // ==========================================================================

  /// Gets the current schema version of the database.
  ///
  /// The schema version is user-managed and can be used for migrations.
  /// Returns 0 if no schema version has been set.
  ///
  /// ## Example
  ///
  /// ```dart
  /// final version = db.schemaVersion;
  /// if (version < 2) {
  ///   // Run migration
  ///   db.schemaVersion = 2;
  /// }
  /// ```
  int get schemaVersion {
    _ensureOpen();

    final versionPtr = calloc<Uint64>();

    try {
      final result = bindings.entidbGetSchemaVersion(_handle!, versionPtr);
      checkResult(result);
      return versionPtr.value;
    } finally {
      calloc.free(versionPtr);
    }
  }

  /// Sets the schema version of the database.
  ///
  /// The schema version is user-managed and can be used for migrations.
  ///
  /// ## Example
  ///
  /// ```dart
  /// db.schemaVersion = 2;
  /// ```
  ///
  /// Throws [EntiDbError] on failure.
  set schemaVersion(int version) {
    _ensureOpen();

    final result = bindings.entidbSetSchemaVersion(_handle!, version);
    checkResult(result);
  }
}

/// Statistics from a restore operation.
final class RestoreStats {
  /// Number of entities restored.
  final int entitiesRestored;

  /// Number of tombstones (deletions) applied.
  final int tombstonesApplied;

  /// Timestamp when the backup was created (Unix millis).
  final int backupTimestamp;

  /// Sequence number at the time of backup.
  final int backupSequence;

  RestoreStats._(EntiDbRestoreStats ref)
      : entitiesRestored = ref.entitiesRestored,
        tombstonesApplied = ref.tombstonesApplied,
        backupTimestamp = ref.backupTimestamp,
        backupSequence = ref.backupSequence;

  @override
  String toString() =>
      'RestoreStats(entitiesRestored: $entitiesRestored, tombstonesApplied: $tombstonesApplied, backupTimestamp: $backupTimestamp, backupSequence: $backupSequence)';
}

/// Information about a backup.
final class BackupInfo {
  /// Whether the backup checksum is valid.
  final bool valid;

  /// Timestamp when the backup was created (Unix millis).
  final int timestamp;

  /// Sequence number at the time of backup.
  final int sequence;

  /// Number of records in the backup.
  final int recordCount;

  /// Size of the backup in bytes.
  final int size;

  BackupInfo._(EntiDbBackupInfo ref)
      : valid = ref.valid,
        timestamp = ref.timestamp,
        sequence = ref.sequence,
        recordCount = ref.recordCount,
        size = ref.size;

  @override
  String toString() =>
      'BackupInfo(valid: $valid, timestamp: $timestamp, sequence: $sequence, recordCount: $recordCount, size: $size)';
}

/// Statistics from a compaction operation.
///
/// Contains information about what was removed during compaction.
final class CompactionStats {
  /// Number of records in the input before compaction.
  final int inputRecords;

  /// Number of records in the output after compaction.
  final int outputRecords;

  /// Number of tombstones (deleted entities) removed.
  final int tombstonesRemoved;

  /// Number of obsolete entity versions removed.
  final int obsoleteVersionsRemoved;

  /// Estimated bytes saved by compaction.
  final int bytesSaved;

  CompactionStats._(EntiDbCompactionStats ref)
      : inputRecords = ref.inputRecords,
        outputRecords = ref.outputRecords,
        tombstonesRemoved = ref.tombstonesRemoved,
        obsoleteVersionsRemoved = ref.obsoleteVersionsRemoved,
        bytesSaved = ref.bytesSaved;

  @override
  String toString() =>
      'CompactionStats(inputRecords: $inputRecords, outputRecords: $outputRecords, '
      'tombstonesRemoved: $tombstonesRemoved, obsoleteVersionsRemoved: $obsoleteVersionsRemoved, '
      'bytesSaved: $bytesSaved)';
}

/// A snapshot of database statistics.
///
/// Contains counters for various database operations, useful for
/// monitoring and diagnostics.
final class DatabaseStats {
  /// Number of entity read operations.
  final int reads;

  /// Number of entity write operations (put).
  final int writes;

  /// Number of entity delete operations.
  final int deletes;

  /// Number of full collection scans.
  final int scans;

  /// Number of index lookup operations.
  final int indexLookups;

  /// Number of transactions started.
  final int transactionsStarted;

  /// Number of transactions committed.
  final int transactionsCommitted;

  /// Number of transactions aborted.
  final int transactionsAborted;

  /// Total bytes read from entities.
  final int bytesRead;

  /// Total bytes written to entities.
  final int bytesWritten;

  /// Number of checkpoints performed.
  final int checkpoints;

  /// Number of errors recorded.
  final int errors;

  /// Total entity count (as of last update).
  final int entityCount;

  DatabaseStats._(EntiDbStats ref)
      : reads = ref.reads,
        writes = ref.writes,
        deletes = ref.deletes,
        scans = ref.scans,
        indexLookups = ref.indexLookups,
        transactionsStarted = ref.transactionsStarted,
        transactionsCommitted = ref.transactionsCommitted,
        transactionsAborted = ref.transactionsAborted,
        bytesRead = ref.bytesRead,
        bytesWritten = ref.bytesWritten,
        checkpoints = ref.checkpoints,
        errors = ref.errors,
        entityCount = ref.entityCount;

  @override
  String toString() =>
      'DatabaseStats(reads: $reads, writes: $writes, deletes: $deletes, '
      'scans: $scans, indexLookups: $indexLookups, '
      'transactionsStarted: $transactionsStarted, transactionsCommitted: $transactionsCommitted, '
      'transactionsAborted: $transactionsAborted, bytesRead: $bytesRead, bytesWritten: $bytesWritten, '
      'checkpoints: $checkpoints, errors: $errors, entityCount: $entityCount)';
}

/// The type of a change event.
enum ChangeType {
  /// An entity was inserted.
  insert,

  /// An entity was updated.
  update,

  /// An entity was deleted.
  delete,
}

/// Represents a change event from the change feed.
///
/// Change events are emitted when entities are created, updated, or deleted.
final class ChangeEvent {
  /// The sequence number of this change.
  final int sequence;

  /// The collection ID where the change occurred.
  final int collectionId;

  /// The entity ID that was changed.
  final EntityId entityId;

  /// The type of change (insert, update, or delete).
  final ChangeType changeType;

  /// The payload bytes (CBOR-encoded entity data).
  /// Empty for delete events.
  final Uint8List payload;

  ChangeEvent._(EntiDbChangeEvent ref)
      : sequence = ref.sequence,
        collectionId = ref.collectionId,
        entityId = EntityId.fromBytes(
            Uint8List.fromList(List.generate(16, (i) => ref.entityId[i]))),
        changeType = _parseChangeType(ref.changeType),
        payload = ref.payloadLen > 0
            ? Uint8List.fromList(
                List.generate(ref.payloadLen, (i) => ref.payload[i]))
            : Uint8List(0);

  static ChangeType _parseChangeType(int type) {
    switch (type) {
      case EntiDbChangeType.insert:
        return ChangeType.insert;
      case EntiDbChangeType.update:
        return ChangeType.update;
      case EntiDbChangeType.delete:
        return ChangeType.delete;
      default:
        return ChangeType.update;
    }
  }

  @override
  String toString() =>
      'ChangeEvent(sequence: $sequence, collectionId: $collectionId, '
      'entityId: $entityId, changeType: $changeType, payloadLen: ${payload.length})';
}
