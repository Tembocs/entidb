/// Entity iterator.
library;

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.dart';
import 'entity_id.dart';
import 'error.dart';

/// An iterator over entities in a collection.
///
/// Iterators provide efficient traversal over collection contents
/// without loading all data into memory at once.
///
/// ## Usage
///
/// ```dart
/// final iter = db.iter(users);
/// try {
///   while (iter.moveNext()) {
///     print('ID: ${iter.currentId}');
///     print('Data: ${iter.currentData.length} bytes');
///   }
/// } finally {
///   iter.dispose();
/// }
/// ```
///
/// **Important**: Always call [dispose] when done to release resources.
final class EntityIterator implements Iterator<(EntityId, Uint8List)> {
  final Pointer<EntiDbIterator> _handle;
  bool _disposed = false;
  EntityId? _currentId;
  Uint8List? _currentData;

  /// Creates a new iterator.
  ///
  /// This is an internal constructor. Use [Database.iter] instead.
  EntityIterator.internal(this._handle);

  /// The current entity's ID.
  ///
  /// Throws [StateError] if [moveNext] hasn't been called or returned false.
  EntityId get currentId {
    if (_currentId == null) {
      throw StateError('No current element');
    }
    return _currentId!;
  }

  /// The current entity's data.
  ///
  /// Throws [StateError] if [moveNext] hasn't been called or returned false.
  Uint8List get currentData {
    if (_currentData == null) {
      throw StateError('No current element');
    }
    return _currentData!;
  }

  @override
  (EntityId, Uint8List) get current {
    if (_currentId == null || _currentData == null) {
      throw StateError('No current element');
    }
    return (_currentId!, _currentData!);
  }

  /// Returns true if there are more entities to iterate.
  bool get hasNext {
    _ensureNotDisposed();

    final hasNextPtr = calloc<Bool>();
    try {
      final result = bindings.entidbIterHasNext(_handle, hasNextPtr);
      checkResult(result);
      return hasNextPtr.value;
    } finally {
      calloc.free(hasNextPtr);
    }
  }

  /// Returns the number of remaining entities.
  int get remaining {
    _ensureNotDisposed();

    final countPtr = calloc<IntPtr>();
    try {
      final result = bindings.entidbIterRemaining(_handle, countPtr);
      checkResult(result);
      return countPtr.value;
    } finally {
      calloc.free(countPtr);
    }
  }

  @override
  bool moveNext() {
    _ensureNotDisposed();

    final entityIdPtr = calloc<EntiDbEntityId>();
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      final result = bindings.entidbIterNext(_handle, entityIdPtr, bufferPtr);

      if (result == EntiDbResult.notFound) {
        _currentId = null;
        _currentData = null;
        return false;
      }

      checkResult(result);

      _currentId = EntityIdFfi.fromFfi(entityIdPtr.ref);

      final buffer = bufferPtr.ref;
      if (buffer.isNull) {
        _currentData = Uint8List(0);
      } else {
        _currentData = Uint8List.fromList(buffer.toList());
        bindings.entidbFreeBuffer(buffer);
      }

      return true;
    } finally {
      calloc.free(entityIdPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Releases iterator resources.
  ///
  /// **Must be called** when done iterating.
  void dispose() {
    if (_disposed) return;
    _disposed = true;

    bindings.entidbIterFree(_handle);
  }

  void _ensureNotDisposed() {
    if (_disposed) {
      throw StateError('Iterator has been disposed');
    }
  }
}

/// Extension methods for EntityIterator to provide Iterable-like behavior.
extension EntityIteratorExtensions on EntityIterator {
  /// Collects all remaining entities into a list.
  ///
  /// This consumes the iterator. Don't call [moveNext] afterwards.
  List<(EntityId, Uint8List)> toList() {
    final result = <(EntityId, Uint8List)>[];
    while (moveNext()) {
      result.add((currentId, currentData));
    }
    return result;
  }

  /// Applies a function to each remaining entity.
  ///
  /// This consumes the iterator.
  void forEach(void Function(EntityId id, Uint8List data) fn) {
    while (moveNext()) {
      fn(currentId, currentData);
    }
  }
}
