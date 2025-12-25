import 'dart:convert';
import 'dart:typed_data';

import 'package:entidb_dart/entidb_dart.dart';
import 'package:test/test.dart';

/// Crypto tests require the native library to be built with encryption feature.
/// Run: `cargo build --release -p entidb_ffi` first.
void main() {
  group('CryptoManager', () {
    test('isAvailable returns true when encryption feature enabled', () {
      expect(CryptoManager.isAvailable, isTrue);
    });

    test('create generates unique key', () {
      final crypto1 = CryptoManager.create();
      final crypto2 = CryptoManager.create();
      try {
        expect(crypto1.key, hasLength(32));
        expect(crypto2.key, hasLength(32));
        // Keys should be different
        expect(crypto1.key, isNot(equals(crypto2.key)));
      } finally {
        crypto1.close();
        crypto2.close();
      }
    });

    test('fromKey restores same encryption context', () {
      final crypto1 = CryptoManager.create();
      final key = crypto1.key;
      final plaintext = Uint8List.fromList(utf8.encode('test message'));
      final encrypted = crypto1.encrypt(plaintext);
      crypto1.close();

      // Create new crypto with same key
      final crypto2 = CryptoManager.fromKey(key);
      try {
        final decrypted = crypto2.decrypt(encrypted);
        expect(decrypted, equals(plaintext));
      } finally {
        crypto2.close();
      }
    });

    test('encrypt/decrypt roundtrip', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('Hello, EntiDB!'));
        final encrypted = crypto.encrypt(plaintext);

        // Encrypted data should be larger (nonce + ciphertext + tag)
        expect(encrypted.length, equals(plaintext.length + 28));

        // Encrypted data should be different from plaintext
        expect(encrypted, isNot(equals(plaintext)));

        final decrypted = crypto.decrypt(encrypted);
        expect(decrypted, equals(plaintext));
        expect(utf8.decode(decrypted), equals('Hello, EntiDB!'));
      } finally {
        crypto.close();
      }
    });

    test('encrypt produces different ciphertext each time', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('same message'));
        final encrypted1 = crypto.encrypt(plaintext);
        final encrypted2 = crypto.encrypt(plaintext);

        // Different nonces should produce different ciphertext
        expect(encrypted1, isNot(equals(encrypted2)));

        // But both should decrypt to same plaintext
        expect(crypto.decrypt(encrypted1), equals(plaintext));
        expect(crypto.decrypt(encrypted2), equals(plaintext));
      } finally {
        crypto.close();
      }
    });

    test('encryptWithAad/decryptWithAad roundtrip', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('secret data'));
        final aad = Uint8List.fromList(utf8.encode('entity-id-123'));

        final encrypted = crypto.encryptWithAad(plaintext, aad);
        final decrypted = crypto.decryptWithAad(encrypted, aad);

        expect(decrypted, equals(plaintext));
      } finally {
        crypto.close();
      }
    });

    test('decryptWithAad fails with wrong AAD', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('secret data'));
        final correctAad = Uint8List.fromList(utf8.encode('correct-aad'));
        final wrongAad = Uint8List.fromList(utf8.encode('wrong-aad'));

        final encrypted = crypto.encryptWithAad(plaintext, correctAad);

        // Should fail with wrong AAD
        expect(
          () => crypto.decryptWithAad(encrypted, wrongAad),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        crypto.close();
      }
    });

    test('decrypt fails with wrong key', () {
      final crypto1 = CryptoManager.create();
      final crypto2 = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('secret'));
        final encrypted = crypto1.encrypt(plaintext);

        // Should fail to decrypt with different key
        expect(
          () => crypto2.decrypt(encrypted),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        crypto1.close();
        crypto2.close();
      }
    });

    test('decrypt fails with corrupted data', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('original'));
        final encrypted = crypto.encrypt(plaintext);

        // Corrupt the ciphertext
        final corrupted = Uint8List.fromList(encrypted);
        corrupted[20] = corrupted[20] ^ 0xFF;

        expect(
          () => crypto.decrypt(corrupted),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        crypto.close();
      }
    });

    test('decrypt fails with truncated data', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('test data'));
        final encrypted = crypto.encrypt(plaintext);

        // Truncate the data (too short for nonce + tag)
        final truncated = encrypted.sublist(0, 10);

        expect(
          () => crypto.decrypt(truncated),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        crypto.close();
      }
    });

    test('fromPassword creates consistent key', () {
      final password = Uint8List.fromList(utf8.encode('my-secret-password'));
      final salt = Uint8List.fromList(utf8.encode('unique-salt-12345678'));

      final crypto1 = CryptoManager.fromPassword(password, salt);
      final plaintext = Uint8List.fromList(utf8.encode('message'));
      final encrypted = crypto1.encrypt(plaintext);
      crypto1.close();

      // Same password and salt should be able to decrypt
      final crypto2 = CryptoManager.fromPassword(password, salt);
      try {
        final decrypted = crypto2.decrypt(encrypted);
        expect(decrypted, equals(plaintext));
      } finally {
        crypto2.close();
      }
    });

    test('fromPasswordString convenience method', () {
      final salt = Uint8List.fromList(utf8.encode('test-salt-12345678'));

      final crypto = CryptoManager.fromPasswordString('my-password', salt);
      try {
        final plaintext = Uint8List.fromList(utf8.encode('data'));
        final encrypted = crypto.encrypt(plaintext);
        final decrypted = crypto.decrypt(encrypted);
        expect(decrypted, equals(plaintext));
      } finally {
        crypto.close();
      }
    });

    test('fromPassword with wrong password fails', () {
      final salt = Uint8List.fromList(utf8.encode('salt-value-123456'));
      final plaintext = Uint8List.fromList(utf8.encode('secret'));

      final crypto1 = CryptoManager.fromPassword(
          Uint8List.fromList(utf8.encode('correct-password')), salt);
      final encrypted = crypto1.encrypt(plaintext);
      crypto1.close();

      final crypto2 = CryptoManager.fromPassword(
          Uint8List.fromList(utf8.encode('wrong-password')), salt);
      try {
        expect(
          () => crypto2.decrypt(encrypted),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        crypto2.close();
      }
    });

    test('fromPassword with different salt fails', () {
      final password = Uint8List.fromList(utf8.encode('same-password'));
      final salt1 = Uint8List.fromList(utf8.encode('salt-one-123456'));
      final salt2 = Uint8List.fromList(utf8.encode('salt-two-654321'));
      final plaintext = Uint8List.fromList(utf8.encode('secret'));

      final crypto1 = CryptoManager.fromPassword(password, salt1);
      final encrypted = crypto1.encrypt(plaintext);
      crypto1.close();

      final crypto2 = CryptoManager.fromPassword(password, salt2);
      try {
        expect(
          () => crypto2.decrypt(encrypted),
          throwsA(isA<EntiDbError>()),
        );
      } finally {
        crypto2.close();
      }
    });

    test('fromKey throws for wrong key length', () {
      expect(
        () => CryptoManager.fromKey(Uint8List(16)),
        throwsA(isA<ArgumentError>()),
      );
      expect(
        () => CryptoManager.fromKey(Uint8List(64)),
        throwsA(isA<ArgumentError>()),
      );
    });

    test('operations throw after close', () {
      final crypto = CryptoManager.create();
      crypto.close();

      expect(
        () => crypto.encrypt(Uint8List.fromList([1, 2, 3])),
        throwsA(isA<StateError>()),
      );
      expect(
        () => crypto.decrypt(Uint8List(50)),
        throwsA(isA<StateError>()),
      );
    });

    test('close is idempotent', () {
      final crypto = CryptoManager.create();
      expect(crypto.isClosed, isFalse);

      crypto.close();
      expect(crypto.isClosed, isTrue);

      // Second close should not throw
      crypto.close();
      expect(crypto.isClosed, isTrue);
    });

    test('encrypt empty data', () {
      final crypto = CryptoManager.create();
      try {
        final empty = Uint8List(0);
        final encrypted = crypto.encrypt(empty);

        // Should have overhead but no plaintext bytes
        expect(encrypted.length, equals(28)); // 12 (nonce) + 0 + 16 (tag)

        final decrypted = crypto.decrypt(encrypted);
        expect(decrypted, isEmpty);
      } finally {
        crypto.close();
      }
    });

    test('encrypt large data', () {
      final crypto = CryptoManager.create();
      try {
        // 1 MB of random-ish data
        final large =
            Uint8List.fromList(List.generate(1024 * 1024, (i) => i % 256));
        final encrypted = crypto.encrypt(large);

        expect(encrypted.length, equals(large.length + 28));

        final decrypted = crypto.decrypt(encrypted);
        expect(decrypted, equals(large));
      } finally {
        crypto.close();
      }
    });

    test('encryptWithAad with empty AAD', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('data'));
        final emptyAad = Uint8List(0);

        final encrypted = crypto.encryptWithAad(plaintext, emptyAad);
        final decrypted = crypto.decryptWithAad(encrypted, emptyAad);

        expect(decrypted, equals(plaintext));
      } finally {
        crypto.close();
      }
    });

    test('encryptWithAad with large AAD', () {
      final crypto = CryptoManager.create();
      try {
        final plaintext = Uint8List.fromList(utf8.encode('data'));
        final largeAad =
            Uint8List.fromList(List.generate(10000, (i) => i % 256));

        final encrypted = crypto.encryptWithAad(plaintext, largeAad);
        final decrypted = crypto.decryptWithAad(encrypted, largeAad);

        expect(decrypted, equals(plaintext));
      } finally {
        crypto.close();
      }
    });
  });
}
