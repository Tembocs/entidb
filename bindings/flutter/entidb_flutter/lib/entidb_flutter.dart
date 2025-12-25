/// EntiDB Flutter Plugin
///
/// This package provides EntiDB database functionality for Flutter applications
/// by bundling the native libraries for all supported platforms.
///
/// ## Usage
///
/// Add `entidb_flutter` to your `pubspec.yaml`:
///
/// ```yaml
/// dependencies:
///   entidb_flutter: ^2.0.0-alpha.1
/// ```
///
/// Then import and use:
///
/// ```dart
/// import 'package:entidb_flutter/entidb_flutter.dart';
///
/// void main() async {
///   // Open a database
///   final db = Database.open('/path/to/database');
///
///   // Or use in-memory
///   final memDb = Database.openMemory();
///
///   // Get a collection
///   final users = db.collection('users');
///
///   // Store data
///   final id = EntityId.generate();
///   db.put(users, id, Uint8List.fromList([1, 2, 3]));
///
///   // Retrieve data
///   final data = db.get(users, id);
///
///   // Use transactions
///   db.transaction((txn) {
///     txn.put(users, id, data);
///   });
///
///   // Always close when done
///   db.close();
/// }
/// ```
///
/// ## Platform Support
///
/// | Platform | Support |
/// |----------|---------|
/// | Android  | ✅      |
/// | iOS      | ✅      |
/// | macOS    | ✅      |
/// | Windows  | ✅      |
/// | Linux    | ✅      |
/// | Web      | ❌ (use entidb_web) |
///
/// ## Documentation
///
/// For full API documentation, see the [entidb_dart](https://pub.dev/packages/entidb_dart) package.
library entidb_flutter;

// Re-export everything from entidb_dart
export 'package:entidb_dart/entidb_dart.dart';
