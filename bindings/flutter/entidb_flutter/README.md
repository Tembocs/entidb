# EntiDB Flutter Plugin

Flutter plugin for [EntiDB](https://github.com/Tembocs/entidb) - an embedded entity database with ACID transactions and CBOR storage.

This package bundles the native EntiDB libraries for all supported platforms, so you don't need to manually install anything.

## Installation

Add to your `pubspec.yaml`:

```yaml
dependencies:
  entidb_flutter: ^2.0.0-alpha.1
```

## Usage

```dart
import 'package:entidb_flutter/entidb_flutter.dart';

void main() async {
  // Open a file-based database
  final db = Database.open('/path/to/database');

  // Or use in-memory for testing
  final memDb = Database.openMemory();

  // Get a collection
  final users = db.collection('users');

  // Generate a unique ID
  final id = EntityId.generate();

  // Store data (CBOR bytes)
  db.put(users, id, Uint8List.fromList([1, 2, 3]));

  // Retrieve data
  final data = db.get(users, id);

  // Use transactions for atomic operations
  db.transaction((txn) {
    txn.put(users, id1, data1);
    txn.put(users, id2, data2);
    // All operations commit atomically
  });

  // Always close when done
  db.close();
}
```

## Platform Support

| Platform | Architecture | Status |
|----------|-------------|--------|
| Android  | arm64-v8a, armeabi-v7a, x86_64 | ✅ |
| iOS      | arm64, arm64-simulator, x86_64-simulator | ✅ |
| macOS    | arm64, x86_64 | ✅ |
| Windows  | x86_64 | ✅ |
| Linux    | x86_64 | ✅ |
| Web      | - | ❌ (use `entidb_web`) |

## Minimum Requirements

- Flutter 3.0.0+
- Dart 3.0.0+
- Android SDK 21+ (Android 5.0)
- iOS 12.0+
- macOS 10.14+
- Windows 10+
- Linux (glibc 2.17+)

## Architecture

This plugin:
1. Bundles prebuilt native libraries (`libentidb_ffi`) for each platform
2. Re-exports the pure Dart API from `entidb_dart`
3. Uses Flutter's FFI plugin mechanism for automatic library loading

## Documentation

- [API Reference](https://pub.dev/documentation/entidb_dart/latest/)
- [GitHub Repository](https://github.com/Tembocs/entidb)
- [Quick Start Guide](https://github.com/Tembocs/entidb/blob/main/docs/quickstart.md)

## Related Packages

- [`entidb_dart`](https://pub.dev/packages/entidb_dart) - Pure Dart bindings (requires manual native library setup)
- `entidb_web` - Web support via WASM (coming soon)

## License

MIT OR Apache-2.0
