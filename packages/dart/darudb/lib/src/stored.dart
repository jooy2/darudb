/// The schema a file holds, as the collections of a database opened without
/// one: what `Database.storedSchema` gives, whose objects are maps by the
/// names the file stores, and [MapQuery], which queries them.
///
/// The stored schema's record is read in full here (`design/objects.md`,
/// "The stored schema"), where `StoredSchema` in `schema.dart` keeps only
/// what the layouts need: each collection's primary key and indexes, and
/// each field's type through lists, links and embedded objects, make the
/// [FieldSpec]s a generated schema would have, so that the collections work
/// as a generated one's do. The layouts are then made from them as from any
/// other schema.
library;

import 'dart:typed_data';

import 'codec.dart';
import 'query.dart';
import 'schema.dart';

/// A collection whose objects are maps by the names the file stores, as
/// `Database.storedSchema` and `Database.storedCollection` give them.
typedef MapCollection =
    CollectionSchema<Map<String, Object?>, MapQuery, Object>;

/// The query builder of a collection whose objects are maps: a field is
/// named by the name the file stores rather than by a property, and the
/// method says what it holds. A dot separates the fields of an embedded
/// object or of the object a link points to: `stringField('address.city')`.
final class MapQuery extends QueryBuilder<Map<String, Object?>> {
  MapQuery._(this._fields);

  final List<FieldSpec> _fields;

  static List<String> _path(String name) => name.split('.');

  /// A `bool` field.
  BoolField boolField(String name) => BoolField(_path(name));

  /// An `int` field.
  IntField intField(String name) => IntField(_path(name));

  /// A `float` field.
  FloatField floatField(String name) => FloatField(_path(name));

  /// A `string` field.
  StringField stringField(String name) => StringField(_path(name));

  /// A `bytes` field.
  BytesField bytesField(String name) => BytesField(_path(name));

  /// A link, which holds the primary key of the object it points to.
  LinkField linkField(String name) => LinkField(_path(name));

  /// A list whose elements are of type [E].
  ListField<E> listField<E extends Object>(String name) =>
      ListField<E>(_path(name));

  /// A list of strings, whose elements a query can also test by prefix and
  /// suffix.
  StringListField stringListField(String name) => StringListField(_path(name));

  /// An embedded object of the collection, which `update` sets whole as a
  /// map. Its fields are named through the dotted paths of the other
  /// methods.
  EmbeddedField<Map<String, Object?>> objectField(String name) {
    final found = _fields.where((field) => field.name == name).firstOrNull;
    final kind = found?.kind;

    if (kind is! ObjectKind) {
      throw invalidArgument(
        '`$name` is not an embedded object of the collection',
      );
    }

    return EmbeddedField<Map<String, Object?>>([
      name,
    ], kind.embedded as EmbeddedSchema<Map<String, Object?>>);
  }
}

/// The schema whose record is [bytes], as a [Schema] of collections whose
/// objects are maps.
Schema mapSchemaOf(Uint8List bytes) {
  try {
    final record = Reader(bytes).anyFields();
    final raws = [
      for (final raw in record[3] as List) raw as Map<int, Object?>,
    ];
    final names = {for (final raw in raws) raw[1] as int: raw[2] as String};

    return Schema(record[2] as int, [
      for (final raw in raws) _collection(raw, names),
    ]);
  } on TypeError catch (error) {
    throw corrupted('the stored schema does not decode: $error');
  }
}

/// The fields of a stored collection or embedded object, in id order.
List<Map<int, Object?>> _storedFields(Object? list) =>
    [for (final raw in list as List) raw as Map<int, Object?>]
      ..sort((a, b) => (a[1] as int).compareTo(b[1] as int));

MapCollection _collection(Map<int, Object?> raw, Map<int, String> names) {
  final name = raw[2] as String;
  final key = raw[5] as int;
  final auto = raw[6] == true;
  final unique = <int, bool>{
    for (final index in raw[7] as List)
      (index as Map<int, Object?>)[2] as int: index[3] == true,
  };
  final fields = <FieldSpec>[];

  for (final field in _storedFields(raw[3])) {
    final id = field[1] as int;
    final spec = FieldSpec(
      field[2] as String,
      _kind(field[3] as Map<int, Object?>, names),
      optional: field[4] == true,
      defaultValue: field[5],
      index: unique[id] == false,
      unique: unique[id] == true,
      primaryKey: !auto && id == key,
    );

    // A generated schema puts the auto-increment `id` first, in slot 0.
    if (auto && id == key) {
      fields.insert(0, spec);
    } else {
      fields.add(spec);
    }
  }

  return MapCollection(
    name: name,
    fields: fields,
    autoKey: auto,
    writeField: (object, slot, sink) => _write(fields, object, slot, sink),
    read: (source) => _read(fields, source),
    query: () => MapQuery._(fields),
  );
}

Kind _kind(Map<int, Object?> type, Map<int, String> names) => switch (type[1]) {
  1 => const BoolKind(),
  2 => const IntKind(),
  3 => const FloatKind(),
  4 => const StringKind(),
  5 => const BytesKind(),
  6 => LinkKind(
    names[type[2]] ??
        (throw corrupted('the stored schema links to a collection it lacks')),
  ),
  7 => ListKind(_kind(type[3] as Map<int, Object?>, names)),
  8 => ObjectKind(_embedded(type[4], names)),
  _ => throw corrupted('the stored schema names an unknown type'),
};

EmbeddedSchema<Map<String, Object?>> _embedded(
  Object? list,
  Map<int, String> names,
) {
  final fields = [
    for (final field in _storedFields(list))
      FieldSpec(
        field[2] as String,
        _kind(field[3] as Map<int, Object?>, names),
        optional: field[4] == true,
        defaultValue: field[5],
      ),
  ];

  return EmbeddedSchema<Map<String, Object?>>(
    fields: fields,
    writeField: (object, slot, sink) => _write(fields, object, slot, sink),
    read: (source) => _read(fields, source),
  );
}

/// Writes the field in [slot] of [object]. A field the map leaves out, or
/// holds null in, is written as left out, so that it reads as its default,
/// as a generated class's null does. Each map is checked once, at slot 0,
/// for a name the schema lacks, which is refused rather than dropped.
void _write(
  List<FieldSpec> fields,
  Map<String, Object?> object,
  int slot,
  FieldSink sink,
) {
  if (slot == 0) {
    for (final name in object.keys) {
      if (!fields.any((field) => field.name == name)) {
        throw invalidArgument('the schema has no field `$name`');
      }
    }
  }

  final field = fields[slot];
  final value = object[field.name];

  switch (field.kind) {
    case BoolKind():
      sink.boolean(_typed<bool>(value, field));
    case IntKind():
      sink.int64(_typed<int>(value, field));
    case FloatKind():
      sink.float(
        value is int ? value.toDouble() : _typed<double>(value, field),
      );
    case StringKind():
      sink.string(_typed<String>(value, field));
    case BytesKind():
      sink.bytes(_typed<Uint8List>(value, field));
    case LinkKind():
      sink.link(_link(value, field));
    case ListKind(:final element):
      sink.list(_list(element, _typed<List<Object?>>(value, field), field));
    case ObjectKind(:final embedded):
      sink.object(
        _typed<Map<String, Object?>>(value, field),
        embedded as EmbeddedSchema<Map<String, Object?>>,
      );
  }
}

/// [value], checked to be a [T] or null, which `field` holds.
T? _typed<T>(Object? value, FieldSpec field) {
  if (value == null || value is T) {
    return value as T?;
  }

  throw invalidArgument(
    '`${field.name}` holds ${_describe(field.kind)}, not ${value.runtimeType}',
  );
}

/// A link's value: a [Link], or the primary key it holds.
Link<Object?>? _link(Object? value, FieldSpec field) => switch (value) {
  null => null,
  Link() => value,
  int() || String() || Uint8List() => Link<Object?>(value),
  _ => throw invalidArgument(
    '`${field.name}` holds a link or a primary key, not ${value.runtimeType}',
  ),
};

/// A list's elements as the sink writes them: a link's as a [Link], and a
/// float's as a `double` where the list holds an `int`.
List<Object?>? _list(Kind element, List<Object?>? values, FieldSpec field) {
  if (values == null) {
    return null;
  }

  return switch (element) {
    LinkKind() => [for (final value in values) _link(value, field)],
    FloatKind() => [
      for (final value in values) value is int ? value.toDouble() : value,
    ],
    _ => values,
  };
}

Map<String, Object?> _read(List<FieldSpec> fields, FieldSource source) {
  final object = <String, Object?>{
    for (final field in fields) field.name: null,
  };

  while (source.next()) {
    final field = fields[source.slot];

    object[field.name] = switch (field.kind) {
      BoolKind() => source.booleanOrNull(),
      IntKind() => source.int64OrNull(),
      FloatKind() => source.floatOrNull(),
      StringKind() => source.stringOrNull(),
      BytesKind() => source.bytesOrNull(),
      LinkKind() => source.linkOrNull<Object?>(),
      ListKind(element: LinkKind()) => source.linkListOrNull<Object?>(),
      ListKind() => source.listOrNull<Object?>(),
      ObjectKind(:final embedded) => source.objectOrNull(
        embedded as EmbeddedSchema<Map<String, Object?>>,
      ),
    };
  }

  return object;
}

String _describe(Kind kind) => switch (kind) {
  BoolKind() => 'a bool',
  IntKind() => 'an int',
  FloatKind() => 'a double',
  StringKind() => 'a String',
  BytesKind() => 'a Uint8List',
  LinkKind() => 'a link',
  ListKind() => 'a List',
  ObjectKind() => 'a Map',
};
