'use strict';

/**
 * The byte formats this package exchanges with the engine, written and read
 * in JavaScript: varints, records, the IR of a query and the stored schema,
 * as `design/objects.md` in the repository specifies them.
 *
 * Objects cross the language boundary as records, many in one buffer, so a
 * batch of objects costs one call into the engine rather than one per field.
 * The engine checks every record it is given against the schema, so what is
 * written here only has to pick the right tag for each value. A record read
 * from the engine comes from the file and is treated as untrusted: anything
 * that does not fit throws an error whose `code` is `CORRUPTED`.
 */

const FALSE = 0x02;
const TRUE = 0x03;
const INT = 0x04;
const FLOAT = 0x05;
const STRING = 0x06;
const BYTES = 0x07;
const LIST = 0x08;
const OBJECT = 0x09;
const LINK = 0x0a;

/** How deeply a record read from the engine may nest, as the engine allows. */
const MAX_DEPTH = 64;

/** The largest magnitude up to which doubling a number stays exact. */
const EXACT_DOUBLE = 2 ** 52;

const BIG_SAFE = BigInt(Number.MAX_SAFE_INTEGER);
const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;
const U64_MAX = 2n ** 64n - 1n;

/** The kind codes of the stored schema's types. */
const KIND_CODES = {
  bool: 1,
  int: 2,
  float: 3,
  string: 4,
  bytes: 5,
  link: 6,
  list: 7,
  object: 8
};
const KIND_NAMES = Object.fromEntries(
  Object.entries(KIND_CODES).map(([name, code]) => [code, name])
);

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

/** An `Error` with a `code`, as every error the package throws has. */
function codeError(code, message) {
  const error = new Error(message);

  error.code = code;

  return error;
}

function invalid(message) {
  return codeError('INVALID_ARGUMENT', message);
}

function corrupted(message) {
  return codeError('CORRUPTED', `a record read from the database is damaged: ${message}`);
}

/** How a value looks, for an error message. */
function describe(value) {
  if (value === null) {
    return 'null';
  }

  if (Array.isArray(value)) {
    return 'an array';
  }

  if (value instanceof Uint8Array) {
    return 'bytes';
  }

  return typeof value === 'object' ? 'an object' : `${typeof value} ${String(value)}`;
}

/** A growing buffer that records and IR are written into. */
class Writer {
  constructor(size = 256) {
    this.bytes = new Uint8Array(size);
    this.at = 0;
    this.floats = null;
  }

  /** A view for writing floats, made the first time one is written. */
  get view() {
    if (this.floats === null || this.floats.buffer !== this.bytes.buffer) {
      this.floats = new DataView(this.bytes.buffer);
    }

    return this.floats;
  }

  reserve(extra) {
    if (this.at + extra <= this.bytes.length) {
      return;
    }

    let size = this.bytes.length * 2;

    while (size < this.at + extra) {
      size *= 2;
    }

    const bigger = new Uint8Array(size);

    bigger.set(this.bytes.subarray(0, this.at));
    this.bytes = bigger;
  }

  /**
   * Starts an embedded object: its tag, and room for its length, which
   * `close` writes once the object has been written. An object written this
   * way needs no buffer of its own.
   */
  open() {
    this.reserve(11);
    this.bytes[this.at++] = OBJECT;

    const mark = this.at;

    this.at += 10;

    return mark;
  }

  /** Ends the object `open` started at `mark`, moving it next to its length. */
  close(mark) {
    const start = mark + 10;
    const length = this.at - start;
    let size = 1;

    for (let rest = length; rest >= 0x80; rest = Math.floor(rest / 0x80)) {
      size++;
    }

    this.bytes.copyWithin(mark + size, start, this.at);
    this.at = mark;
    this.varint(length);
    this.at += length;
  }

  byte(value) {
    this.reserve(1);
    this.bytes[this.at++] = value;
  }

  /** An unsigned LEB128 varint of a non-negative number or `bigint`. */
  varint(value) {
    this.reserve(10);

    if (typeof value === 'bigint') {
      while (value >= 0x80n) {
        this.bytes[this.at++] = Number(value & 0x7fn) | 0x80;
        value >>= 7n;
      }

      this.bytes[this.at++] = Number(value);

      return;
    }

    while (value >= 0x80) {
      this.bytes[this.at++] = (value % 0x80) | 0x80;
      value = Math.floor(value / 0x80);
    }

    this.bytes[this.at++] = value;
  }

  /** A zigzag varint of an int: a whole number or a 64-bit `bigint`. */
  int(value) {
    if (typeof value === 'number') {
      if (value >= -EXACT_DOUBLE && value <= EXACT_DOUBLE) {
        this.varint(value >= 0 ? value * 2 : -value * 2 - 1);

        return;
      }

      value = BigInt(value);
    }

    this.varint(value >= 0n ? value << 1n : (-value << 1n) - 1n);
  }

  float(value) {
    this.reserve(8);
    this.view.setFloat64(this.at, value, true);
    this.at += 8;
  }

  string(value) {
    const length = value.length;
    let ascii = true;

    for (let index = 0; index < length; index++) {
      if (value.charCodeAt(index) > 0x7f) {
        ascii = false;
        break;
      }
    }

    if (!ascii) {
      // An unpaired surrogate has no UTF-8, and `encode` would turn it into
      // U+FFFD without a word.
      if (!value.isWellFormed()) {
        throw invalid(
          `the string ${JSON.stringify(value)} holds an unpaired surrogate, which UTF-8 cannot hold`
        );
      }

      this.bytesOf(encoder.encode(value));

      return;
    }

    this.varint(length);
    this.reserve(length);

    for (let index = 0; index < length; index++) {
      this.bytes[this.at++] = value.charCodeAt(index);
    }
  }

  bytesOf(value) {
    this.varint(value.length);
    this.reserve(value.length);
    this.bytes.set(value, this.at);
    this.at += value.length;
  }

  /** What has been written, copied out. */
  finish() {
    return this.bytes.slice(0, this.at);
  }
}

/** Reads what the engine gives back, checking every length and tag. */
/** The bytes of a float being read, and the view that reads them. */
const floatBytes = new Uint8Array(8);
const floatView = new DataView(floatBytes.buffer);

class Reader {
  constructor(bytes, start = 0, end = bytes.length) {
    this.bytes = bytes;
    this.at = start;
    this.end = end;
    this.floats = null;
  }

  /** A view for reading floats, made the first time one is read. */
  get view() {
    if (this.floats === null) {
      this.floats = new DataView(this.bytes.buffer, this.bytes.byteOffset, this.bytes.byteLength);
    }

    return this.floats;
  }

  byte() {
    if (this.at >= this.end) {
      throw corrupted('it ends inside a value');
    }

    return this.bytes[this.at++];
  }

  /** A varint, as a number while it fits in one exactly, a `bigint` after. */
  varint() {
    let result = 0;
    let scale = 1;

    for (let index = 0; index < 7; index++) {
      const byte = this.byte();

      result += (byte & 0x7f) * scale;

      if (byte < 0x80) {
        return result;
      }

      scale *= 0x80;
    }

    let big = BigInt(result);
    let shift = 49n;

    for (let index = 7; index < 10; index++) {
      const byte = this.byte();

      big |= BigInt(byte & 0x7f) << shift;

      if (byte < 0x80) {
        if (big > U64_MAX) {
          throw corrupted('a number is beyond 64 bits');
        }

        return big <= BIG_SAFE ? Number(big) : big;
      }

      shift += 7n;
    }

    throw corrupted('a number does not end');
  }

  /** A varint that counts bytes or values still ahead. */
  count(each = 1) {
    const count = this.varint();

    if (typeof count !== 'number' || count * each > this.end - this.at) {
      throw corrupted('it counts more than it holds');
    }

    return count;
  }

  int() {
    const zigzag = this.varint();

    if (typeof zigzag === 'number') {
      return zigzag % 2 === 0 ? zigzag / 2 : -(zigzag + 1) / 2;
    }

    const value = (zigzag & 1n) === 0n ? zigzag >> 1n : -((zigzag + 1n) >> 1n);

    return value >= -BIG_SAFE && value <= BIG_SAFE ? Number(value) : value;
  }

  float() {
    if (this.end - this.at < 8) {
      throw corrupted('it ends inside a float');
    }

    // Copied into one shared view, rather than a view of this reader's bytes
    // made for the purpose, which cost more than reading a small record.
    for (let index = 0; index < 8; index++) {
      floatBytes[index] = this.bytes[this.at + index];
    }

    this.at += 8;

    return floatView.getFloat64(0, true);
  }

  string() {
    const length = this.count();
    const start = this.at;

    this.at += length;

    if (length <= 64) {
      let ascii = true;

      for (let index = start; index < this.at; index++) {
        if (this.bytes[index] > 0x7f) {
          ascii = false;
          break;
        }
      }

      // A `Buffer` makes a string of it in one call, at a cost that does
      // not grow with its length as building one from its codes does.
      if (ascii) {
        return Buffer.isBuffer(this.bytes)
          ? this.bytes.toString('latin1', start, this.at)
          : String.fromCharCode.apply(null, this.bytes.subarray(start, this.at));
      }
    }

    try {
      return decoder.decode(this.bytes.subarray(start, this.at));
    } catch {
      throw corrupted('it holds a string that is not UTF-8');
    }
  }

  /** Bytes, copied into a `Uint8Array` of their own. */
  bytesValue() {
    const length = this.count();
    const value = new Uint8Array(length);

    value.set(this.bytes.subarray(this.at, this.at + length));
    this.at += length;

    return value;
  }
}

/** Whether `value` is a whole number that a 64-bit int holds. */
function isInt(value) {
  return (
    (typeof value === 'number' && Number.isSafeInteger(value)) ||
    (typeof value === 'bigint' && value >= I64_MIN && value <= I64_MAX)
  );
}

/**
 * Writes `value` as a value of `kind`, a type of the schema. `where` names it
 * in an error.
 */
function writeValue(writer, kind, value, where) {
  switch (kind.type) {
    case 'bool':
      if (typeof value !== 'boolean') {
        throw invalid(`\`${where}\` holds a bool, not ${describe(value)}`);
      }

      writer.byte(value ? TRUE : FALSE);

      return;
    case 'int':
      if (!isInt(value)) {
        throw invalid(`\`${where}\` holds an int, not ${describe(value)}`);
      }

      // Beyond 2^53, only a field declared with `t.bigint()` reads the value
      // back, so only such a field takes it.
      if (
        !kind.big &&
        !kind.anyInt &&
        typeof value === 'bigint' &&
        (value > BIG_SAFE || value < -BIG_SAFE)
      ) {
        throw invalid(
          `\`${where}\` is an int read as a number, which does not hold ${value} exactly; declare it with \`t.bigint()\``
        );
      }

      writer.byte(INT);
      writer.int(value);

      return;
    case 'float':
      if (typeof value !== 'number') {
        throw invalid(`\`${where}\` holds a float, not ${describe(value)}`);
      }

      writer.byte(FLOAT);
      writer.float(value);

      return;
    case 'string':
      if (typeof value !== 'string') {
        throw invalid(`\`${where}\` holds a string, not ${describe(value)}`);
      }

      writer.byte(STRING);
      writer.string(value);

      return;
    case 'bytes':
      if (!(value instanceof Uint8Array)) {
        throw invalid(`\`${where}\` holds bytes, not ${describe(value)}`);
      }

      writer.byte(BYTES);
      writer.bytesOf(value);

      return;
    case 'link':
      writer.byte(LINK);
      writeValue(writer, kind.target.key.kind, value, where);

      return;
    case 'list':
      if (!Array.isArray(value)) {
        throw invalid(`\`${where}\` holds a list, not ${describe(value)}`);
      }

      writer.byte(LIST);
      writer.varint(value.length);

      for (const element of value) {
        if (element === null || element === undefined) {
          throw invalid(`\`${where}\` is a list, and a list holds no nulls`);
        }

        writeValue(writer, kind.element, element, where);
      }

      return;
    case 'object': {
      if (typeof value !== 'object' || value === null || Array.isArray(value)) {
        throw invalid(`\`${where}\` holds an object, not ${describe(value)}`);
      }

      const mark = writer.open();

      writeFields(writer, kind.fields, value, where);
      writer.close(mark);

      return;
    }
    default:
      throw invalid(`\`${where}\` has a type this package does not know`);
  }
}

/**
 * Writes the fields of `object` that `fields` names and that are not null,
 * by id in ascending order: a record.
 */
function writeFields(writer, fields, object, where) {
  // A property the schema does not have is refused, as the engine refuses
  // it, rather than dropped: it is a typo, or a name a migration changed.
  for (const name of Object.keys(object)) {
    if (!fields.names.has(name)) {
      throw invalid(`\`${where ? `${where}.${name}` : name}\` is not a field`);
    }
  }

  let present = 0;

  for (const field of fields.list) {
    const value = own(object, field.name);

    if (value !== undefined && value !== null) {
      present++;
    }
  }

  writer.varint(present);

  for (const field of fields.list) {
    const value = own(object, field.name);

    if (value !== undefined && value !== null) {
      writer.varint(field.id);
      writeValue(writer, field.kind, value, where ? `${where}.${field.name}` : field.name);
    }
  }
}

/** Property `name` of `object` itself, never one it inherits such as `constructor`. */
function own(object, name) {
  return Object.hasOwn(object, name) ? object[name] : undefined;
}

/**
 * Writes the records of `objects` into one buffer, each after its length,
 * for a batch write.
 */
function encodeRecords(collection, objects) {
  const writer = new Writer(64 * objects.length + 64);

  for (const object of objects) {
    if (typeof object !== 'object' || object === null || Array.isArray(object)) {
      throw invalid(`an object of \`${collection.name}\` is ${describe(object)}`);
    }

    // The length goes into five bytes kept for it, a varint longer than it
    // has to be, which the engine reads all the same.
    writer.reserve(5);

    const slot = writer.at;

    writer.at += 5;
    writeFields(writer, collection.fields, object, '');

    let length = writer.at - slot - 5;

    for (let index = 0; index < 4; index++) {
      writer.bytes[slot + index] = (length % 0x80) | 0x80;
      length = Math.floor(length / 0x80);
    }

    writer.bytes[slot + 4] = length;
  }

  return writer.finish();
}

/** Skips a value of any type, as a record read with an older schema holds. */
function skipValue(reader, depth) {
  if (depth >= MAX_DEPTH) {
    throw corrupted('it nests too deeply');
  }

  const tag = reader.byte();

  switch (tag) {
    case FALSE:
    case TRUE:
      return;
    case INT:
      reader.varint();

      return;
    case FLOAT:
      reader.float();

      return;
    case STRING:
    case BYTES:
    case OBJECT: {
      // Read first: `reader.at += reader.count()` would add the length to
      // where the reader was before the length itself.
      const length = reader.count();

      reader.at += length;

      return;
    }
    case LIST: {
      const count = reader.count();

      for (let index = 0; index < count; index++) {
        skipValue(reader, depth + 1);
      }

      return;
    }
    case LINK:
      skipValue(reader, depth + 1);

      return;
    default:
      throw corrupted(`it holds an unknown tag ${tag}`);
  }
}

/** Reads a value of `kind`. */
function readValue(reader, kind, depth) {
  if (depth >= MAX_DEPTH) {
    throw corrupted('it nests too deeply');
  }

  const tag = reader.byte();

  switch (kind.type) {
    case 'bool':
      if (tag === FALSE || tag === TRUE) {
        return tag === TRUE;
      }

      break;
    case 'int':
      if (tag === INT) {
        const value = reader.int();

        if (kind.big) {
          return BigInt(value);
        }

        if (typeof value === 'bigint' && !kind.anyInt) {
          throw invalid(
            `an int field holds ${value}, beyond what a number holds exactly; declare it with \`t.bigint()\``
          );
        }

        return value;
      }

      break;
    case 'float':
      if (tag === FLOAT) {
        return reader.float();
      }

      break;
    case 'string':
      if (tag === STRING) {
        return reader.string();
      }

      break;
    case 'bytes':
      if (tag === BYTES) {
        return reader.bytesValue();
      }

      break;
    case 'link':
      if (tag === LINK) {
        // A link holds a key, which may be a `bigint`.
        const key = kind.target.key.kind;

        return readValue(reader, key.type === 'int' ? ANY_INT : key, depth + 1);
      }

      break;
    case 'list':
      if (tag === LIST) {
        const count = reader.count();
        const values = new Array(count);

        for (let index = 0; index < count; index++) {
          values[index] = readValue(reader, kind.element, depth + 1);
        }

        return values;
      }

      break;
    case 'object':
      if (tag === OBJECT) {
        const length = reader.count();
        const inner = new Reader(reader.bytes, reader.at, reader.at + length);
        const value = readFields(inner, kind.fields, false, depth + 1);

        if (inner.at !== inner.end) {
          throw corrupted('an embedded object has bytes after its last field');
        }

        reader.at += length;

        return value;
      }

      break;
    default:
      break;
  }

  throw corrupted(`a field of type ${kind.type} holds the tag ${tag}`);
}

/** An int read as a number, or a `bigint` beyond 2^53, as keys are. */
const ANY_INT = Object.freeze({ type: 'int', anyInt: true });

/** A default as a field reads it, a fresh copy of a list or bytes. */
function defaultOf(field) {
  const value = field.default;

  if (Array.isArray(value)) {
    return value.slice();
  }

  return value instanceof Uint8Array ? value.slice() : value;
}

/**
 * Reads a record with the fields of `fields`: an object holding every field,
 * a field the record lacks holding its default or null. With `lenient`, a
 * required field may be missing too, as in an object a migration has
 * rewritten; otherwise that is damage.
 */
function readFields(reader, fields, lenient, depth) {
  const count = reader.count(2);
  const values = new Array(fields.list.length);
  let last = -1;

  for (let index = 0; index < count; index++) {
    const id = reader.varint();

    if (typeof id !== 'number' || id <= last) {
      throw corrupted('its field ids are out of order');
    }

    last = id;

    const position = fields.positions.get(id);

    if (position === undefined) {
      skipValue(reader, depth);
    } else {
      values[position] = readValue(reader, fields.list[position].kind, depth);
    }
  }

  const object = {};

  for (let index = 0; index < fields.list.length; index++) {
    const field = fields.list[index];
    let value = values[index];

    if (value === undefined) {
      if (field.default !== undefined) {
        value = defaultOf(field);
      } else if (field.optional || lenient) {
        value = null;
      } else {
        throw corrupted(`it lacks the required field \`${field.name}\``);
      }
    }

    if (fields.hasProto && field.name === '__proto__') {
      // Assigning it would set the object's prototype instead.
      Object.defineProperty(object, field.name, {
        value,
        writable: true,
        enumerable: true,
        configurable: true
      });
    } else {
      object[field.name] = value;
    }
  }

  return object;
}

/** The object whose record is `bytes`. */
function decodeRecord(collection, bytes, lenient = false) {
  const reader = new Reader(bytes);
  const object = readFields(reader, collection.fields, lenient, 0);

  if (reader.at !== reader.end) {
    throw corrupted('it has bytes after its last field');
  }

  return object;
}

/** The objects of records one after another, each after its length. */
function decodeRecords(collection, bytes) {
  const reader = new Reader(bytes);
  // One reader for every record, moved from each to the next.
  const record = new Reader(bytes, 0, 0);
  const objects = [];

  while (reader.at < reader.end) {
    const length = reader.count();

    record.at = reader.at;
    record.end = reader.at + length;

    const object = readFields(record, collection.fields, false, 0);

    if (record.at !== record.end) {
      throw corrupted('it has bytes after its last field');
    }

    objects.push(object);
    reader.at += length;
  }

  return objects;
}

/**
 * A value of any type, tagged with it, for records whose schema is known
 * only once they are read: the stored schema itself.
 */
function readAny(reader, depth) {
  if (depth >= MAX_DEPTH) {
    throw corrupted('it nests too deeply');
  }

  const tag = reader.byte();

  switch (tag) {
    case FALSE:
    case TRUE:
      return { tag: 'bool', value: tag === TRUE };
    case INT:
      return { tag: 'int', value: reader.int() };
    case FLOAT:
      return { tag: 'float', value: reader.float() };
    case STRING:
      return { tag: 'string', value: reader.string() };
    case BYTES:
      return { tag: 'bytes', value: reader.bytesValue() };
    case LIST: {
      const count = reader.count();
      const values = [];

      for (let index = 0; index < count; index++) {
        values.push(readAny(reader, depth + 1));
      }

      return { tag: 'list', value: values };
    }
    case OBJECT: {
      const length = reader.count();
      const inner = new Reader(reader.bytes, reader.at, reader.at + length);
      const value = readAnyFields(inner, depth + 1);

      reader.at += length;

      return { tag: 'object', value };
    }
    case LINK:
      return { tag: 'link', value: readAny(reader, depth + 1) };
    default:
      throw corrupted(`it holds an unknown tag ${tag}`);
  }
}

function readAnyFields(reader, depth) {
  const count = reader.count(2);
  const fields = new Map();

  for (let index = 0; index < count; index++) {
    fields.set(reader.varint(), readAny(reader, depth));
  }

  if (reader.at !== reader.end) {
    throw corrupted('it has bytes after its last field');
  }

  return fields;
}

/** A tagged value as the JavaScript value it holds. */
function untag(tagged) {
  switch (tagged.tag) {
    case 'list':
      return tagged.value.map(untag);
    case 'link':
      return untag(tagged.value);
    default:
      return tagged.value;
  }
}

/** Field `id` of a record read with `readAny`, which has to be there. */
function field(record, id, tag) {
  const value = record.get(id);

  if (value === undefined || value.tag !== tag) {
    throw corrupted(`the schema lacks a ${tag} in field ${id}`);
  }

  return value.value;
}

/** The fields of a collection or an embedded object, ready to read and write. */
function fieldsOf(list) {
  list.sort((a, b) => a.id - b.id);

  return {
    list,
    positions: new Map(list.map((field, index) => [field.id, index])),
    names: new Set(list.map((field) => field.name)),
    hasProto: list.some((field) => field.name === '__proto__')
  };
}

/**
 * The collections of a stored schema's record: for each, its fields with
 * their ids and types, its key, and its indexes, with links resolved to the
 * collections they name.
 */
function decodeSchema(bytes) {
  try {
    return decodeSchemaFields(bytes);
  } catch (error) {
    if (error.code !== undefined) {
      throw error;
    }

    throw corrupted(`the stored schema does not decode: ${error.message}`);
  }
}

function decodeSchemaFields(bytes) {
  const record = readAnyFields(new Reader(bytes), 0);
  const byId = new Map();
  const links = [];

  const kindOf = (raw) => {
    const type = KIND_NAMES[field(raw, 1, 'int')];

    switch (type) {
      case 'link': {
        const kind = { type, target: null };

        links.push([kind, field(raw, 2, 'int')]);

        return kind;
      }
      case 'list':
        return { type, element: kindOf(field(raw, 3, 'object')) };
      case 'object':
        return { type, fields: fieldsOf(field(raw, 4, 'list').map(fieldOf)) };
      case undefined:
        throw corrupted('the schema names an unknown type');
      default:
        return { type };
    }
  };
  const fieldOf = (raw) => {
    const fields = raw.value;
    const defaultValue = fields.get(5);

    return {
      id: field(fields, 1, 'int'),
      name: field(fields, 2, 'string'),
      kind: kindOf(field(fields, 3, 'object')),
      optional: field(fields, 4, 'bool'),
      default: defaultValue === undefined ? undefined : untag(defaultValue)
    };
  };
  const collections = new Map();

  for (const raw of field(record, 3, 'list')) {
    const fields = raw.value;
    const list = field(fields, 3, 'list').map(fieldOf);
    const key = field(fields, 5, 'int');
    const collection = {
      id: field(fields, 1, 'int'),
      name: field(fields, 2, 'string'),
      fields: fieldsOf(list),
      key: list.find((each) => each.id === key),
      auto: field(fields, 6, 'bool')
    };

    if (collection.key === undefined) {
      throw corrupted(`\`${collection.name}\` has no key field`);
    }

    byId.set(collection.id, collection);
    collections.set(collection.name, collection);
  }

  for (const [kind, id] of links) {
    kind.target = byId.get(id);

    if (kind.target === undefined) {
      throw corrupted('the schema links to a collection it lacks');
    }
  }

  return { version: field(record, 2, 'int'), collections };
}

/**
 * The record of a schema declared with this package, encoded as the file
 * stores a schema, with ids given in the order of declaration. The engine
 * reads it with `Schema::decode` and gives the file's ids its own way.
 */
function encodeSchema(declared) {
  const names = Object.keys(declared.collections);
  const ids = new Map(names.map((name, index) => [name, index + 1]));
  let nextIndex = 1;
  const writer = new Writer(1024);

  const writeType = (w, spec, where) => {
    const entries = [[1, 'int', KIND_CODES[spec.type]]];

    if (spec.type === 'link') {
      const target = ids.get(spec.target);

      if (target === undefined) {
        throw invalid(`\`${where}\` links to \`${spec.target}\`, which is not a collection`);
      }

      entries.push([2, 'int', target]);
    } else if (spec.type === 'list') {
      if (spec.element.optional || spec.element.default !== undefined) {
        throw invalid(
          `\`${where}\` is a list, and a list's elements are neither optional nor defaulted`
        );
      }

      entries.push([3, 'type', spec.element]);
    } else if (spec.type === 'object') {
      entries.push([4, 'fields', spec.fields], [5, 'int', Object.keys(spec.fields).length + 1]);
    }

    writeEntries(w, entries, where);
  };
  const writeFieldList = (w, fields, where, embedded) => {
    w.byte(LIST);
    w.varint(fields.length);

    for (const [id, name, spec] of fields) {
      if (embedded && (spec.index || spec.unique || spec.primaryKey)) {
        throw invalid(
          `\`${where}.${name}\` is inside an embedded object, where no field is a key or indexed`
        );
      }

      const entries = [
        [1, 'int', id],
        [2, 'string', name],
        [3, 'type', spec],
        [4, 'bool', spec.optional === true]
      ];

      if (spec.default !== undefined) {
        entries.push([5, 'default', spec]);
      }

      w.byte(OBJECT);

      const inner = new Writer(128);

      writeEntries(inner, entries, `${where}.${name}`);
      w.bytesOf(inner.bytes.subarray(0, inner.at));
    }
  };
  const writeEntries = (w, entries, where) => {
    w.varint(entries.length);

    for (const [id, what, value] of entries) {
      w.varint(id);

      switch (what) {
        case 'int':
          w.byte(INT);
          w.int(value);
          break;
        case 'bool':
          w.byte(value ? TRUE : FALSE);
          break;
        case 'string':
          w.byte(STRING);
          w.string(value);
          break;
        case 'type': {
          const inner = new Writer(64);

          writeType(inner, value, where);
          w.byte(OBJECT);
          w.bytesOf(inner.bytes.subarray(0, inner.at));
          break;
        }
        case 'fields':
          writeFieldList(
            w,
            Object.entries(value).map(([name, type], index) => [index + 1, name, type.spec]),
            where,
            true
          );
          break;
        case 'default':
          writeValue(w, kindOfSpec(value, ids), value.default, where);
          break;
        case 'raw':
          value(w);
          break;
        default:
          throw invalid(`the schema has an entry of an unknown kind ${what}`);
      }
    }
  };

  const collections = names.map((name) => {
    const declaredFields = Object.entries(declared.collections[name].fields);
    const keys = declaredFields.filter(([, type]) => type.spec.primaryKey);

    if (keys.length > 1) {
      throw invalid(`\`${name}\` has more than one primary key`);
    }

    const auto = keys.length === 0;
    const offset = auto ? 2 : 1;
    const fields = declaredFields.map(([fieldName, type], index) => [
      index + offset,
      fieldName,
      type.spec
    ]);

    if (auto) {
      fields.unshift([1, 'id', { type: 'int', optional: false }]);
    }

    const key = auto ? 1 : fields.find(([, fieldName]) => fieldName === keys[0][0])[0];
    const indexes = fields
      .filter(([, , spec]) => spec.index || spec.unique)
      .map(([id, , spec]) => [nextIndex++, id, spec.unique === true]);

    return { id: ids.get(name), name, fields, key, auto, indexes };
  });

  writeEntries(
    writer,
    [
      [1, 'int', 1],
      [2, 'int', declared.version],
      [
        3,
        'raw',
        (w) => {
          w.byte(LIST);
          w.varint(collections.length);

          for (const collection of collections) {
            const inner = new Writer(256);

            writeEntries(
              inner,
              [
                [1, 'int', collection.id],
                [2, 'string', collection.name],
                [3, 'raw', (fw) => writeFieldList(fw, collection.fields, collection.name, false)],
                [4, 'int', collection.fields.length + 1],
                [5, 'int', collection.key],
                [6, 'bool', collection.auto],
                [
                  7,
                  'raw',
                  (iw) => {
                    iw.byte(LIST);
                    iw.varint(collection.indexes.length);

                    for (const [id, fieldId, unique] of collection.indexes) {
                      const index = new Writer(16);

                      writeEntries(
                        index,
                        [
                          [1, 'int', id],
                          [2, 'int', fieldId],
                          [3, 'bool', unique]
                        ],
                        collection.name
                      );
                      iw.byte(OBJECT);
                      iw.bytesOf(index.bytes.subarray(0, index.at));
                    }
                  }
                ]
              ],
              collection.name
            );
            w.byte(OBJECT);
            w.bytesOf(inner.bytes.subarray(0, inner.at));
          }
        }
      ],
      [4, 'int', collections.length + 1],
      [5, 'int', nextIndex]
    ],
    ''
  );

  return writer.finish();
}

/** The kind of a declared type, for writing its default. */
function kindOfSpec(spec, ids) {
  switch (spec.type) {
    case 'list':
      return { type: 'list', element: kindOfSpec(spec.element, ids) };
    case 'link':
      throw invalid('a link has no default: it would name an object');
    case 'object':
      throw invalid('an embedded object has no default: its fields have their own');
    default:
      return { type: spec.type };
  }
}

/**
 * A parameter of a prepared query, which each run gives a value: `param(0)`
 * is the first. The IR holds one as an object whose field 1 is its number.
 */
class Param {
  constructor(index) {
    this.index = index;
    Object.freeze(this);
  }
}

/** Writes a value a query compares with, by its JavaScript type, or a parameter. */
function writeQueryValue(writer, value) {
  if (value instanceof Param) {
    const mark = writer.open();

    writer.varint(1);
    writer.varint(1);
    writer.byte(INT);
    writer.int(value.index);
    writer.close(mark);
  } else if (typeof value === 'boolean') {
    writer.byte(value ? TRUE : FALSE);
  } else if (typeof value === 'number') {
    if (Number.isSafeInteger(value)) {
      writer.byte(INT);
      writer.int(value);
    } else {
      writer.byte(FLOAT);
      writer.float(value);
    }
  } else if (typeof value === 'bigint') {
    if (!isInt(value)) {
      throw codeError('INVALID_QUERY', `${value} is beyond a 64-bit int`);
    }

    writer.byte(INT);
    writer.int(value);
  } else if (typeof value === 'string') {
    writer.byte(STRING);
    writer.string(value);
  } else if (value instanceof Uint8Array) {
    writer.byte(BYTES);
    writer.bytesOf(value);
  } else {
    throw codeError('INVALID_QUERY', `a query compares with single values, not ${describe(value)}`);
  }
}

const AND = 1;
const OR = 2;
const NOT = 3;

function writePath(writer, path) {
  writer.byte(LIST);
  writer.varint(path.length);

  for (const name of path) {
    writer.byte(STRING);
    writer.string(name);
  }
}

/** How deeply a filter may nest, as the engine holds every query to. */
const MAX_FILTER_DEPTH = 24;

function writeExpression(writer, node, depth = 1) {
  if (depth > MAX_FILTER_DEPTH) {
    throw codeError('INVALID_QUERY', `the filter nests more than ${MAX_FILTER_DEPTH} levels deep`);
  }

  const inner = writer;
  const mark = writer.open();

  switch (node.kind) {
    case 'and':
    case 'or':
    case 'not': {
      const terms = node.kind === 'not' ? [node.term] : node.terms;

      inner.varint(terms.length === 0 ? 1 : 2);
      inner.varint(1);
      inner.byte(INT);
      inner.int(node.kind === 'and' ? AND : node.kind === 'or' ? OR : NOT);

      if (terms.length > 0) {
        inner.varint(4);
        inner.byte(LIST);
        inner.varint(terms.length);

        for (const term of terms) {
          writeExpression(inner, term, depth + 1);
        }
      }

      break;
    }
    case 'test':
      inner.varint(node.values.length === 0 ? 2 : 3);
      inner.varint(1);
      inner.byte(INT);
      inner.int(node.op);
      inner.varint(2);
      writePath(inner, node.path);

      if (node.values.length > 0) {
        inner.varint(3);
        inner.byte(LIST);
        inner.varint(node.values.length);

        for (const value of node.values) {
          writeQueryValue(inner, value);
        }
      }

      break;
    default:
      throw codeError('INVALID_QUERY', 'a filter holds something that is not a condition');
  }

  writer.close(mark);
}

/** The buffer parameters are encoded in, reused as `queryWriter` is. */
const parameterWriter = new Writer(64);

/**
 * The values of a query's parameters as the engine reads them
 * (`Query::bind_encoded`): a record whose field 0 is how many there are and
 * field `n + 1` the value of parameter `n`, a null one left out, since a
 * record holds no null. The buffer is lent: the next parameters are encoded
 * in it, so the caller hands it to the engine at once.
 */
function encodeParameters(parameters) {
  const writer = parameterWriter;
  let present = 0;

  for (const parameter of parameters) {
    if (parameter !== null && parameter !== undefined) {
      present++;
    }
  }

  writer.at = 0;
  writer.varint(present + 1);
  writer.varint(0);
  writer.byte(INT);
  writer.int(parameters.length);

  for (let n = 0; n < parameters.length; n++) {
    const parameter = parameters[n];

    if (parameter !== null && parameter !== undefined) {
      writer.varint(n + 1);
      writeQueryValue(writer, parameter);
    }
  }

  return Buffer.from(writer.bytes.buffer, writer.bytes.byteOffset, writer.at);
}

/** The buffer queries are encoded in, reused: a query is copied out of it. */
const queryWriter = new Writer(256);

/**
 * The IR of a query on `collection`: its filter, sort, offset and limit, and
 * whether it counts rather than returns the objects. With `lend`, the IR is
 * a view of the buffer the next query is encoded in, for a caller that
 * hands it to the engine at once; otherwise it is a copy.
 */
function encodeQuery(collection, query, count, lend = false) {
  const writer = queryWriter;

  writer.at = 0;
  const entries = [1];

  if (query.filter !== null) {
    entries.push(2);
  }

  if (query.sort.length > 0) {
    entries.push(3);
  }

  if (query.offset > 0) {
    entries.push(4);
  }

  if (query.limit !== null) {
    entries.push(5);
  }

  if (count) {
    entries.push(6);
  }

  writer.varint(entries.length);

  for (const id of entries) {
    writer.varint(id);

    switch (id) {
      case 1:
        writer.byte(STRING);
        writer.string(collection);
        break;
      case 2:
        writeExpression(writer, query.filter);
        break;
      case 3:
        writer.byte(LIST);
        writer.varint(query.sort.length);

        for (const [path, descending] of query.sort) {
          const mark = writer.open();

          writer.varint(2);
          writer.varint(1);
          writePath(writer, path);
          writer.varint(2);
          writer.byte(descending ? TRUE : FALSE);
          writer.close(mark);
        }

        break;
      case 4:
        writer.byte(INT);
        writer.int(query.offset);
        break;
      case 5:
        writer.byte(INT);
        writer.int(query.limit);
        break;
      default:
        writer.byte(TRUE);
        break;
    }
  }

  if (lend) {
    return Buffer.from(writer.bytes.buffer, writer.bytes.byteOffset, writer.at);
  }

  return Buffer.from(writer.bytes.subarray(0, writer.at));
}

module.exports = {
  Reader,
  Param,
  codeError,
  invalid,
  encodeRecords,
  decodeRecord,
  decodeRecords,
  decodeSchema,
  encodeSchema,
  encodeQuery,
  encodeParameters
};
