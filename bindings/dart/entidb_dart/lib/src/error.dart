/// Error types for EntiDB.
library;

import 'package:ffi/ffi.dart';

import 'bindings.dart';

/// Base class for all EntiDB errors.
sealed class EntiDbError implements Exception {
  /// The error message.
  final String message;

  /// Creates an EntiDB error.
  const EntiDbError(this.message);

  @override
  String toString() => '$runtimeType: $message';

  /// Creates an appropriate error from a result code.
  static EntiDbError fromResult(int result, [String? message]) {
    final msg = message ?? _getLastError() ?? 'Unknown error';

    return switch (result) {
      EntiDbResult.notFound => EntiDbNotFoundError(msg),
      EntiDbResult.invalidArgument => EntiDbInvalidError(msg),
      EntiDbResult.nullPointer => EntiDbInvalidError(msg),
      EntiDbResult.conflict => EntiDbTransactionError(msg),
      EntiDbResult.closed => EntiDbInvalidError(msg),
      EntiDbResult.locked => EntiDbTransactionError(msg),
      EntiDbResult.corruption => EntiDbCorruptionError(msg),
      EntiDbResult.ioError => EntiDbIoError(msg),
      EntiDbResult.outOfMemory => EntiDbError._generic(msg),
      EntiDbResult.invalidFormat => EntiDbCorruptionError(msg),
      EntiDbResult.codecError => EntiDbInvalidError(msg),
      _ => EntiDbError._generic(msg),
    };
  }

  /// Creates a generic error.
  factory EntiDbError._generic(String message) = _GenericEntiDbError;
}

/// Generic error for unspecified error types.
final class _GenericEntiDbError extends EntiDbError {
  const _GenericEntiDbError(super.message);
}

/// Error thrown when an entity is not found.
final class EntiDbNotFoundError extends EntiDbError {
  /// Creates a not found error.
  const EntiDbNotFoundError(super.message);
}

/// Error thrown for invalid arguments or state.
final class EntiDbInvalidError extends EntiDbError {
  /// Creates an invalid argument error.
  const EntiDbInvalidError(super.message);
}

/// Error thrown for I/O failures.
final class EntiDbIoError extends EntiDbError {
  /// Creates an I/O error.
  const EntiDbIoError(super.message);
}

/// Error thrown when data corruption is detected.
final class EntiDbCorruptionError extends EntiDbError {
  /// Creates a corruption error.
  const EntiDbCorruptionError(super.message);
}

/// Error thrown for transaction failures.
final class EntiDbTransactionError extends EntiDbError {
  /// Creates a transaction error.
  const EntiDbTransactionError(super.message);
}

/// Gets the last error message from the native library.
String? _getLastError() {
  final ptr = bindings.entidbGetLastError();
  if (ptr.address == 0) return null;
  return ptr.toDartString();
}

/// Checks a result code and throws an error if it indicates failure.
void checkResult(int result, [String? context]) {
  if (result == EntiDbResult.ok) return;
  throw EntiDbError.fromResult(result, context);
}
