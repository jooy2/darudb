/// The annotations `darudb_generator` reads: a class annotated [Collection]
/// becomes the objects of a collection, one annotated [Embedded] an
/// embedded object, and the others mark fields.
library;

/// Makes the class the objects of a collection, named [name], or after the
/// class without one. The class has `final` fields and a constructor that
/// takes each by name; `build_runner` generates its schema constant, such as
/// `userSchema` for `User`, its query builder and a `copyWith`.
///
/// Without a field annotated [PrimaryKey], the collection is keyed by an
/// auto-increment, and the class needs a field `final int? id`, `null` until
/// the object is inserted.
final class Collection {
  const Collection([this.name]);

  final String? name;
}

/// Makes the class an embedded object, which a field of an object holds.
final class Embedded {
  const Embedded();
}

/// Makes the field the collection's primary key: an `int`, a `String` or a
/// `Uint8List`.
final class PrimaryKey {
  const PrimaryKey();
}

/// Keeps an index on the field, so that queries on it read only the objects
/// they find.
final class Index {
  const Index();
}

/// Keeps an index on the field that also refuses two objects with the same
/// value. Any number of objects may hold null.
final class Unique {
  const Unique();
}

/// Names the field [name] in the collection, rather than as the Dart field
/// is named.
final class Name {
  const Name(this.name);

  final String name;
}
