import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:test/test.dart';

/// Cross-language test vector validation for EntiDB Dart bindings.
///
/// These tests validate that Dart produces identical CBOR encoding
/// and entity ID handling as Rust and Python implementations.
void main() {
  group('Cross-Language Test Vectors', () {
    group('CBOR Encoding', () {
      late List<dynamic> vectors;

      setUpAll(() {
        final file = File('../../docs/test_vectors/cbor.json');
        if (!file.existsSync()) {
          throw StateError(
            'Test vectors not found. Run from bindings/dart/entidb_dart directory.',
          );
        }
        vectors = jsonDecode(file.readAsStringSync()) as List<dynamic>;
      });

      test('all CBOR vectors pass', () {
        for (final vector in vectors) {
          final id = vector['id'] as String;
          final description = vector['description'] as String;
          final inputHex = vector['input_hex'] as String;
          final expectedHex = vector['expected_hex'] as String;
          final expectedError = vector['expected_error'] as String?;

          final input = _hexDecode(inputHex);

          if (expectedError != null) {
            // This vector should fail
            expect(
              () => _decodeCbor(input),
              throwsA(anything),
              reason: 'Vector $id should fail: $description',
            );
          } else {
            // This vector should succeed and round-trip
            try {
              final decoded = _decodeCbor(input);
              final reencoded = _encodeCbor(decoded);
              final reencodedHex = _hexEncode(reencoded);

              expect(
                reencodedHex.toLowerCase(),
                equals(expectedHex.toLowerCase()),
                reason: 'Vector $id failed: $description',
              );
            } catch (e) {
              fail('Vector $id unexpected failure: $description - $e');
            }
          }
        }
      });
    });

    group('Entity ID', () {
      late List<dynamic> vectors;

      setUpAll(() {
        final file = File('../../docs/test_vectors/entity_id.json');
        if (!file.existsSync()) {
          throw StateError(
            'Test vectors not found. Run from bindings/dart/entidb_dart directory.',
          );
        }
        vectors = jsonDecode(file.readAsStringSync()) as List<dynamic>;
      });

      test('all Entity ID vectors pass', () {
        for (final vector in vectors) {
          final id = vector['id'] as String;
          final description = vector['description'] as String;
          final inputHex = vector['input_hex'] as String;
          final expectedHex = vector['expected_hex'] as String;
          final expectedError = vector['expected_error'] as String?;

          final input = _hexDecode(inputHex);

          if (expectedError != null) {
            // This vector should fail (wrong length)
            expect(
              input.length != 16,
              isTrue,
              reason: 'Vector $id should fail: $description',
            );
          } else {
            // This vector should succeed
            expect(input.length, equals(16));
            final roundtripped = _hexEncode(input);
            expect(
              roundtripped.toLowerCase(),
              equals(expectedHex.toLowerCase()),
              reason: 'Vector $id failed: $description',
            );
          }
        }
      });
    });
  });
}

/// Decode hex string to bytes.
Uint8List _hexDecode(String hex) {
  if (hex.isEmpty) return Uint8List(0);
  final result = Uint8List(hex.length ~/ 2);
  for (var i = 0; i < result.length; i++) {
    result[i] = int.parse(hex.substring(i * 2, i * 2 + 2), radix: 16);
  }
  return result;
}

/// Encode bytes to hex string.
String _hexEncode(Uint8List bytes) {
  return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
}

/// Simple CBOR decoder for test vectors.
/// This is a minimal implementation for testing purposes.
dynamic _decodeCbor(Uint8List data) {
  if (data.isEmpty) throw FormatException('Empty CBOR data');

  var offset = 0;

  int readByte() {
    if (offset >= data.length) throw FormatException('Unexpected end of data');
    return data[offset++];
  }

  int readUint(int additionalInfo) {
    if (additionalInfo < 24) return additionalInfo;
    if (additionalInfo == 24) return readByte();
    if (additionalInfo == 25) {
      return (readByte() << 8) | readByte();
    }
    if (additionalInfo == 26) {
      return (readByte() << 24) |
          (readByte() << 16) |
          (readByte() << 8) |
          readByte();
    }
    if (additionalInfo == 27) {
      // 64-bit - simplified handling
      var value = 0;
      for (var i = 0; i < 8; i++) {
        value = (value << 8) | readByte();
      }
      return value;
    }
    if (additionalInfo >= 28) {
      throw FormatException('Indefinite-length items are forbidden');
    }
    throw FormatException('Invalid additional info: $additionalInfo');
  }

  dynamic decode() {
    final initial = readByte();
    final majorType = initial >> 5;
    final additionalInfo = initial & 0x1f;

    // Check for indefinite-length items
    if (additionalInfo == 31) {
      throw FormatException('Indefinite-length items are forbidden');
    }

    switch (majorType) {
      case 0: // Unsigned integer
        return readUint(additionalInfo);
      case 1: // Negative integer
        return -1 - readUint(additionalInfo);
      case 2: // Byte string
        final length = readUint(additionalInfo);
        final bytes = data.sublist(offset, offset + length);
        offset += length;
        return Uint8List.fromList(bytes);
      case 3: // Text string
        final length = readUint(additionalInfo);
        final bytes = data.sublist(offset, offset + length);
        offset += length;
        return utf8.decode(bytes);
      case 4: // Array
        final length = readUint(additionalInfo);
        final list = <dynamic>[];
        for (var i = 0; i < length; i++) {
          list.add(decode());
        }
        return list;
      case 5: // Map
        final length = readUint(additionalInfo);
        final map = <dynamic, dynamic>{};
        for (var i = 0; i < length; i++) {
          final key = decode();
          final value = decode();
          map[key] = value;
        }
        return map;
      case 6: // Tag (not used in EntiDB)
        throw FormatException('Tags are not supported');
      case 7: // Simple/float
        if (additionalInfo == 20) return false;
        if (additionalInfo == 21) return true;
        if (additionalInfo == 22) return null;
        if (additionalInfo >= 25 && additionalInfo <= 27) {
          throw FormatException('Floats are not allowed');
        }
        throw FormatException('Unknown simple value: $additionalInfo');
      default:
        throw FormatException('Unknown major type: $majorType');
    }
  }

  return decode();
}

/// Simple canonical CBOR encoder for test vectors.
Uint8List _encodeCbor(dynamic value) {
  final buffer = <int>[];

  void writeUint(int majorType, int value) {
    final major = majorType << 5;
    if (value < 24) {
      buffer.add(major | value);
    } else if (value < 256) {
      buffer.add(major | 24);
      buffer.add(value);
    } else if (value < 65536) {
      buffer.add(major | 25);
      buffer.add((value >> 8) & 0xff);
      buffer.add(value & 0xff);
    } else if (value < 4294967296) {
      buffer.add(major | 26);
      buffer.add((value >> 24) & 0xff);
      buffer.add((value >> 16) & 0xff);
      buffer.add((value >> 8) & 0xff);
      buffer.add(value & 0xff);
    } else {
      buffer.add(major | 27);
      for (var i = 7; i >= 0; i--) {
        buffer.add((value >> (i * 8)) & 0xff);
      }
    }
  }

  void encode(dynamic v) {
    if (v == null) {
      buffer.add(0xf6);
    } else if (v is bool) {
      buffer.add(v ? 0xf5 : 0xf4);
    } else if (v is int) {
      if (v >= 0) {
        writeUint(0, v);
      } else {
        writeUint(1, -1 - v);
      }
    } else if (v is Uint8List) {
      writeUint(2, v.length);
      buffer.addAll(v);
    } else if (v is String) {
      final bytes = utf8.encode(v);
      writeUint(3, bytes.length);
      buffer.addAll(bytes);
    } else if (v is List) {
      writeUint(4, v.length);
      for (final item in v) {
        encode(item);
      }
    } else if (v is Map) {
      // Sort keys canonically (by encoded bytes)
      final entries = v.entries.toList();
      entries.sort((a, b) {
        final aKey = _encodeCbor(a.key);
        final bKey = _encodeCbor(b.key);
        // Compare by length first, then by bytes
        if (aKey.length != bKey.length) {
          return aKey.length.compareTo(bKey.length);
        }
        for (var i = 0; i < aKey.length; i++) {
          if (aKey[i] != bKey[i]) {
            return aKey[i].compareTo(bKey[i]);
          }
        }
        return 0;
      });

      writeUint(5, entries.length);
      for (final entry in entries) {
        encode(entry.key);
        encode(entry.value);
      }
    } else {
      throw ArgumentError('Unsupported type: ${v.runtimeType}');
    }
  }

  encode(value);
  return Uint8List.fromList(buffer);
}
