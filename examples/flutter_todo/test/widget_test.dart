// Unit tests for the EntiDB Todo example.
//
// Note: Tests that require the native library (EntityId.generate, Database)
// must run as integration tests on a real device/emulator.
// These tests verify the pure Dart logic only.

import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('Todo JSON serialization roundtrip', () {
    // Test the JSON encoding/decoding logic without requiring FFI
    final original = {
      'title': 'Test Todo',
      'completed': false,
      'priority': 1,
      'created_at': 1234567890,
    };

    final bytes = Uint8List.fromList(utf8.encode(jsonEncode(original)));
    final decoded = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;

    expect(decoded['title'], 'Test Todo');
    expect(decoded['completed'], false);
    expect(decoded['priority'], 1);
    expect(decoded['created_at'], 1234567890);
  });

  test('Todo JSON handles missing optional fields', () {
    final minimal = {'title': 'Minimal Todo'};

    final bytes = Uint8List.fromList(utf8.encode(jsonEncode(minimal)));
    final decoded = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;

    expect(decoded['title'], 'Minimal Todo');
    expect(decoded['completed'], isNull);
    expect(decoded['priority'], isNull);
  });

  test('Todo title validation', () {
    // Empty titles should be handled by the UI, but test the data flow
    final todo = {'title': '', 'completed': false};
    final bytes = Uint8List.fromList(utf8.encode(jsonEncode(todo)));
    final decoded = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;

    expect(decoded['title'], '');
  });
}
