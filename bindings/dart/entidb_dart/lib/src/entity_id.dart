/// Entity ID type.
library;

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.dart';
import 'error.dart';

/// A 16-byte unique entity identifier.
///
/// Entity IDs are globally unique within a database and are immutable.
/// Use [EntityId.generate] to create new IDs, or [EntityId.fromBytes]
/// to restore IDs from storage.
///
/// ## Example
///
/// ```dart
/// // Generate a new ID
/// final id = EntityId.generate();
///
/// // Create from raw bytes
/// final id2 = EntityId.fromBytes(Uint8List(16));
///
/// // Get raw bytes
/// final bytes = id.bytes;
///
/// // Compare IDs
/// if (id == id2) {
///   print('Same entity');
/// }
/// ```
final class EntityId implements Comparable<EntityId> {
  /// The raw 16-byte identifier.
  final Uint8List _bytes;

  /// Creates an entity ID from raw bytes.
  ///
  /// Throws [ArgumentError] if [bytes] is not exactly 16 bytes.
  EntityId.fromBytes(Uint8List bytes) : _bytes = Uint8List.fromList(bytes) {
    if (_bytes.length != 16) {
      throw ArgumentError.value(
        bytes.length,
        'bytes',
        'EntityId must be exactly 16 bytes',
      );
    }
  }

  /// Creates an entity ID from a list of integers.
  ///
  /// Throws [ArgumentError] if [bytes] is not exactly 16 bytes.
  factory EntityId.fromList(List<int> bytes) {
    return EntityId.fromBytes(Uint8List.fromList(bytes));
  }

  /// Generates a new unique entity ID.
  ///
  /// Uses UUID v4 internally for guaranteed uniqueness.
  factory EntityId.generate() {
    final ptr = calloc<EntiDbEntityId>();
    try {
      final result = bindings.entidbGenerateId(ptr);
      checkResult(result);
      return EntityId.fromList(ptr.ref.toList());
    } finally {
      calloc.free(ptr);
    }
  }

  /// Creates a zero (null) entity ID.
  ///
  /// This creates an ID filled with zeros, which can be used as a
  /// sentinel value but should not be used for actual entities.
  factory EntityId.zero() {
    return EntityId.fromBytes(Uint8List(16));
  }

  /// The raw 16-byte identifier.
  Uint8List get bytes => Uint8List.fromList(_bytes);

  /// Returns true if this is a zero ID.
  bool get isZero => _bytes.every((b) => b == 0);

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    if (other is! EntityId) return false;
    for (var i = 0; i < 16; i++) {
      if (_bytes[i] != other._bytes[i]) return false;
    }
    return true;
  }

  @override
  int get hashCode {
    var hash = 0;
    for (var i = 0; i < 16; i++) {
      hash = 31 * hash + _bytes[i];
    }
    return hash;
  }

  @override
  int compareTo(EntityId other) {
    for (var i = 0; i < 16; i++) {
      final cmp = _bytes[i].compareTo(other._bytes[i]);
      if (cmp != 0) return cmp;
    }
    return 0;
  }

  /// Converts to a hex string representation.
  String toHexString() {
    return _bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  }

  @override
  String toString() => 'EntityId(${toHexString()})';
}

/// Extension to convert EntityId to/from FFI types.
extension EntityIdFfi on EntityId {
  /// Creates an FFI entity ID pointer.
  ///
  /// The caller is responsible for freeing the pointer.
  Pointer<EntiDbEntityId> toFfi() {
    return EntiDbEntityId.allocate(_bytes);
  }

  /// Copies to an existing FFI struct.
  void copyToFfi(Pointer<EntiDbEntityId> ptr) {
    for (var i = 0; i < 16; i++) {
      ptr.ref.bytes[i] = _bytes[i];
    }
  }

  /// Creates an EntityId from an FFI struct.
  static EntityId fromFfi(EntiDbEntityId ffi) {
    return EntityId.fromList(ffi.toList());
  }
}
