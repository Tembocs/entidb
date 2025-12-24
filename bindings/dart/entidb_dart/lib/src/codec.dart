/// Codec interface for entity serialization.
///
/// This module provides a type-safe interface for converting entities
/// to and from CBOR bytes.
library;

import 'dart:typed_data';

/// A codec for encoding and decoding entities of type [T].
///
/// Implement this interface to define how your entity types are
/// serialized to and from CBOR bytes.
///
/// ## Example
///
/// ```dart
/// class UserCodec implements Codec<User> {
///   @override
///   Uint8List encode(User value) {
///     // Encode user to CBOR bytes
///     return cbor.encode({
///       'name': value.name,
///       'email': value.email,
///     });
///   }
///
///   @override
///   User decode(Uint8List bytes) {
///     // Decode CBOR bytes to user
///     final map = cbor.decode(bytes);
///     return User(
///       name: map['name'],
///       email: map['email'],
///     );
///   }
/// }
/// ```
abstract interface class Codec<T> {
  /// Encodes a value to bytes.
  ///
  /// The returned bytes should be canonical CBOR as defined in
  /// the EntiDB specification.
  Uint8List encode(T value);

  /// Decodes bytes to a value.
  ///
  /// Throws on invalid input.
  T decode(Uint8List bytes);
}

/// A simple codec that wraps encode/decode functions.
///
/// ## Example
///
/// ```dart
/// final userCodec = FunctionCodec<User>(
///   encode: (user) => utf8.encode(jsonEncode(user.toJson())),
///   decode: (bytes) => User.fromJson(jsonDecode(utf8.decode(bytes))),
/// );
/// ```
final class FunctionCodec<T> implements Codec<T> {
  final Uint8List Function(T value) _encode;
  final T Function(Uint8List bytes) _decode;

  /// Creates a codec from encode/decode functions.
  const FunctionCodec({
    required Uint8List Function(T value) encode,
    required T Function(Uint8List bytes) decode,
  })  : _encode = encode,
        _decode = decode;

  @override
  Uint8List encode(T value) => _encode(value);

  @override
  T decode(Uint8List bytes) => _decode(bytes);
}

/// A pass-through codec for raw bytes.
///
/// Use this when you want to store and retrieve raw bytes without
/// any transformation.
final class BytesCodec implements Codec<Uint8List> {
  /// The singleton instance.
  static const BytesCodec instance = BytesCodec._();

  const BytesCodec._();

  @override
  Uint8List encode(Uint8List value) => value;

  @override
  Uint8List decode(Uint8List bytes) => bytes;
}

/// A codec for UTF-8 strings.
///
/// Encodes strings as raw UTF-8 bytes (not CBOR text strings).
final class StringCodec implements Codec<String> {
  /// The singleton instance.
  static const StringCodec instance = StringCodec._();

  const StringCodec._();

  @override
  Uint8List encode(String value) {
    final units = value.codeUnits;
    return Uint8List.fromList(units);
  }

  @override
  String decode(Uint8List bytes) {
    return String.fromCharCodes(bytes);
  }
}
