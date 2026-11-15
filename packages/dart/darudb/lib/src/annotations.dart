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

/// Keeps a text index on the field, a `String` or a `List<String>`, nullable
/// or not: an entry for each word of the text, which a search for words,
/// `matches` in a query or `MATCHES` in the query language, reads rather
/// than every object. The field may be annotated [Index] too, for an index
/// on its values. Every write of the object writes an entry for each
/// different word, so a long text costs more to write than any other field.
/// A file with a text index is in object format 3.
final class TextIndex {
  const TextIndex();
}

/// Makes deleting the object the [Link] field names delete the object that
/// holds it, in the same transaction, and so on down the chain. Only a
/// `Link`, nullable or not, of the collection itself cascades, not one in a
/// `List` or an embedded object. Annotate it [Index] too: an index is how a
/// delete finds the objects that link to one, and without it each delete
/// reads the whole collection. A file with a cascading link is in object
/// format 2.
final class Cascade {
  const Cascade();
}

/// Keeps an index on several [fields] of the collection, in order, or on a
/// field inside an embedded object, annotating the class beside
/// [Collection]: `@IndexOn(['last', 'first'], unique: true)`. Each is a
/// field's Dart name, or names joined by `.` through embedded objects, as
/// `'home.city'`. Its entries sort by the first field, then the next, so a
/// query testing the first fields for equality and the next for a range
/// reads it. A unique one refuses two objects with the same values in every
/// field, though any number may hold null in one. There are 1 to 8 fields,
/// and a path through a list is allowed in an index of one field. A file
/// with such an index is in object format 2.
///
/// With [text], it is a text index, as [TextIndex] keeps on a field, on the
/// one field named, a `String` or a `List<String>` inside an embedded
/// object: `@IndexOn(['meta.note'], text: true)`. A text index is on one
/// field and is never unique.
final class IndexOn {
  const IndexOn(this.fields, {this.unique = false, this.text = false});

  final List<String> fields;
  final bool unique;

  /// Whether the index holds the words of its field, for a search to read.
  final bool text;
}

/// Names the field [name] in the collection, rather than as the Dart field
/// is named.
final class Name {
  const Name(this.name);

  final String name;
}
