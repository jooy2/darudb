/// What a schema declares, as the code `darudb_generator` writes declares
/// it: the kinds of fields, a collection's fields and how its objects are
/// written and read, embedded objects, and the schema with its version.
///
/// The fields of a type are numbered in the order the type declares them,
/// the auto-increment `id` first when the collection has one: that number is
/// a field's slot. When a database opens, each collection's fields are
/// matched with the stored schema's by name, into a [Layout]: the stored
/// fields in id order, each with its slot and the bytes of its default.
/// Writing goes down the layout, so a record's fields come out in id order;
/// reading walks the record and the layout together, so a field a record
/// leaves out reads as its default.
library;

import 'dart:typed_data';

import 'codec.dart';
import 'query.dart';

/// The kind of a field: what values it holds.
sealed class Kind {
  const Kind();

  /// The code the stored schema gives the kind.
  int get code;
}

/// `true` or `false`.
final class BoolKind extends Kind {
  const BoolKind();

  @override
  int get code => 1;
}

/// A 64-bit integer.
final class IntKind extends Kind {
  const IntKind();

  @override
  int get code => 2;
}

/// A 64-bit floating-point number.
final class FloatKind extends Kind {
  const FloatKind();

  @override
  int get code => 3;
}

/// UTF-8 text.
final class StringKind extends Kind {
  const StringKind();

  @override
  int get code => 4;
}

/// Any bytes, as a `Uint8List`.
final class BytesKind extends Kind {
  const BytesKind();

  @override
  int get code => 5;
}

/// A link to an object of the collection [collection]: its primary key.
final class LinkKind extends Kind {
  const LinkKind(this.collection);

  final String collection;

  @override
  int get code => 6;
}

/// A list of values of a scalar kind or of links.
final class ListKind extends Kind {
  const ListKind(this.element);

  final Kind element;

  @override
  int get code => 7;
}

/// An embedded object, whose fields [embedded] declares.
final class ObjectKind extends Kind {
  const ObjectKind(this.embedded);

  final EmbeddedSchema<Object?> embedded;

  @override
  int get code => 8;
}

/// A field as a schema declares it.
final class FieldSpec {
  const FieldSpec(
    this.name,
    this.kind, {
    this.optional = false,
    this.defaultValue,
    this.index = false,
    this.unique = false,
    this.primaryKey = false,
  });

  final String name;
  final Kind kind;

  /// Whether the field may be null, which it is when left out.
  final bool optional;

  /// The value a record that leaves the field out holds, or `null` for
  /// none.
  final Object? defaultValue;

  /// Whether queries on the field read an index.
  final bool index;

  /// Whether the index also refuses two objects with the same value.
  final bool unique;

  /// Whether the field is the collection's primary key.
  final bool primaryKey;
}

/// A link to an object of collection [T]: the object's primary key.
///
/// A link to an object that does not exist, or no longer does, is allowed,
/// and reads as the key it holds.
final class Link<T> {
  const Link(this.key);

  /// The primary key of the object linked to: an `int`, a `String` or a
  /// `Uint8List`.
  final Object key;

  @override
  bool operator ==(Object other) =>
      other is Link<T> && _sameKey(other.key, key);

  @override
  int get hashCode =>
      key is Uint8List ? Object.hashAll(key as Uint8List) : key.hashCode;

  @override
  String toString() => 'Link<$T>($key)';
}

bool _sameKey(Object a, Object b) {
  if (a is Uint8List && b is Uint8List) {
    if (a.length != b.length) {
      return false;
    }

    for (var index = 0; index < a.length; index++) {
      if (a[index] != b[index]) {
        return false;
      }
    }

    return true;
  }

  return a == b;
}

/// The fields of an embedded object of type [E], and how one is written and
/// read.
final class EmbeddedSchema<E> {
  const EmbeddedSchema({
    required this.fields,
    required void Function(E object, int slot, FieldSink sink) writeField,
    required E Function(FieldSource source) read,
  }) : _writeField = writeField,
       _read = read;

  /// The fields, in slot order.
  final List<FieldSpec> fields;
  final void Function(E object, int slot, FieldSink sink) _writeField;
  final E Function(FieldSource source) _read;

  void _write(Object? object, int slot, FieldSink sink) =>
      _writeField(object as E, slot, sink);

  Object? _readOne(FieldSource source) => _read(source);
}

/// A collection whose objects are of type [T], whose queries [Q] builds and
/// whose primary key is of type [K]: what `darudb_generator` makes for a
/// class annotated `@Collection()`, as a constant such as `userSchema`.
final class CollectionSchema<T, Q extends QueryBuilder<T>, K extends Object> {
  const CollectionSchema({
    required this.name,
    required this.fields,
    required this.autoKey,
    required void Function(T object, int slot, FieldSink sink) writeField,
    required T Function(FieldSource source) read,
    required Q Function() query,
  }) : _writeField = writeField,
       _read = read,
       _query = query;

  /// The collection's name.
  final String name;

  /// The fields, in slot order: the auto-increment `id` first when
  /// [autoKey] is set.
  final List<FieldSpec> fields;

  /// Whether the engine assigns the primary key, an `int` field `id`.
  final bool autoKey;

  final void Function(T object, int slot, FieldSink sink) _writeField;
  final T Function(FieldSource source) _read;
  final Q Function() _query;

  /// A new query builder.
  Q newQuery() => _query();

  /// Writes [object] as a record, whose fields lie as [layout] says.
  void encode(T object, Writer writer, Layout layout, FieldSink sink) {
    sink._encode(writer, layout, (slot) => _writeField(object, slot, sink));
  }

  /// The object whose record is [bytes] from [start] to [end].
  T decode(Uint8List bytes, int start, int end, Layout layout) =>
      _read(FieldSource(Reader(bytes, start, end), layout));

  /// The primary key of [key], checked to be of the collection's key type.
  K keyOf(Object? value) {
    if (value is K) {
      return value;
    }

    throw invalidArgument('a primary key of `$name` is a $K, not $value');
  }
}

/// A schema: a version, from 1 up, and its collections.
final class Schema {
  const Schema(this.version, this.collections);

  final int version;
  final List<CollectionSchema<Object?, QueryBuilder<Object?>, Object>>
  collections;
}

/// Writes the fields of one object, a field at a time, as the generated code
/// tells it each field's value.
final class FieldSink {
  Writer? _writer;
  int _id = 0;
  bool _written = false;
  Layout? _nested;

  void _encode(Writer writer, Layout layout, void Function(int slot) write) {
    final saved = (_writer, _id, _written, _nested);
    final most = layout.fields.length;

    _writer = writer;

    final start = writer.reserveCount(most);
    var written = 0;

    for (final field in layout.fields) {
      _id = field.id;
      _written = false;
      _nested = field.nested;
      write(field.slot);

      if (_written) {
        written++;
      }
    }

    writer.setCount(start, most, written);
    _writer = saved.$1;
    _id = saved.$2;
    _written = saved.$3;
    _nested = saved.$4;
  }

  /// Writes the field's id, before its value, and counts the field.
  Writer _begin() {
    final writer = _writer!;

    _written = true;
    writer.varint(_id);

    return writer;
  }

  void boolean(bool? value) {
    if (value != null) {
      _begin().byte(value ? Tag.trueValue : Tag.falseValue);
    }
  }

  void int64(int? value) {
    if (value != null) {
      _begin()
        ..byte(Tag.int64)
        ..int64(value);
    }
  }

  void float(double? value) {
    if (value != null) {
      _begin()
        ..byte(Tag.float)
        ..float(value);
    }
  }

  void string(String? value) {
    if (value != null) {
      _begin()
        ..byte(Tag.string)
        ..string(value);
    }
  }

  void bytes(Uint8List? value) {
    if (value != null) {
      _begin()
        ..byte(Tag.bytes)
        ..bytesOf(value);
    }
  }

  void link(Link<Object?>? value) {
    if (value != null) {
      writeKey(_begin()..byte(Tag.link), value.key);
    }
  }

  /// A list, whose elements are `bool`, `int`, `double`, `String`,
  /// `Uint8List` or [Link] values.
  void list(List<Object?>? values) {
    if (values == null) {
      return;
    }

    final writer = _begin()
      ..byte(Tag.list)
      ..varint(values.length);

    for (final value in values) {
      writeElement(writer, value);
    }
  }

  /// An embedded object, written by [schema].
  void object<E>(E? value, EmbeddedSchema<E> schema) {
    if (value == null) {
      return;
    }

    final layout = _nested;

    if (layout == null) {
      throw invalidArgument('an embedded object for a field that holds none');
    }

    final writer = _begin();
    final mark = writer.open();

    _encode(writer, layout, (slot) => schema._write(value, slot, this));
    writer.close(mark);
  }
}

/// Writes [value] as the embedded object [schema] declares, tag first, with
/// its fields as [layout] says they lie: the value of an embedded field that
/// `update` sets.
void writeEmbedded<E>(
  Writer writer,
  E value,
  EmbeddedSchema<E> schema,
  Layout layout,
) {
  final sink = FieldSink();
  final mark = writer.open();

  sink._encode(writer, layout, (slot) => schema._write(value, slot, sink));
  writer.close(mark);
}

/// Writes a list's element, tag first.
void writeElement(Writer writer, Object? value) {
  switch (value) {
    case bool():
      writer.byte(value ? Tag.trueValue : Tag.falseValue);
    case int():
      writer
        ..byte(Tag.int64)
        ..int64(value);
    case double():
      writer
        ..byte(Tag.float)
        ..float(value);
    case String():
      writer
        ..byte(Tag.string)
        ..string(value);
    case Uint8List():
      writer
        ..byte(Tag.bytes)
        ..bytesOf(value);
    case Link<Object?>():
      writeKey(writer..byte(Tag.link), value.key);
    default:
      throw invalidArgument(
        'a list holds `bool`, `int`, `double`, `String`, `Uint8List` and '
        '`Link` values, not $value',
      );
  }
}

/// Writes a primary key as a record's value, tag first: an `int`, a
/// `String` or a `Uint8List`.
void writeKey(Writer writer, Object? key) {
  switch (key) {
    case int():
      writer
        ..byte(Tag.int64)
        ..int64(key);
    case String():
      writer
        ..byte(Tag.string)
        ..string(key);
    case Uint8List():
      writer
        ..byte(Tag.bytes)
        ..bytesOf(key);
    default:
      throw invalidArgument(
        'a primary key is an `int`, a `String` or a `Uint8List`, not $key',
      );
  }
}

/// Gives the fields of one record, a field at a time, to the generated code
/// that reads them into an object: the value the record holds, or the
/// field's default or null when the record leaves it out, as a record
/// written before the field existed does. Every field of the type comes
/// once.
final class FieldSource {
  FieldSource(this._reader, this._layout) {
    _left = _reader.count(2);
  }

  final Reader _reader;
  final Layout _layout;
  late int _left;
  int _last = -1;
  int _pendingId = -1;
  int _pendingAt = 0;
  int _next = 0;

  int _slot = 0;
  Uint8List _bytes = _none;
  int _start = 0;
  int _end = 0;
  Layout? _nested;

  static final Uint8List _none = Uint8List(0);

  /// The slot of the field [next] moved to.
  int get slot => _slot;

  /// Whether the field's value is null.
  bool get isNull => _start == _end;

  /// Moves to the next field, and returns whether there was one.
  bool next() {
    final reader = _reader;

    while (true) {
      if (_next >= _layout.fields.length) {
        _finish();

        return false;
      }

      final field = _layout.fields[_next];

      if (_pendingId < 0 && _left > 0) {
        final id = reader.varint();

        if (id <= _last) {
          throw corrupted("a record's field ids are out of order");
        }

        _last = id;
        _left--;
        _pendingId = id;
        _pendingAt = reader.at;
      }

      if (_pendingId >= 0 && _pendingId < field.id) {
        // A field the schema no longer has.
        reader
          ..at = _pendingAt
          ..skip();
        _pendingId = -1;

        continue;
      }

      _next++;
      _slot = field.slot;
      _nested = field.nested;

      if (_pendingId == field.id) {
        reader
          ..at = _pendingAt
          ..skip();
        _bytes = reader.bytes;
        _start = _pendingAt;
        _end = reader.at;
        _pendingId = -1;

        return true;
      }

      // The record leaves the field out.
      final fallback = field.defaultBytes;

      if (fallback != null) {
        _bytes = fallback;
        _start = 0;
        _end = fallback.length;
      } else if (field.optional) {
        _bytes = _none;
        _start = 0;
        _end = 0;
      } else {
        throw corrupted('a record lacks a required field');
      }

      return true;
    }
  }

  /// Steps over the fields after the last one the layout has, which belong
  /// to fields the schema no longer has, and checks nothing follows them.
  void _finish() {
    final reader = _reader;

    if (_pendingId >= 0) {
      reader
        ..at = _pendingAt
        ..skip();
      _pendingId = -1;
    }

    while (_left > 0) {
      final id = reader.varint();

      if (id <= _last) {
        throw corrupted("a record's field ids are out of order");
      }

      _last = id;
      _left--;
      reader.skip();
    }

    if (!reader.isDone) {
      throw corrupted('a record has bytes after its last field');
    }
  }

  Reader _value(int tag) {
    if (_start == _end) {
      throw corrupted('a record holds null for a required field');
    }

    final reader = Reader(_bytes, _start, _end);

    if (reader.byte() != tag) {
      throw corrupted('a record holds a value of another type than its field');
    }

    return reader;
  }

  bool boolean() {
    if (_start == _end) {
      throw corrupted('a record holds null for a required field');
    }

    return switch (_bytes[_start]) {
      Tag.trueValue => true,
      Tag.falseValue => false,
      _ => throw corrupted(
        'a record holds a value of another type than its field',
      ),
    };
  }

  bool? booleanOrNull() => isNull ? null : boolean();

  int int64() => _value(Tag.int64).int64();

  int? int64OrNull() => isNull ? null : int64();

  double float() => _value(Tag.float).float();

  double? floatOrNull() => isNull ? null : float();

  String string() => _value(Tag.string).string();

  String? stringOrNull() => isNull ? null : string();

  Uint8List bytes() => Uint8List.fromList(_value(Tag.bytes).counted());

  Uint8List? bytesOrNull() => isNull ? null : bytes();

  Link<T> link<T>() => Link<T>(_key(_value(Tag.link)));

  Link<T>? linkOrNull<T>() => isNull ? null : link<T>();

  /// A list, each element as [E]: `bool`, `int`, `double`, `String`,
  /// `Uint8List` or a [Link].
  List<E> list<E>() {
    final reader = _value(Tag.list);
    final values = <E>[];

    for (var left = reader.count(); left > 0; left--) {
      values.add(_element(reader) as E);
    }

    return values;
  }

  List<E>? listOrNull<E>() => isNull ? null : list<E>();

  /// A list of links to objects of [T].
  List<Link<T>> linkList<T>() {
    final reader = _value(Tag.list);
    final links = <Link<T>>[];

    for (var left = reader.count(); left > 0; left--) {
      if (reader.byte() != Tag.link) {
        throw corrupted('a list of links holds something that is not a link');
      }

      links.add(Link<T>(_key(reader)));
    }

    return links;
  }

  List<Link<T>>? linkListOrNull<T>() => isNull ? null : linkList<T>();

  /// An embedded object, read by [schema].
  E object<E>(EmbeddedSchema<E> schema) {
    final layout = _nested;

    if (layout == null) {
      throw corrupted(
        'an embedded object is read from a field that holds none',
      );
    }

    final record = _value(Tag.object).counted();

    return schema._readOne(FieldSource(Reader(record), layout)) as E;
  }

  E? objectOrNull<E>(EmbeddedSchema<E> schema) =>
      isNull ? null : object(schema);

  static Object _element(Reader reader) {
    switch (reader.byte()) {
      case Tag.trueValue:
        return true;
      case Tag.falseValue:
        return false;
      case Tag.int64:
        return reader.int64();
      case Tag.float:
        return reader.float();
      case Tag.string:
        return reader.string();
      case Tag.bytes:
        return Uint8List.fromList(reader.counted());
      case Tag.link:
        return Link<Object?>(_key(reader));
      default:
        throw corrupted('a list holds a value of a type no list holds');
    }
  }

  static Object _key(Reader reader) {
    switch (reader.byte()) {
      case Tag.int64:
        return reader.int64();
      case Tag.string:
        return reader.string();
      case Tag.bytes:
        return Uint8List.fromList(reader.counted());
      default:
        throw corrupted('a link holds something that is not a key');
    }
  }
}

/// How the fields of a type lie in a stored collection or embedded object.
final class Layout {
  Layout._(this.fields);

  /// The stored fields, in id order.
  final List<LaidField> fields;

  /// The stored fields, by slot.
  late final List<LaidField?> fieldOfSlot = () {
    final bySlot = List<LaidField?>.filled(fields.length, null);

    for (final field in fields) {
      bySlot[field.slot] = field;
    }

    return bySlot;
  }();
}

/// A stored field: its id, the slot the type gives it, and what reading it
/// needs when a record leaves it out.
final class LaidField {
  LaidField(this.id, this.slot, this.optional, this.defaultBytes, this.nested);

  final int id;
  final int slot;
  final bool optional;

  /// The default as a record holds it, tag first.
  final Uint8List? defaultBytes;
  final Layout? nested;
}

/// A field of the stored schema, as the layout needs it.
final class StoredField {
  StoredField(
    this.id,
    this.name,
    this.kindCode,
    this.optional,
    this.defaultValue,
    this.fields,
  );

  final int id;
  final String name;
  final int kindCode;
  final bool optional;

  /// The default, as [Reader.any] reads it, or `null` for none.
  final StoredDefault? defaultValue;

  /// An embedded object's fields.
  final List<StoredField>? fields;
}

/// The stored schema, as the layouts need it: its version, and each
/// collection's fields by name.
final class StoredSchema {
  StoredSchema(this.version, this.collections);

  final int version;
  final Map<String, List<StoredField>> collections;

  /// Reads the stored schema's record (`design/objects.md`, "The stored
  /// schema").
  static StoredSchema decode(Uint8List bytes) {
    final record = Reader(bytes).anyFields();

    List<StoredField> fieldsOf(Object? list) {
      if (list is! List) {
        throw corrupted(
          'the stored schema has a field list that is not a list',
        );
      }

      final fields = <StoredField>[];

      for (final raw in list) {
        if (raw is! Map<int, Object?>) {
          throw corrupted(
            'the stored schema has a field that is not an object',
          );
        }

        final type = raw[3];

        if (type is! Map<int, Object?>) {
          throw corrupted('the stored schema has a field without a type');
        }

        final embedded = type[4];

        fields.add(
          StoredField(
            raw[1] as int,
            raw[2] as String,
            type[1] as int,
            raw[4] == true,
            raw.containsKey(5) ? StoredDefault(raw[5]) : null,
            embedded == null ? null : fieldsOf(embedded),
          ),
        );
      }

      fields.sort((a, b) => a.id.compareTo(b.id));

      return fields;
    }

    try {
      final collections = <String, List<StoredField>>{};

      for (final raw in record[3] as List) {
        final collection = raw as Map<int, Object?>;

        collections[collection[2] as String] = fieldsOf(collection[3]);
      }

      return StoredSchema(record[2] as int, collections);
    } on TypeError catch (error) {
      throw corrupted('the stored schema does not decode: $error');
    }
  }

  /// How [schema]'s fields lie in the stored collection of the same name.
  Layout layoutOf(
    CollectionSchema<Object?, QueryBuilder<Object?>, Object> schema,
  ) {
    final stored = collections[schema.name];

    if (stored == null) {
      throw invalidArgument(
        'the database holds no collection `${schema.name}`',
      );
    }

    return _layout(schema.fields, stored, schema.name);
  }

  static Layout _layout(
    List<FieldSpec> declared,
    List<StoredField> stored,
    String owner,
  ) {
    if (declared.length != stored.length) {
      throw invalidArgument(
        '`$owner` declares ${declared.length} fields and the database holds '
        '${stored.length}',
      );
    }

    final fields = <LaidField>[];

    for (final field in stored) {
      final slot = declared.indexWhere((each) => each.name == field.name);

      if (slot < 0) {
        throw invalidArgument(
          '`$owner` has a field `${field.name}` the type does not',
        );
      }

      final spec = declared[slot];
      final kind = spec.kind;
      Uint8List? defaultBytes;
      final fallback = field.defaultValue;

      if (fallback != null) {
        final writer = Writer(32);

        writeAny(writer, fallback.value);
        defaultBytes = Uint8List.fromList(writer.written);
      }

      fields.add(
        LaidField(
          field.id,
          slot,
          field.optional,
          defaultBytes,
          kind is ObjectKind && field.fields != null
              ? _layout(
                  kind.embedded.fields,
                  field.fields!,
                  '$owner.${field.name}',
                )
              : null,
        ),
      );
    }

    return Layout._(fields);
  }
}

/// A default the stored schema holds, apart from no default at all.
final class StoredDefault {
  const StoredDefault(this.value);

  final Object? value;
}

/// Writes a value as [Reader.any] reads it, tag first.
void writeAny(Writer writer, Object? value) {
  switch (value) {
    case Map<int, Object?>():
      final mark = writer.open();
      final ids = value.keys.toList()..sort();

      writer.varint(ids.length);

      for (final id in ids) {
        writer.varint(id);
        writeAny(writer, value[id]);
      }

      writer.close(mark);
    case List<Object?>():
      writer
        ..byte(Tag.list)
        ..varint(value.length);

      for (final element in value) {
        writeAny(writer, element);
      }
    case RawLink():
      writer.byte(Tag.link);
      writeAny(writer, value.key);
    default:
      writeElement(writer, value);
  }
}

/// The record of a declared schema, encoded as the file stores a schema,
/// with ids given in the order of declaration. The engine reads it with
/// `Schema::decode` and gives the file's ids its own way.
Uint8List encodeSchema(Schema schema) => _SchemaEncoder(schema).encode();

final class _SchemaEncoder {
  _SchemaEncoder(this.schema) {
    for (final (index, collection) in schema.collections.indexed) {
      if (ids.containsKey(collection.name)) {
        throw invalidArgument(
          'the collection `${collection.name}` is declared twice',
        );
      }

      ids[collection.name] = index + 1;
    }
  }

  final Schema schema;
  final Map<String, int> ids = {};
  final Writer writer = Writer(1024);
  int nextIndex = 1;

  Uint8List encode() {
    writer
      ..varint(5)
      ..varint(1)
      ..byte(Tag.int64)
      ..int64(1)
      ..varint(2)
      ..byte(Tag.int64)
      ..int64(schema.version)
      ..varint(3)
      ..byte(Tag.list)
      ..varint(schema.collections.length);

    for (final collection in schema.collections) {
      collectionEntry(collection);
    }

    writer
      ..varint(4)
      ..byte(Tag.int64)
      ..int64(schema.collections.length + 1)
      ..varint(5)
      ..byte(Tag.int64)
      ..int64(nextIndex);

    return Uint8List.fromList(writer.written);
  }

  void collectionEntry(
    CollectionSchema<Object?, QueryBuilder<Object?>, Object> collection,
  ) {
    final declared = collection.fields;
    final keys = declared.where((field) => field.primaryKey).length;

    if (collection.autoKey) {
      // The auto-increment `id` is declared like any other field, first, so
      // that it gets id 1 as the engine numbers it.
      if (declared.isEmpty ||
          declared.first.name != 'id' ||
          declared.first.kind is! IntKind ||
          keys != 0) {
        throw invalidArgument(
          '`${collection.name}` is keyed by an auto-increment, whose `int` '
          'field `id` comes first, and has no other key',
        );
      }
    } else if (keys != 1) {
      throw invalidArgument(
        '`${collection.name}` has $keys primary keys, not one',
      );
    }

    final fields = [
      for (final (index, field) in declared.indexed) (index + 1, field),
    ];
    final key = collection.autoKey
        ? 1
        : fields.firstWhere((entry) => entry.$2.primaryKey).$1;
    final indexes = [
      for (final (id, spec) in fields)
        if (spec.index || spec.unique) (nextIndex++, id, spec.unique),
    ];
    final mark = writer.open();

    writer
      ..varint(7)
      ..varint(1)
      ..byte(Tag.int64)
      ..int64(ids[collection.name]!)
      ..varint(2)
      ..byte(Tag.string)
      ..string(collection.name)
      ..varint(3);
    fieldList(fields, collection.name, embedded: false);
    writer
      ..varint(4)
      ..byte(Tag.int64)
      ..int64(fields.length + 1)
      ..varint(5)
      ..byte(Tag.int64)
      ..int64(key)
      ..varint(6)
      ..byte(collection.autoKey ? Tag.trueValue : Tag.falseValue)
      ..varint(7)
      ..byte(Tag.list)
      ..varint(indexes.length);

    for (final (id, field, unique) in indexes) {
      final index = writer.open();

      writer
        ..varint(3)
        ..varint(1)
        ..byte(Tag.int64)
        ..int64(id)
        ..varint(2)
        ..byte(Tag.int64)
        ..int64(field)
        ..varint(3)
        ..byte(unique ? Tag.trueValue : Tag.falseValue);
      writer.close(index);
    }

    writer.close(mark);
  }

  void fieldList(
    List<(int, FieldSpec)> fields,
    String where, {
    required bool embedded,
  }) {
    writer
      ..byte(Tag.list)
      ..varint(fields.length);

    for (final (id, spec) in fields) {
      final path = '$where.${spec.name}';

      if (embedded && (spec.index || spec.unique || spec.primaryKey)) {
        throw invalidArgument(
          '`$path` is inside an embedded object, where no field is a key or '
          'indexed',
        );
      }

      final mark = writer.open();
      final hasDefault = spec.defaultValue != null;

      writer
        ..varint(hasDefault ? 5 : 4)
        ..varint(1)
        ..byte(Tag.int64)
        ..int64(id)
        ..varint(2)
        ..byte(Tag.string)
        ..string(spec.name)
        ..varint(3);
      kindEntry(spec.kind, path);
      writer
        ..varint(4)
        ..byte(spec.optional ? Tag.trueValue : Tag.falseValue);

      if (hasDefault) {
        writer.varint(5);
        writeDefault(writer, spec.kind, spec.defaultValue, path);
      }

      writer.close(mark);
    }
  }

  void kindEntry(Kind kind, String where) {
    final mark = writer.open();

    switch (kind) {
      case LinkKind(:final collection):
        final target = ids[collection];

        if (target == null) {
          throw invalidArgument(
            '`$where` links to `$collection`, which is not a collection',
          );
        }

        writer
          ..varint(2)
          ..varint(1)
          ..byte(Tag.int64)
          ..int64(kind.code)
          ..varint(2)
          ..byte(Tag.int64)
          ..int64(target);
      case ListKind(:final element):
        writer
          ..varint(2)
          ..varint(1)
          ..byte(Tag.int64)
          ..int64(kind.code)
          ..varint(3);
        kindEntry(element, where);
      case ObjectKind(:final embedded):
        writer
          ..varint(3)
          ..varint(1)
          ..byte(Tag.int64)
          ..int64(kind.code)
          ..varint(4);
        fieldList(
          [
            for (final (index, field) in embedded.fields.indexed)
              (index + 1, field),
          ],
          where,
          embedded: true,
        );
        writer
          ..varint(5)
          ..byte(Tag.int64)
          ..int64(embedded.fields.length + 1);
      default:
        writer
          ..varint(1)
          ..varint(1)
          ..byte(Tag.int64)
          ..int64(kind.code);
    }

    writer.close(mark);
  }
}

/// Writes a field's default, as a value of the field's kind.
void writeDefault(Writer writer, Kind kind, Object? value, String where) {
  final fits = switch (kind) {
    BoolKind() => value is bool,
    IntKind() => value is int,
    FloatKind() => value is double || value is int,
    StringKind() => value is String,
    BytesKind() => value is Uint8List,
    ListKind() => value is List,
    _ => false,
  };

  if (!fits) {
    throw invalidArgument('the default of `$where` does not have its type');
  }

  if (kind is FloatKind && value is int) {
    writer
      ..byte(Tag.float)
      ..float(value.toDouble());
  } else if (value is List) {
    writer
      ..byte(Tag.list)
      ..varint(value.length);

    for (final element in value) {
      writeElement(writer, element);
    }
  } else {
    writeElement(writer, value);
  }
}
