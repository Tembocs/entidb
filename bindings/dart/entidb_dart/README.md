# EntiDB Dart Bindings

Dart bindings for EntiDB - an embedded entity database engine with ACID transactions and CBOR storage.

## Features

- **Entity-first API**: Store and retrieve entities directly
- **ACID Transactions**: Full transaction support with snapshot isolation
- **CBOR Storage**: Canonical CBOR encoding for all data
- **Encryption**: Optional AES-256-GCM encryption at rest
- **Cross-platform**: Works on Windows, macOS, Linux, iOS, and Android

## Installation

Add to your `pubspec.yaml`:

```yaml
dependencies:
  entidb_dart: ^0.1.0
```

## Usage

```dart
import 'package:entidb_dart/entidb_dart.dart';

void main() {
  // Open an in-memory database
  final db = Database.openMemory();
  
  // Get or create a collection
  final users = db.collection('users');
  
  // Generate a new entity ID
  final userId = EntityId.generate();
  
  // Store data
  db.put(users, userId, utf8.encode('{"name": "Alice"}'));
  
  // Retrieve data
  final data = db.get(users, userId);
  print(utf8.decode(data!));
  
  // Use transactions
  final txn = db.transaction();
  txn.put(users, EntityId.generate(), utf8.encode('data1'));
  txn.put(users, EntityId.generate(), utf8.encode('data2'));
  db.commit(txn);
  
  // Close the database
  db.close();
}
```

## File-based Database

```dart
final db = Database.openFile('/path/to/database');
// ... use the database ...
db.close();
```

## Transactions

```dart
final txn = db.transaction();

try {
  txn.put(users, id1, data1);
  txn.put(users, id2, data2);
  db.commit(txn);
} catch (e) {
  db.abort(txn);
  rethrow;
}
```

## Encryption

```dart
// Create a 32-byte encryption key
final key = CryptoManager.generateKey();

// Encrypt data
final encrypted = CryptoManager.encrypt(data, key);

// Decrypt data
final decrypted = CryptoManager.decrypt(encrypted, key);
```

## Platform Support

| Platform | Status |
|----------|--------|
| Windows  | ✅     |
| macOS    | ✅     |
| Linux    | ✅     |
| iOS      | ✅     |
| Android  | ✅     |
| Web      | 🚧 (via WASM) |

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
