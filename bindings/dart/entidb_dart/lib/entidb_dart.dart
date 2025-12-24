/// EntiDB Dart Bindings
///
/// A high-performance embedded entity database with ACID transactions,
/// CBOR storage, and language-native filtering.
///
/// ## Quick Start
///
/// ```dart
/// import 'package:entidb_dart/entidb_dart.dart';
///
/// void main() {
///   // Open an in-memory database
///   final db = Database.openMemory();
///
///   // Get or create a collection
///   final users = db.collection('users');
///
///   // Generate a unique entity ID
///   final userId = EntityId.generate();
///
///   // Store data
///   db.put(users, userId, Uint8List.fromList([1, 2, 3, 4]));
///
///   // Retrieve data
///   final data = db.get(users, userId);
///
///   // Use transactions for atomic operations
///   db.transaction((txn) {
///     txn.put(users, EntityId.generate(), Uint8List.fromList([5, 6]));
///     txn.put(users, EntityId.generate(), Uint8List.fromList([7, 8]));
///   });
///
///   db.close();
/// }
/// ```
///
/// ## Typed Collections
///
/// For type-safe operations, use [TypedCollection] with a [Codec]:
///
/// ```dart
/// final userCodec = FunctionCodec<User>(
///   encode: (user) => cbor.encode(user.toMap()),
///   decode: (bytes) => User.fromMap(cbor.decode(bytes)),
/// );
///
/// final users = db.typedCollection<User>('users', userCodec);
/// users.put(userId, User(name: 'Alice'));
/// ```
library entidb_dart;

export 'src/database.dart'
    show Database, RestoreStats, BackupInfo, DatabaseStats;
export 'src/collection.dart' show Collection;
export 'src/entity_id.dart' show EntityId;
export 'src/transaction.dart' show Transaction;
export 'src/iterator.dart' show EntityIterator, EntityIteratorExtensions;
export 'src/error.dart'
    show
        EntiDbError,
        EntiDbNotFoundError,
        EntiDbInvalidError,
        EntiDbIoError,
        EntiDbCorruptionError,
        EntiDbTransactionError;
export 'src/codec.dart' show Codec, FunctionCodec, BytesCodec, StringCodec;
export 'src/typed_collection.dart'
    show TypedCollection, TypedCollectionExtension;
export 'src/bindings.dart' show setEntiDbLibraryPath;
