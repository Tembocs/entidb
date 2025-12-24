/// Collection type.
library;

/// Represents a named collection of entities.
///
/// Collections are created via [Database.collection]. Multiple calls
/// with the same name return the same collection ID.
///
/// ## Example
///
/// ```dart
/// final users = db.collection('users');
/// final products = db.collection('products');
///
/// // Store in different collections
/// db.put(users, userId, userData);
/// db.put(products, productId, productData);
/// ```
final class Collection {
  /// The collection name.
  final String name;

  /// The internal collection ID.
  final int id;

  /// Creates a collection reference.
  ///
  /// This is an internal constructor. Use [Database.collection] instead.
  const Collection.internal(this.name, this.id);

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    return other is Collection && other.id == id;
  }

  @override
  int get hashCode => id.hashCode;

  @override
  String toString() => 'Collection($name, id: $id)';
}
