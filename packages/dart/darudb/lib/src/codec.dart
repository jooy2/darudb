/// The record format of `design/objects.md`, "Records", written and read in
/// Dart: varints, tags and the values they mark. Objects, queries and the
/// schema cross into the engine as records, so a batch of objects costs one
/// call and one buffer rather than a call per field.
///
/// A record read from the engine comes from the file and is untrusted:
/// anything that does not fit throws a [DaruException] whose code is
/// `CORRUPTED`.
library;

import 'dart:convert';
import 'dart:typed_data';

import 'errors.dart';

/// The tags a record's values carry.
abstract final class Tag {
  /// Null, in the changes of an update alone: the field becomes null.
  static const int nil = 0x01;
  static const int falseValue = 0x02;
  static const int trueValue = 0x03;
  static const int int64 = 0x04;
  static const int float = 0x05;
  static const int string = 0x06;
  static const int bytes = 0x07;
  static const int list = 0x08;
  static const int object = 0x09;
  static const int link = 0x0A;
}

/// How deeply a record read from the engine may nest, as the engine allows.
const int maxDepth = 64;

DaruException corrupted(String message) => DaruException('CORRUPTED', message);

DaruException invalidArgument(String message) =>
    DaruException('INVALID_ARGUMENT', message);

/// A growing buffer that a record is written into.
final class Writer {
  Writer([int size = 256]) : bytes = Uint8List(size);

  Uint8List bytes;
  int at = 0;
  ByteData? _floats;

  /// What has been written, as a view of the buffer.
  Uint8List get written => Uint8List.sublistView(bytes, 0, at);

  void reset() {
    at = 0;
  }

  void reserve(int extra) {
    if (at + extra <= bytes.length) {
      return;
    }

    var size = bytes.length * 2;

    while (size < at + extra) {
      size *= 2;
    }

    bytes = Uint8List(size)..setRange(0, at, bytes);
    _floats = null;
  }

  void byte(int value) {
    reserve(1);
    bytes[at++] = value;
  }

  /// An unsigned LEB128 varint of a non-negative int below 2^63.
  void varint(int value) {
    reserve(10);

    while (value >= 0x80 || value < 0) {
      bytes[at++] = (value & 0x7F) | 0x80;
      value = value >>> 7;
    }

    bytes[at++] = value;
  }

  /// A zigzag varint of a 64-bit int.
  void int64(int value) {
    varint((value << 1) ^ (value >> 63));
  }

  void float(double value) {
    reserve(8);
    (_floats ??= ByteData.sublistView(
      bytes,
    )).setFloat64(at, value, Endian.little);
    at += 8;
  }

  /// A string's length and UTF-8. An ASCII string, as nearly every one is,
  /// is copied as it is checked.
  void string(String value) {
    final length = value.length;
    final start = at;

    reserve(10 + length);
    varint(length);

    final buffer = bytes;
    var position = at;

    for (var index = 0; index < length; index++) {
      final unit = value.codeUnitAt(index);

      if (unit > 0x7F) {
        at = start;
        bytesOf(_utf8(value));

        return;
      }

      buffer[position++] = unit;
    }

    at = position;
  }

  static Uint8List _utf8(String value) {
    // An unpaired surrogate has no UTF-8, and the encoder would turn it
    // into U+FFFD without a word.
    for (var index = 0; index < value.length; index++) {
      final unit = value.codeUnitAt(index);

      if (unit >= 0xD800 && unit <= 0xDFFF) {
        final high = unit <= 0xDBFF;
        final next = index + 1 < value.length ? value.codeUnitAt(index + 1) : 0;

        if (!high || next < 0xDC00 || next > 0xDFFF) {
          throw invalidArgument(
            'the string ${jsonEncode(value)} holds an unpaired surrogate, '
            'which UTF-8 cannot hold',
          );
        }

        index++;
      }
    }

    return utf8.encode(value);
  }

  void bytesOf(List<int> value) {
    varint(value.length);
    reserve(value.length);
    bytes.setRange(at, at + value.length, value);
    at += value.length;
  }

  /// Starts an embedded object: its tag, and room for its length, which
  /// [close] writes once the object has been written.
  int open() {
    byte(Tag.object);

    return openLength();
  }

  /// Leaves room for the length of what comes next, which [close] writes
  /// once it has been written, as a batch of records holds each record.
  int openLength() {
    reserve(10);

    final mark = at;

    at += 10;

    return mark;
  }

  /// Ends what [open] or [openLength] started at [mark], moving it next to
  /// its length.
  void close(int mark) {
    final start = mark + 10;
    final length = at - start;
    var size = 1;

    for (var rest = length; rest >= 0x80; rest >>>= 7) {
      size++;
    }

    bytes.setRange(mark + size, mark + size + length, bytes, start);
    at = mark;
    varint(length);
    at += length;
  }

  /// Leaves room for a count that is known once what it counts is written:
  /// the most it can be. Returns where it starts.
  int reserveCount(int most) {
    final start = at;

    varint(most);

    return start;
  }

  /// Writes [count] in the room [reserveCount] left at [start] for at most
  /// [most], taking back what a shorter count does not need.
  void setCount(int start, int most, int count) {
    if (count == most) {
      return;
    }

    final reserved = _varintLength(most);
    final needed = _varintLength(count);
    final end = at;

    if (needed != reserved) {
      bytes.setRange(
        start + needed,
        end - reserved + needed,
        bytes,
        start + reserved,
      );
      at = end - reserved + needed;
    }

    final saved = at;

    at = start;
    varint(count);
    at = saved;
  }

  static int _varintLength(int value) {
    var length = 1;

    for (var rest = value; rest >= 0x80; rest >>>= 7) {
      length++;
    }

    return length;
  }
}

const _decoder = Utf8Decoder();

/// A position in a record being read.
final class Reader {
  Reader(this.bytes, [this.at = 0, int? end]) : end = end ?? bytes.length;

  final Uint8List bytes;
  int at;
  final int end;
  ByteData? _floats;

  bool get isDone => at >= end;

  int byte() {
    if (at >= end) {
      throw corrupted('a record ends early');
    }

    return bytes[at++];
  }

  int varint() {
    var value = 0;

    for (var shift = 0; shift < 64; shift += 7) {
      final byte = this.byte();

      if (shift == 63 && (byte & 0x7F) > 1) {
        throw corrupted('a record has a number too large');
      }

      value |= (byte & 0x7F) << shift;

      if (byte & 0x80 == 0) {
        return value;
      }
    }

    throw corrupted('a record has a number too long');
  }

  int int64() {
    final value = varint();

    return (value >>> 1) ^ -(value & 1);
  }

  double float() {
    if (at + 8 > end) {
      throw corrupted('a record ends early');
    }

    final value = (_floats ??= ByteData.sublistView(
      bytes,
    )).getFloat64(at, Endian.little);

    at += 8;

    return value;
  }

  /// A length and the bytes it counts, as a view of the record.
  Uint8List counted() {
    final length = varint();

    if (length < 0 || length > end - at) {
      throw corrupted('a record ends early');
    }

    final view = Uint8List.sublistView(bytes, at, at + length);

    at += length;

    return view;
  }

  /// A string's length and UTF-8.
  String string() {
    final length = varint();

    if (length < 0 || length > end - at) {
      throw corrupted('a record ends early');
    }

    final start = at;

    at += length;

    for (var index = start; index < at; index++) {
      if (bytes[index] > 0x7F) {
        try {
          return _decoder.convert(bytes, start, at);
        } on FormatException {
          throw corrupted('a record holds a string that is not UTF-8');
        }
      }
    }

    return String.fromCharCodes(bytes, start, at);
  }

  /// A count of things each at least [each] bytes long, which a damaged
  /// record cannot make larger than what is left of it.
  int count([int each = 1]) {
    final count = varint();

    if (count < 0 || count * each > end - at) {
      throw corrupted('a record counts more than it holds');
    }

    return count;
  }

  /// Steps over a value, tag first, without reading it into one.
  void skip([int depth = 0]) {
    if (depth >= maxDepth) {
      throw corrupted('a record nests too deeply');
    }

    switch (byte()) {
      case Tag.falseValue || Tag.trueValue:
        break;
      case Tag.int64:
        varint();
      case Tag.float:
        if (at + 8 > end) {
          throw corrupted('a record ends early');
        }

        at += 8;
      case Tag.string || Tag.bytes || Tag.object:
        counted();
      case Tag.list:
        for (var left = count(); left > 0; left--) {
          skip(depth + 1);
        }
      case Tag.link:
        skip(depth + 1);
      default:
        throw corrupted('a record has an unknown tag');
    }
  }

  /// A value of any type, tag first, as the plain Dart value it is: `bool`,
  /// `int`, `double`, `String`, `Uint8List`, a `List`, a `Map` of an
  /// embedded record's fields by id, or a [RawLink].
  Object? any([int depth = 0]) {
    if (depth >= maxDepth) {
      throw corrupted('a record nests too deeply');
    }

    switch (byte()) {
      case Tag.falseValue:
        return false;
      case Tag.trueValue:
        return true;
      case Tag.int64:
        return int64();
      case Tag.float:
        return float();
      case Tag.string:
        return string();
      case Tag.bytes:
        return Uint8List.fromList(counted());
      case Tag.list:
        return [for (var left = count(); left > 0; left--) any(depth + 1)];
      case Tag.object:
        final inner = Reader(counted());
        final fields = inner.anyFields(depth + 1);

        if (!inner.isDone) {
          throw corrupted('an embedded record has bytes after its last field');
        }

        return fields;
      case Tag.link:
        return RawLink(any(depth + 1));
      default:
        throw corrupted('a record has an unknown tag');
    }
  }

  /// The fields of a record of any shape, by id.
  Map<int, Object?> anyFields([int depth = 0]) {
    final fields = <int, Object?>{};
    var last = -1;

    for (var left = count(2); left > 0; left--) {
      final id = varint();

      if (id <= last) {
        throw corrupted("a record's field ids are out of order");
      }

      last = id;
      fields[id] = any(depth);
    }

    return fields;
  }
}

/// A link as a record holds it, read without its schema: the key.
final class RawLink {
  const RawLink(this.key);

  final Object? key;
}
