// What the screens know of each collection: its key, its fields and what
// kind of value each holds, which columns the list shows, and which fields it
// can sort by. It mirrors `model.dart`, and `samples/node/core/fields.ts`
// describes the same fields for the Node.js sample.
//
// The screens handle every collection the same way, so an object reaches
// them as a `SampleRow`, a map from field names to values, which `store.dart`
// makes from the annotated classes and back.

/// How a field's value is shown and edited. `tags` is a list of strings,
/// `location` the embedded `Place` of a person, and `color` three bytes.
enum FieldKind {
  string,
  integer,
  float,
  boolean,
  date,
  color,
  link,
  tags,
  location,
}

/// An object as the screens see it: each field's value by the field's name.
typedef SampleRow = Map<String, Object?>;

final class FieldInfo {
  const FieldInfo(
    this.name,
    this.kind, {
    this.optional = false,
    this.column = true,
    this.target,
  });

  final String name;
  final FieldKind kind;

  /// Whether the field may be null.
  final bool optional;

  /// Whether the list shows it as a column.
  final bool column;

  /// The collection a link holds a key of.
  final SampleCollection? target;
}

final class CollectionInfo {
  const CollectionInfo({
    required this.key,
    required this.autoKey,
    required this.fields,
    required this.sortable,
  });

  /// The field that holds the primary key.
  final String key;

  /// Whether the engine assigns the key, as it does for a collection without
  /// a key field.
  final bool autoKey;

  /// Every field but an automatic key.
  final List<FieldInfo> fields;

  /// The fields the list can sort by: the key and the indexed fields.
  final List<String> sortable;
}

enum SampleCollection {
  organizations(
    CollectionInfo(
      key: 'code',
      autoKey: false,
      fields: <FieldInfo>[
        FieldInfo('code', FieldKind.string),
        FieldInfo('name', FieldKind.string),
        FieldInfo('kind', FieldKind.string),
        FieldInfo('industry', FieldKind.string, optional: true),
        FieldInfo('language', FieldKind.string),
        FieldInfo('founded', FieldKind.integer),
      ],
      sortable: <String>['code', 'name', 'kind', 'founded'],
    ),
  ),
  people(
    CollectionInfo(
      key: 'id',
      autoKey: true,
      fields: <FieldInfo>[
        FieldInfo('color', FieldKind.color),
        FieldInfo('name', FieldKind.string),
        FieldInfo('nickname', FieldKind.string),
        FieldInfo('email', FieldKind.string, optional: true),
        FieldInfo('age', FieldKind.integer),
        FieldInfo('gender', FieldKind.string, column: false),
        FieldInfo('language', FieldKind.string),
        FieldInfo('location', FieldKind.location, optional: true),
        FieldInfo(
          'organization',
          FieldKind.link,
          optional: true,
          target: SampleCollection.organizations,
        ),
        FieldInfo('tags', FieldKind.tags),
        FieldInfo('active', FieldKind.boolean),
        FieldInfo('score', FieldKind.float, column: false),
        FieldInfo('joinedAt', FieldKind.date, column: false),
      ],
      sortable: <String>[
        'id',
        'name',
        'nickname',
        'email',
        'age',
        'language',
        'organization',
        'joinedAt',
      ],
    ),
  ),
  posts(
    CollectionInfo(
      key: 'id',
      autoKey: true,
      fields: <FieldInfo>[
        FieldInfo('author', FieldKind.link, target: SampleCollection.people),
        FieldInfo('title', FieldKind.string),
        FieldInfo('body', FieldKind.string, column: false),
        FieldInfo('language', FieldKind.string),
        FieldInfo('tags', FieldKind.tags, optional: true),
        FieldInfo('likes', FieldKind.integer),
        FieldInfo('pinned', FieldKind.boolean),
        FieldInfo('createdAt', FieldKind.date),
      ],
      sortable: <String>['id', 'author', 'language', 'likes', 'createdAt'],
    ),
  );

  const SampleCollection(this.info);

  /// What the screens know of the collection.
  final CollectionInfo info;
}
