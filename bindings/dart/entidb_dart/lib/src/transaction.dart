/// Transaction support.
library;

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.dart';
import 'collection.dart';
import 'entity_id.dart';
import 'error.dart';

/// A database transaction for atomic operations.
///
/// Transactions provide ACID guarantees:
/// - **Atomicity**: All operations succeed or all fail
/// - **Consistency**: Database remains in a valid state
/// - **Isolation**: Concurrent transactions don't interfere
/// - **Durability**: Committed changes survive crashes
///
/// ## Usage
///
/// Transactions are created via [Database.transaction]:
///
/// ```dart
/// db.transaction((txn) {
///   txn.put(users, id1, data1);
///   txn.put(users, id2, data2);
///   // All operations commit atomically
/// });
/// ```
///
/// If an exception is thrown, the transaction is rolled back:
///
/// ```dart
/// try {
///   db.transaction((txn) {
///     txn.put(users, id, data);
///     throw Exception('Rollback!');
///   });
/// } catch (e) {
///   // Transaction was rolled back
/// }
/// ```
final class Transaction {
  final Pointer<EntiDbHandle> _dbHandle;
  final Pointer<EntiDbTransaction> _txnHandle;
  bool _committed = false;
  bool _aborted = false;

  /// Creates a new transaction.
  ///
  /// This is an internal constructor. Use [Database.transaction] instead.
  Transaction.internal(this._dbHandle, this._txnHandle);

  /// Whether this transaction is still active.
  bool get isActive => !_committed && !_aborted;

  /// Stores an entity within this transaction.
  ///
  /// The entity will be visible to subsequent [get] calls within this
  /// transaction, but won't be visible to other transactions until commit.
  ///
  /// Throws [EntiDbError] on failure.
  void put(Collection collection, EntityId entityId, Uint8List data) {
    _ensureActive();

    final entityPtr = entityId.toFfi();
    final dataPtr = calloc<Uint8>(data.length);

    try {
      for (var i = 0; i < data.length; i++) {
        dataPtr[i] = data[i];
      }

      final collId = calloc<EntiDbCollectionId>();
      collId.ref.id = collection.id;

      try {
        final result = bindings.entidbTxnPut(
          _txnHandle,
          collId.ref,
          entityPtr.ref,
          dataPtr,
          data.length,
        );
        checkResult(result);
      } finally {
        calloc.free(collId);
      }
    } finally {
      calloc.free(entityPtr);
      calloc.free(dataPtr);
    }
  }

  /// Gets an entity within this transaction.
  ///
  /// This sees uncommitted writes from this transaction.
  ///
  /// Returns `null` if the entity doesn't exist or was deleted.
  ///
  /// Throws [EntiDbError] on failure.
  Uint8List? get(Collection collection, EntityId entityId) {
    _ensureActive();

    final entityPtr = entityId.toFfi();
    final bufferPtr = calloc<EntiDbBuffer>();
    final collId = calloc<EntiDbCollectionId>();

    try {
      collId.ref.id = collection.id;

      final result = bindings.entidbTxnGet(
        _dbHandle,
        _txnHandle,
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

  /// Deletes an entity within this transaction.
  ///
  /// The delete will be visible to subsequent [get] calls within this
  /// transaction, but won't affect other transactions until commit.
  ///
  /// Throws [EntiDbError] on failure.
  void delete(Collection collection, EntityId entityId) {
    _ensureActive();

    final entityPtr = entityId.toFfi();
    final collId = calloc<EntiDbCollectionId>();

    try {
      collId.ref.id = collection.id;

      final result = bindings.entidbTxnDelete(
        _txnHandle,
        collId.ref,
        entityPtr.ref,
      );
      checkResult(result);
    } finally {
      calloc.free(entityPtr);
      calloc.free(collId);
    }
  }

  /// Commits the transaction.
  ///
  /// This is called automatically by [Database.transaction] on success.
  void commit() {
    _ensureActive();

    final result = bindings.entidbTxnCommit(_dbHandle, _txnHandle);
    _committed = true;
    checkResult(result);
  }

  /// Aborts the transaction, rolling back all changes.
  ///
  /// This is called automatically by [Database.transaction] on error.
  void abort() {
    if (!isActive) return;

    bindings.entidbTxnAbort(_txnHandle);
    _aborted = true;
  }

  void _ensureActive() {
    if (_committed) {
      throw const EntiDbInvalidError('Transaction already committed');
    }
    if (_aborted) {
      throw const EntiDbInvalidError('Transaction already aborted');
    }
  }
}
