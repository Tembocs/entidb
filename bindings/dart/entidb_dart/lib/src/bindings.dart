// ignore_for_file: always_specify_types, avoid_private_typedef_functions

/// FFI bindings for the EntiDB native library.
///
/// This file provides the low-level FFI interface to the Rust core.
/// Application code should use the high-level API in other files.
library;

import 'dart:ffi';
import 'dart:io' show Platform;
import 'package:ffi/ffi.dart';

// ============================================================================
// Type Definitions
// ============================================================================

/// Opaque database handle.
final class EntiDbHandle extends Opaque {}

/// Opaque transaction handle.
final class EntiDbTransaction extends Opaque {}

/// Opaque iterator handle.
final class EntiDbIterator extends Opaque {}

/// Entity ID as a 16-byte structure.
final class EntiDbEntityId extends Struct {
  @Array(16)
  external Array<Uint8> bytes;

  /// Creates an EntiDbEntityId from a list of bytes.
  static Pointer<EntiDbEntityId> allocate(List<int> data) {
    assert(data.length == 16, 'EntityId must be exactly 16 bytes');
    final ptr = calloc<EntiDbEntityId>();
    for (var i = 0; i < 16; i++) {
      ptr.ref.bytes[i] = data[i];
    }
    return ptr;
  }

  /// Copies bytes to a Dart list.
  List<int> toList() {
    final result = <int>[];
    for (var i = 0; i < 16; i++) {
      result.add(bytes[i]);
    }
    return result;
  }
}

/// Collection ID.
final class EntiDbCollectionId extends Struct {
  @Uint32()
  external int id;

  /// Creates a collection ID pointer.
  static Pointer<EntiDbCollectionId> allocate([int id = 0]) {
    final ptr = calloc<EntiDbCollectionId>();
    ptr.ref.id = id;
    return ptr;
  }
}

/// Database configuration.
final class EntiDbConfig extends Struct {
  external Pointer<Utf8> path;

  @Uint64()
  external int maxSegmentSize;

  @Bool()
  external bool syncOnCommit;

  @Bool()
  external bool createIfMissing;

  /// Creates a config pointer with default values.
  static Pointer<EntiDbConfig> allocate({
    String? path,
    int maxSegmentSize = 64 * 1024 * 1024,
    bool syncOnCommit = true,
    bool createIfMissing = true,
  }) {
    final ptr = calloc<EntiDbConfig>();
    ptr.ref.path = path != null ? path.toNativeUtf8() : nullptr;
    ptr.ref.maxSegmentSize = maxSegmentSize;
    ptr.ref.syncOnCommit = syncOnCommit;
    ptr.ref.createIfMissing = createIfMissing;
    return ptr;
  }

  /// Frees a config pointer.
  static void free(Pointer<EntiDbConfig> ptr) {
    if (ptr.ref.path != nullptr) {
      calloc.free(ptr.ref.path);
    }
    calloc.free(ptr);
  }
}

/// A byte buffer.
final class EntiDbBuffer extends Struct {
  external Pointer<Uint8> data;

  @IntPtr()
  external int len;

  @IntPtr()
  external int capacity;

  /// Returns true if the buffer is null/empty.
  bool get isNull => data == nullptr;

  /// Copies buffer data to a Dart Uint8List.
  List<int> toList() {
    if (data == nullptr || len == 0) return [];
    return data.asTypedList(len).toList();
  }
}

// ============================================================================
// Result Codes
// ============================================================================

/// Result codes returned by FFI functions.
abstract final class EntiDbResult {
  static const int ok = 0;
  static const int error = 1;
  static const int invalidArgument = 2;
  static const int notFound = 3;
  static const int conflict = 4;
  static const int closed = 5;
  static const int locked = 6;
  static const int corruption = 7;
  static const int ioError = 8;
  static const int outOfMemory = 9;
  static const int invalidFormat = 10;
  static const int codecError = 11;
  static const int nullPointer = 12;
}

// ============================================================================
// Function Signatures
// ============================================================================

// Database functions
typedef EntiDbOpenNative = Int32 Function(
  Pointer<EntiDbConfig> config,
  Pointer<Pointer<EntiDbHandle>> outHandle,
);
typedef EntiDbOpenDart = int Function(
  Pointer<EntiDbConfig> config,
  Pointer<Pointer<EntiDbHandle>> outHandle,
);

typedef EntiDbOpenMemoryNative = Int32 Function(
  Pointer<Pointer<EntiDbHandle>> outHandle,
);
typedef EntiDbOpenMemoryDart = int Function(
  Pointer<Pointer<EntiDbHandle>> outHandle,
);

typedef EntiDbCloseNative = Int32 Function(Pointer<EntiDbHandle> handle);
typedef EntiDbCloseDart = int Function(Pointer<EntiDbHandle> handle);

typedef EntiDbCollectionNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  Pointer<Utf8> name,
  Pointer<EntiDbCollectionId> outCollectionId,
);
typedef EntiDbCollectionDart = int Function(
  Pointer<EntiDbHandle> handle,
  Pointer<Utf8> name,
  Pointer<EntiDbCollectionId> outCollectionId,
);

typedef EntiDbPutNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<Uint8> data,
  IntPtr dataLen,
);
typedef EntiDbPutDart = int Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<Uint8> data,
  int dataLen,
);

typedef EntiDbGetNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<EntiDbBuffer> outBuffer,
);
typedef EntiDbGetDart = int Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<EntiDbBuffer> outBuffer,
);

typedef EntiDbDeleteNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
);
typedef EntiDbDeleteDart = int Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
);

typedef EntiDbGenerateIdNative = Int32 Function(
  Pointer<EntiDbEntityId> outId,
);
typedef EntiDbGenerateIdDart = int Function(
  Pointer<EntiDbEntityId> outId,
);

typedef EntiDbVersionNative = Pointer<Utf8> Function();
typedef EntiDbVersionDart = Pointer<Utf8> Function();

// Error functions
typedef EntiDbGetLastErrorNative = Pointer<Utf8> Function();
typedef EntiDbGetLastErrorDart = Pointer<Utf8> Function();

typedef EntiDbClearErrorNative = Void Function();
typedef EntiDbClearErrorDart = void Function();

// Buffer functions
typedef EntiDbFreeBufferNative = Void Function(EntiDbBuffer buffer);
typedef EntiDbFreeBufferDart = void Function(EntiDbBuffer buffer);

// Transaction functions
typedef EntiDbTxnBeginNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  Pointer<Pointer<EntiDbTransaction>> outTxn,
);
typedef EntiDbTxnBeginDart = int Function(
  Pointer<EntiDbHandle> handle,
  Pointer<Pointer<EntiDbTransaction>> outTxn,
);

typedef EntiDbTxnCommitNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  Pointer<EntiDbTransaction> txn,
);
typedef EntiDbTxnCommitDart = int Function(
  Pointer<EntiDbHandle> handle,
  Pointer<EntiDbTransaction> txn,
);

typedef EntiDbTxnAbortNative = Int32 Function(
  Pointer<EntiDbTransaction> txn,
);
typedef EntiDbTxnAbortDart = int Function(
  Pointer<EntiDbTransaction> txn,
);

typedef EntiDbTxnPutNative = Int32 Function(
  Pointer<EntiDbTransaction> txn,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<Uint8> data,
  IntPtr dataLen,
);
typedef EntiDbTxnPutDart = int Function(
  Pointer<EntiDbTransaction> txn,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<Uint8> data,
  int dataLen,
);

typedef EntiDbTxnDeleteNative = Int32 Function(
  Pointer<EntiDbTransaction> txn,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
);
typedef EntiDbTxnDeleteDart = int Function(
  Pointer<EntiDbTransaction> txn,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
);

typedef EntiDbTxnGetNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  Pointer<EntiDbTransaction> txn,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<EntiDbBuffer> outBuffer,
);
typedef EntiDbTxnGetDart = int Function(
  Pointer<EntiDbHandle> handle,
  Pointer<EntiDbTransaction> txn,
  EntiDbCollectionId collectionId,
  EntiDbEntityId entityId,
  Pointer<EntiDbBuffer> outBuffer,
);

// Iterator functions
typedef EntiDbIterCreateNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  Pointer<Pointer<EntiDbIterator>> outIter,
);
typedef EntiDbIterCreateDart = int Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  Pointer<Pointer<EntiDbIterator>> outIter,
);

typedef EntiDbIterHasNextNative = Int32 Function(
  Pointer<EntiDbIterator> iter,
  Pointer<Bool> outHasNext,
);
typedef EntiDbIterHasNextDart = int Function(
  Pointer<EntiDbIterator> iter,
  Pointer<Bool> outHasNext,
);

typedef EntiDbIterNextNative = Int32 Function(
  Pointer<EntiDbIterator> iter,
  Pointer<EntiDbEntityId> outEntityId,
  Pointer<EntiDbBuffer> outBuffer,
);
typedef EntiDbIterNextDart = int Function(
  Pointer<EntiDbIterator> iter,
  Pointer<EntiDbEntityId> outEntityId,
  Pointer<EntiDbBuffer> outBuffer,
);

typedef EntiDbIterRemainingNative = Int32 Function(
  Pointer<EntiDbIterator> iter,
  Pointer<IntPtr> outCount,
);
typedef EntiDbIterRemainingDart = int Function(
  Pointer<EntiDbIterator> iter,
  Pointer<IntPtr> outCount,
);

typedef EntiDbIterFreeNative = Int32 Function(
  Pointer<EntiDbIterator> iter,
);
typedef EntiDbIterFreeDart = int Function(
  Pointer<EntiDbIterator> iter,
);

typedef EntiDbCountNative = Int32 Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  Pointer<IntPtr> outCount,
);
typedef EntiDbCountDart = int Function(
  Pointer<EntiDbHandle> handle,
  EntiDbCollectionId collectionId,
  Pointer<IntPtr> outCount,
);

// ============================================================================
// Library Loading
// ============================================================================

/// Gets the native library name for the current platform.
String _getLibraryName() {
  if (Platform.isWindows) {
    return 'entidb_ffi.dll';
  } else if (Platform.isMacOS) {
    return 'libentidb_ffi.dylib';
  } else if (Platform.isLinux) {
    return 'libentidb_ffi.so';
  } else if (Platform.isAndroid) {
    return 'libentidb_ffi.so';
  } else if (Platform.isIOS) {
    return 'libentidb_ffi.dylib';
  } else {
    throw UnsupportedError('Unsupported platform: ${Platform.operatingSystem}');
  }
}

/// Custom library path override for testing.
String? _customLibraryPath;

/// Sets a custom library path for testing or development.
void setEntiDbLibraryPath(String path) {
  _customLibraryPath = path;
  _bindings = null; // Force reload
}

/// The loaded dynamic library.
DynamicLibrary? _library;

/// Gets the dynamic library, loading it if necessary.
DynamicLibrary get library {
  if (_library != null) return _library!;

  final path = _customLibraryPath ?? _getLibraryName();
  _library = DynamicLibrary.open(path);
  return _library!;
}

// ============================================================================
// Bindings Class
// ============================================================================

/// Container for all FFI function bindings.
class EntiDbBindings {
  final DynamicLibrary _lib;

  EntiDbBindings(this._lib);

  // Database functions
  late final entidbOpen =
      _lib.lookupFunction<EntiDbOpenNative, EntiDbOpenDart>('entidb_open');

  late final entidbOpenMemory =
      _lib.lookupFunction<EntiDbOpenMemoryNative, EntiDbOpenMemoryDart>(
          'entidb_open_memory');

  late final entidbClose =
      _lib.lookupFunction<EntiDbCloseNative, EntiDbCloseDart>('entidb_close');

  late final entidbCollection =
      _lib.lookupFunction<EntiDbCollectionNative, EntiDbCollectionDart>(
          'entidb_collection');

  late final entidbPut =
      _lib.lookupFunction<EntiDbPutNative, EntiDbPutDart>('entidb_put');

  late final entidbGet =
      _lib.lookupFunction<EntiDbGetNative, EntiDbGetDart>('entidb_get');

  late final entidbDelete = _lib
      .lookupFunction<EntiDbDeleteNative, EntiDbDeleteDart>('entidb_delete');

  late final entidbGenerateId =
      _lib.lookupFunction<EntiDbGenerateIdNative, EntiDbGenerateIdDart>(
          'entidb_generate_id');

  late final entidbVersion = _lib
      .lookupFunction<EntiDbVersionNative, EntiDbVersionDart>('entidb_version');

  // Error functions
  late final entidbGetLastError =
      _lib.lookupFunction<EntiDbGetLastErrorNative, EntiDbGetLastErrorDart>(
          'entidb_get_last_error');

  late final entidbClearError =
      _lib.lookupFunction<EntiDbClearErrorNative, EntiDbClearErrorDart>(
          'entidb_clear_error');

  // Buffer functions
  late final entidbFreeBuffer =
      _lib.lookupFunction<EntiDbFreeBufferNative, EntiDbFreeBufferDart>(
          'entidb_free_buffer');

  // Transaction functions
  late final entidbTxnBegin =
      _lib.lookupFunction<EntiDbTxnBeginNative, EntiDbTxnBeginDart>(
          'entidb_txn_begin');

  late final entidbTxnCommit =
      _lib.lookupFunction<EntiDbTxnCommitNative, EntiDbTxnCommitDart>(
          'entidb_txn_commit');

  late final entidbTxnAbort =
      _lib.lookupFunction<EntiDbTxnAbortNative, EntiDbTxnAbortDart>(
          'entidb_txn_abort');

  late final entidbTxnPut = _lib
      .lookupFunction<EntiDbTxnPutNative, EntiDbTxnPutDart>('entidb_txn_put');

  late final entidbTxnDelete =
      _lib.lookupFunction<EntiDbTxnDeleteNative, EntiDbTxnDeleteDart>(
          'entidb_txn_delete');

  late final entidbTxnGet = _lib
      .lookupFunction<EntiDbTxnGetNative, EntiDbTxnGetDart>('entidb_txn_get');

  // Iterator functions
  late final entidbIterCreate =
      _lib.lookupFunction<EntiDbIterCreateNative, EntiDbIterCreateDart>(
          'entidb_iter_create');

  late final entidbIterHasNext =
      _lib.lookupFunction<EntiDbIterHasNextNative, EntiDbIterHasNextDart>(
          'entidb_iter_has_next');

  late final entidbIterNext =
      _lib.lookupFunction<EntiDbIterNextNative, EntiDbIterNextDart>(
          'entidb_iter_next');

  late final entidbIterRemaining =
      _lib.lookupFunction<EntiDbIterRemainingNative, EntiDbIterRemainingDart>(
          'entidb_iter_remaining');

  late final entidbIterFree =
      _lib.lookupFunction<EntiDbIterFreeNative, EntiDbIterFreeDart>(
          'entidb_iter_free');

  late final entidbCount =
      _lib.lookupFunction<EntiDbCountNative, EntiDbCountDart>('entidb_count');
}

/// Cached bindings instance.
EntiDbBindings? _bindings;

/// Gets the bindings, creating them if necessary.
EntiDbBindings get bindings {
  _bindings ??= EntiDbBindings(library);
  return _bindings!;
}
