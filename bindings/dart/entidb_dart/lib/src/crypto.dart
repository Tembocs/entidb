import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.dart';
import 'error.dart';

/// Encryption manager for EntiDB.
///
/// Provides AES-256-GCM encryption and decryption capabilities.
/// Keys are 32 bytes (256 bits) and are zeroized when the manager is closed.
///
/// Example:
/// ```dart
/// // Create with a generated key
/// final crypto = await CryptoManager.create();
/// final key = crypto.key; // Save this key securely!
///
/// // Encrypt data
/// final encrypted = crypto.encrypt(Uint8List.fromList(utf8.encode('secret')));
///
/// // Decrypt data
/// final decrypted = crypto.decrypt(encrypted);
///
/// // Always close when done
/// crypto.close();
/// ```
class CryptoManager {
  CryptoManager._(this._handle, this._key);

  final Pointer<EntiDbCryptoHandle> _handle;
  final Uint8List _key;
  bool _isClosed = false;

  /// Returns true if encryption is available in the native library.
  ///
  /// Encryption requires the `encryption` feature to be enabled when building
  /// the Rust library.
  static bool get isAvailable {
    try {
      return bindings.entidbCryptoAvailable();
    } catch (e) {
      return false;
    }
  }

  /// Creates a new CryptoManager with a generated random key.
  ///
  /// The generated key can be accessed via the [key] property and should be
  /// stored securely for future use.
  static CryptoManager create() {
    final handlePtr = calloc<Pointer<EntiDbCryptoHandle>>();
    final keyPtr = calloc<Uint8>(32);

    try {
      final result = bindings.entidbCryptoCreate(handlePtr, keyPtr);
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      final keyBytes = Uint8List(32);
      for (var i = 0; i < 32; i++) {
        keyBytes[i] = keyPtr[i];
      }

      return CryptoManager._(handlePtr.value, keyBytes);
    } finally {
      calloc.free(handlePtr);
      // Zero out key buffer before freeing
      for (var i = 0; i < 32; i++) {
        keyPtr[i] = 0;
      }
      calloc.free(keyPtr);
    }
  }

  /// Creates a CryptoManager from an existing key.
  ///
  /// The key must be exactly 32 bytes (256 bits).
  static CryptoManager fromKey(Uint8List key) {
    if (key.length != 32) {
      throw ArgumentError('Key must be exactly 32 bytes, got ${key.length}');
    }

    final handlePtr = calloc<Pointer<EntiDbCryptoHandle>>();
    final keyPtr = calloc<Uint8>(32);

    try {
      for (var i = 0; i < 32; i++) {
        keyPtr[i] = key[i];
      }

      final result = bindings.entidbCryptoCreateWithKey(keyPtr, handlePtr);
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      return CryptoManager._(handlePtr.value, Uint8List.fromList(key));
    } finally {
      calloc.free(handlePtr);
      // Zero out key buffer before freeing
      for (var i = 0; i < 32; i++) {
        keyPtr[i] = 0;
      }
      calloc.free(keyPtr);
    }
  }

  /// Creates a CryptoManager from a password and salt.
  ///
  /// The password and salt are used to derive a key using HKDF.
  /// The same password and salt will always produce the same key.
  ///
  /// For security, the salt should be unique per database and stored alongside
  /// the encrypted data.
  static CryptoManager fromPassword(Uint8List password, Uint8List salt) {
    final handlePtr = calloc<Pointer<EntiDbCryptoHandle>>();
    final passwordPtr = calloc<Uint8>(password.length);
    final saltPtr = calloc<Uint8>(salt.length);

    try {
      for (var i = 0; i < password.length; i++) {
        passwordPtr[i] = password[i];
      }
      for (var i = 0; i < salt.length; i++) {
        saltPtr[i] = salt[i];
      }

      final result = bindings.entidbCryptoCreateFromPassword(
        passwordPtr,
        password.length,
        saltPtr,
        salt.length,
        handlePtr,
      );
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      // Key is derived internally, we don't have access to it
      return CryptoManager._(handlePtr.value, Uint8List(32));
    } finally {
      calloc.free(handlePtr);
      // Zero out password buffer before freeing
      for (var i = 0; i < password.length; i++) {
        passwordPtr[i] = 0;
      }
      calloc.free(passwordPtr);
      calloc.free(saltPtr);
    }
  }

  /// Creates a CryptoManager from a password string and salt.
  ///
  /// Convenience method that converts the password string to bytes.
  static CryptoManager fromPasswordString(String password, Uint8List salt) {
    final passwordBytes = Uint8List.fromList(password.codeUnits);
    return fromPassword(passwordBytes, salt);
  }

  /// The encryption key (32 bytes).
  ///
  /// This key should be stored securely if you need to decrypt data later.
  /// For password-derived keys, this may be empty or unset.
  Uint8List get key => Uint8List.fromList(_key);

  /// Encrypts data using AES-256-GCM.
  ///
  /// Returns the encrypted data with nonce prepended:
  /// `nonce (12 bytes) || ciphertext || tag (16 bytes)`
  ///
  /// The encrypted data is 28 bytes larger than the plaintext.
  Uint8List encrypt(Uint8List data) {
    _checkClosed();

    final dataPtr = calloc<Uint8>(data.length);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      for (var i = 0; i < data.length; i++) {
        dataPtr[i] = data[i];
      }

      final result = bindings.entidbCryptoEncrypt(
        _handle,
        dataPtr,
        data.length,
        bufferPtr,
      );
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      final encrypted = Uint8List(bufferPtr.ref.len);
      for (var i = 0; i < bufferPtr.ref.len; i++) {
        encrypted[i] = bufferPtr.ref.data[i];
      }

      bindings.entidbFreeBuffer(bufferPtr.ref);
      return encrypted;
    } finally {
      calloc.free(dataPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Decrypts data that was encrypted with [encrypt].
  ///
  /// Throws an exception if decryption fails (wrong key, corrupted data, etc.).
  Uint8List decrypt(Uint8List data) {
    _checkClosed();

    final dataPtr = calloc<Uint8>(data.length);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      for (var i = 0; i < data.length; i++) {
        dataPtr[i] = data[i];
      }

      final result = bindings.entidbCryptoDecrypt(
        _handle,
        dataPtr,
        data.length,
        bufferPtr,
      );
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      final decrypted = Uint8List(bufferPtr.ref.len);
      for (var i = 0; i < bufferPtr.ref.len; i++) {
        decrypted[i] = bufferPtr.ref.data[i];
      }

      bindings.entidbFreeBuffer(bufferPtr.ref);
      return decrypted;
    } finally {
      calloc.free(dataPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Encrypts data with associated authenticated data (AAD).
  ///
  /// The AAD is authenticated but not encrypted. This is useful for binding
  /// the ciphertext to metadata (e.g., entity ID, collection name).
  ///
  /// The same AAD must be provided for decryption.
  Uint8List encryptWithAad(Uint8List data, Uint8List aad) {
    _checkClosed();

    final dataPtr = calloc<Uint8>(data.length);
    final aadPtr = calloc<Uint8>(aad.length);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      for (var i = 0; i < data.length; i++) {
        dataPtr[i] = data[i];
      }
      for (var i = 0; i < aad.length; i++) {
        aadPtr[i] = aad[i];
      }

      final result = bindings.entidbCryptoEncryptWithAad(
        _handle,
        dataPtr,
        data.length,
        aadPtr,
        aad.length,
        bufferPtr,
      );
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      final encrypted = Uint8List(bufferPtr.ref.len);
      for (var i = 0; i < bufferPtr.ref.len; i++) {
        encrypted[i] = bufferPtr.ref.data[i];
      }

      bindings.entidbFreeBuffer(bufferPtr.ref);
      return encrypted;
    } finally {
      calloc.free(dataPtr);
      calloc.free(aadPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Decrypts data with associated authenticated data (AAD).
  ///
  /// The same AAD that was used during encryption must be provided.
  /// Throws an exception if the AAD doesn't match or decryption fails.
  Uint8List decryptWithAad(Uint8List data, Uint8List aad) {
    _checkClosed();

    final dataPtr = calloc<Uint8>(data.length);
    final aadPtr = calloc<Uint8>(aad.length);
    final bufferPtr = calloc<EntiDbBuffer>();

    try {
      for (var i = 0; i < data.length; i++) {
        dataPtr[i] = data[i];
      }
      for (var i = 0; i < aad.length; i++) {
        aadPtr[i] = aad[i];
      }

      final result = bindings.entidbCryptoDecryptWithAad(
        _handle,
        dataPtr,
        data.length,
        aadPtr,
        aad.length,
        bufferPtr,
      );
      if (result != EntiDbResult.ok) {
        throw EntiDbError.fromResult(result);
      }

      final decrypted = Uint8List(bufferPtr.ref.len);
      for (var i = 0; i < bufferPtr.ref.len; i++) {
        decrypted[i] = bufferPtr.ref.data[i];
      }

      bindings.entidbFreeBuffer(bufferPtr.ref);
      return decrypted;
    } finally {
      calloc.free(dataPtr);
      calloc.free(aadPtr);
      calloc.free(bufferPtr);
    }
  }

  /// Closes the crypto manager and releases native resources.
  ///
  /// The manager should not be used after calling this method.
  void close() {
    if (!_isClosed) {
      bindings.entidbCryptoFree(_handle);
      _isClosed = true;
    }
  }

  void _checkClosed() {
    if (_isClosed) {
      throw StateError('CryptoManager has been closed');
    }
  }

  /// Whether this manager has been closed.
  bool get isClosed => _isClosed;
}
