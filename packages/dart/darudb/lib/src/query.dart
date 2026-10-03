/// Queries built in Dart: a typed field object for each field, whose methods
/// make [Condition]s combined with `&`, `|` and `~`, and a [QueryBuilder]
/// that takes a filter, a sort, an offset and a limit. A query crosses into
/// the engine as its IR (`design/objects.md`, "The IR"), the same tree the
/// query language parses into.
library;

import 'dart:typed_data';

import 'codec.dart';
import 'errors.dart';
import 'schema.dart';

// The operators of the IR.
const int _and = 1;
const int _or = 2;
const int _not = 3;
const int _equal = 4;
const int _notEqual = 5;
const int _less = 6;
const int _atMost = 7;
const int _greater = 8;
const int _atLeast = 9;
const int _between = 10;
const int _in = 11;
const int _contains = 12;
const int _startsWith = 13;
const int _endsWith = 14;
const int _isNull = 15;

/// How deeply a filter may nest, as the engine holds every query to.
const int _maxFilterDepth = 24;

/// A condition of a query's filter. Combine conditions with `&` (both hold),
/// `|` (either holds) and `~` (it does not hold).
final class Condition {
  const Condition._(this._op, this._path, this._values, this._terms);

  final int _op;
  final List<String> _path;
  final List<Object?> _values;
  final List<Condition> _terms;

  /// Holds when both hold.
  Condition operator &(Condition other) =>
      Condition._(_and, const [], const [], [
        ...(_op == _and ? _terms : [this]),
        ...(other._op == _and ? other._terms : [other]),
      ]);

  /// Holds when either holds.
  Condition operator |(Condition other) =>
      Condition._(_or, const [], const [], [
        ...(_op == _or ? _terms : [this]),
        ...(other._op == _or ? other._terms : [other]),
      ]);

  /// Holds when this does not.
  Condition operator ~() => Condition._(_not, const [], const [], [this]);
}

Condition _test(int op, List<String> path, [List<Object?> values = const []]) =>
    Condition._(op, path, values, const []);

/// A change of one field, for `update`, which [ValueField.set] and the
/// `set` of other fields make.
final class Change {
  const Change._(this._name, this._write);

  final String _name;

  /// Writes the value, tag first, given how the fields of an embedded
  /// object lie in the stored field, or `null` for null.
  final void Function(Writer writer, Layout? nested)? _write;
}

/// A field of a collection or an embedded object, as a query names it.
abstract base class Field {
  const Field(this.path);

  /// The names from the collection to the field, through embedded objects
  /// and links.
  final List<String> path;

  /// Holds when the field is null.
  Condition isNull() => _test(_isNull, path);

  /// Holds when the field is not null.
  Condition isNotNull() => ~isNull();
}

/// A field whose values are of type [V], compared and set as such.
abstract base class ValueField<V extends Object> extends Field {
  const ValueField(super.path);

  void _write(Writer writer, V value);

  /// Holds when the field equals [value].
  Condition equals(V value) => _test(_equal, path, [value]);

  /// Holds when the field does not equal [value], or is null.
  Condition notEquals(V value) => _test(_notEqual, path, [value]);

  /// Holds when the field equals one of [values].
  Condition isIn(List<V> values) => _test(_in, path, values);

  /// The change that sets the field to [value], or makes it null, for
  /// `update`. Null gives a required field with a default its default.
  Change set(V? value) {
    if (path.length != 1) {
      throw invalidArgument(
        '`update` sets the fields of the object itself, not `${path.join('.')}`',
      );
    }

    return Change._(
      path.single,
      value == null ? null : (writer, _) => _write(writer, value),
    );
  }
}

/// A field whose values have an order: comparisons and ranges.
abstract base class OrderedField<V extends Object> extends ValueField<V> {
  const OrderedField(super.path);

  /// Holds when the field is below [value].
  Condition lessThan(V value) => _test(_less, path, [value]);

  /// Holds when the field is [value] or below.
  Condition atMost(V value) => _test(_atMost, path, [value]);

  /// Holds when the field is above [value].
  Condition greaterThan(V value) => _test(_greater, path, [value]);

  /// Holds when the field is [value] or above.
  Condition atLeast(V value) => _test(_atLeast, path, [value]);

  /// Holds when the field is from [low] to [high], both included.
  Condition between(V low, V high) => _test(_between, path, [low, high]);
}

/// A `bool` field.
final class BoolField extends ValueField<bool> {
  const BoolField(super.path);

  @override
  void _write(Writer writer, bool value) =>
      writer.byte(value ? Tag.trueValue : Tag.falseValue);
}

/// An `int` field.
final class IntField extends OrderedField<int> {
  const IntField(super.path);

  @override
  void _write(Writer writer, int value) => writer
    ..byte(Tag.int64)
    ..int64(value);
}

/// A `double` field.
final class FloatField extends OrderedField<double> {
  const FloatField(super.path);

  @override
  void _write(Writer writer, double value) => writer
    ..byte(Tag.float)
    ..float(value);
}

/// A `String` field. Strings compare by their UTF-8 bytes.
final class StringField extends OrderedField<String> {
  const StringField(super.path);

  @override
  void _write(Writer writer, String value) => writer
    ..byte(Tag.string)
    ..string(value);

  /// Holds when the field contains [text].
  Condition contains(String text) => _test(_contains, path, [text]);

  /// Holds when the field starts with [text].
  Condition startsWith(String text) => _test(_startsWith, path, [text]);

  /// Holds when the field ends with [text].
  Condition endsWith(String text) => _test(_endsWith, path, [text]);
}

/// A `Uint8List` field.
final class BytesField extends OrderedField<Uint8List> {
  const BytesField(super.path);

  @override
  void _write(Writer writer, Uint8List value) => writer
    ..byte(Tag.bytes)
    ..bytesOf(value);
}

/// A list field. A condition on a list holds when it holds for any element.
base class ListField<E extends Object> extends Field {
  const ListField(super.path);

  /// Holds when the list has an element equal to [value].
  Condition contains(E value) => _test(_contains, path, [value]);

  /// Holds when an element equals one of [values].
  Condition containsAny(List<E> values) => _test(_in, path, values);

  /// The change that sets the list, or makes it null, for `update`.
  Change set(List<E>? values) {
    if (path.length != 1) {
      throw invalidArgument(
        '`update` sets the fields of the object itself, not `${path.join('.')}`',
      );
    }

    return Change._(
      path.single,
      values == null
          ? null
          : (writer, _) {
              writer
                ..byte(Tag.list)
                ..varint(values.length);

              for (final value in values) {
                writeElement(writer, value);
              }
            },
    );
  }
}

/// A list of strings, whose elements can also be tested for how they start
/// and end.
final class StringListField extends ListField<String> {
  const StringListField(super.path);

  /// Holds when an element starts with [text].
  Condition anyStartsWith(String text) => _test(_startsWith, path, [text]);

  /// Holds when an element ends with [text].
  Condition anyEndsWith(String text) => _test(_endsWith, path, [text]);
}

/// A link field: the key of the object linked to, compared as such. The
/// code `darudb_generator` writes adds the linked collection's fields, which
/// a condition reads through the link.
base class LinkField extends Field {
  const LinkField(super.path);

  /// Holds when the link holds [key].
  Condition equals(Object key) => _test(_equal, path, [key]);

  /// Holds when the link does not hold [key], or is null.
  Condition notEquals(Object key) => _test(_notEqual, path, [key]);

  /// Holds when the link holds one of [keys].
  Condition isIn(List<Object> keys) => _test(_in, path, keys);

  /// The change that sets the link, or makes it null, for `update`.
  Change set(Link<Object?>? link) {
    if (path.length != 1) {
      throw invalidArgument(
        '`update` sets the fields of the object itself, not `${path.join('.')}`',
      );
    }

    return Change._(
      path.single,
      link == null
          ? null
          : (writer, _) => writeKey(writer..byte(Tag.link), link.key),
    );
  }
}

/// A field that holds an embedded object of type [E]. The code
/// `darudb_generator` writes adds the embedded object's fields.
base class EmbeddedField<E> extends Field {
  const EmbeddedField(super.path, this._schema);

  final EmbeddedSchema<E> _schema;

  /// The change that replaces the embedded object whole with [value], or
  /// makes it null, for `update`. Null gives a required field with a
  /// default its default.
  Change set(E? value) {
    if (path.length != 1) {
      throw invalidArgument(
        '`update` sets the fields of the object itself, not `${path.join('.')}`',
      );
    }

    return Change._(
      path.single,
      value == null
          ? null
          : (writer, nested) {
              if (nested == null) {
                throw invalidArgument(
                  '`${path.single}` does not hold an embedded object in the '
                  'stored schema',
                );
              }

              writeEmbedded(writer, value, _schema, nested);
            },
    );
  }
}

/// The parts of a query: a filter, a sort, an offset and a limit. The code
/// `darudb_generator` writes subclasses it with a getter for each field.
abstract base class QueryBuilder<T> {
  QueryBuilder();

  Condition? _filter;
  final List<(List<String>, bool)> _sort = [];
  int _offset = 0;
  int? _limit;

  /// Keeps the objects [condition] holds for. A second `where` keeps those
  /// both hold for.
  QueryBuilder<T> where(Condition condition) {
    final filter = _filter;

    _filter = filter == null ? condition : filter & condition;

    return this;
  }

  /// Sorts by [field], ascending unless [descending]. A second `sortBy`
  /// sorts the objects that the first leaves equal. Null sorts first
  /// ascending and last descending; without a sort, objects come in primary
  /// key order.
  QueryBuilder<T> sortBy(Field field, {bool descending = false}) {
    _sort.add((field.path, descending));

    return this;
  }

  /// Skips the first [count] objects.
  QueryBuilder<T> offset(int count) {
    if (count < 0) {
      throw invalidArgument('an offset is not negative');
    }

    _offset = count;

    return this;
  }

  /// Keeps at most [count] objects.
  QueryBuilder<T> limit(int count) {
    if (count < 0) {
      throw invalidArgument('a limit is not negative');
    }

    _limit = count;

    return this;
  }
}

/// Writes the changes of an update as a record: the fields they set, by id,
/// a null one with the tag that makes it null.
void encodeChanges(
  Writer writer,
  List<Change> changes,
  Layout layout,
  List<String> names,
) {
  final byId = <int, (Change, LaidField)>{};

  for (final change in changes) {
    final slot = names.indexOf(change._name);
    final field = slot < 0 ? null : layout.fieldOfSlot[slot];

    if (field == null) {
      throw invalidArgument('`${change._name}` is not a field');
    }

    byId[field.id] = (change, field);
  }

  final ids = byId.keys.toList()..sort();

  writer.varint(ids.length);

  for (final id in ids) {
    final (change, field) = byId[id]!;
    final write = change._write;

    writer.varint(id);

    if (write == null) {
      writer.byte(Tag.nil);
    } else {
      write(writer, field.nested);
    }
  }
}

void _writePath(Writer writer, List<String> path) {
  writer
    ..byte(Tag.list)
    ..varint(path.length);

  for (final name in path) {
    writer
      ..byte(Tag.string)
      ..string(name);
  }
}

/// Writes a value a query compares with, tag first.
void writeQueryValue(Writer writer, Object? value) {
  switch (value) {
    case Link<Object?>():
      writeKey(writer, value.key);
    case null:
      throw DaruException(
        'INVALID_QUERY',
        'a query compares with values, not null: use isNull()',
      );
    default:
      writeElement(writer, value);
  }
}

void _writeExpression(Writer writer, Condition node, int depth) {
  if (depth > _maxFilterDepth) {
    throw DaruException(
      'INVALID_QUERY',
      'the filter nests more than $_maxFilterDepth levels deep',
    );
  }

  final mark = writer.open();

  if (node._op == _and || node._op == _or || node._op == _not) {
    final terms = node._terms;

    writer
      ..varint(terms.isEmpty ? 1 : 2)
      ..varint(1)
      ..byte(Tag.int64)
      ..int64(node._op);

    if (terms.isNotEmpty) {
      writer
        ..varint(4)
        ..byte(Tag.list)
        ..varint(terms.length);

      for (final term in terms) {
        _writeExpression(writer, term, depth + 1);
      }
    }
  } else {
    final values = node._values;

    writer
      ..varint(values.isEmpty ? 2 : 3)
      ..varint(1)
      ..byte(Tag.int64)
      ..int64(node._op)
      ..varint(2);
    _writePath(writer, node._path);

    if (values.isNotEmpty) {
      writer
        ..varint(3)
        ..byte(Tag.list)
        ..varint(values.length);

      for (final value in values) {
        writeQueryValue(writer, value);
      }
    }
  }

  writer.close(mark);
}

/// Writes the IR of [query] on [collection] into [writer]: its filter, sort,
/// offset and limit.
void encodeQuery(
  Writer writer,
  String collection,
  QueryBuilder<Object?>? query,
) {
  final filter = query?._filter;
  final sort = query?._sort ?? const [];
  final offset = query?._offset ?? 0;
  final limit = query?._limit;
  final entries =
      1 +
      (filter == null ? 0 : 1) +
      (sort.isEmpty ? 0 : 1) +
      (offset > 0 ? 1 : 0) +
      (limit == null ? 0 : 1);

  writer
    ..varint(entries)
    ..varint(1)
    ..byte(Tag.string)
    ..string(collection);

  if (filter != null) {
    writer.varint(2);
    _writeExpression(writer, filter, 1);
  }

  if (sort.isNotEmpty) {
    writer
      ..varint(3)
      ..byte(Tag.list)
      ..varint(sort.length);

    for (final (path, descending) in sort) {
      final mark = writer.open();

      writer
        ..varint(2)
        ..varint(1);
      _writePath(writer, path);
      writer
        ..varint(2)
        ..byte(descending ? Tag.trueValue : Tag.falseValue);
      writer.close(mark);
    }
  }

  if (offset > 0) {
    writer
      ..varint(4)
      ..byte(Tag.int64)
      ..int64(offset);
  }

  if (limit != null) {
    writer
      ..varint(5)
      ..byte(Tag.int64)
      ..int64(limit);
  }
}

/// Writes the values of a query's parameters as the engine reads them: a
/// record whose field 0 is how many there are and field `n + 1` the value of
/// parameter `n`, a null one left out.
void encodeParameters(Writer writer, List<Object?> parameters) {
  var present = 0;

  for (final parameter in parameters) {
    if (parameter != null) {
      present++;
    }
  }

  writer
    ..varint(present + 1)
    ..varint(0)
    ..byte(Tag.int64)
    ..int64(parameters.length);

  for (final (index, parameter) in parameters.indexed) {
    if (parameter != null) {
      writer.varint(index + 1);
      writeQueryValue(writer, parameter);
    }
  }
}
