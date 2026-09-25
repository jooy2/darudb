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

import type { DeclaredFields, DeclaredSchema, Spec } from './schema.js';
import type { FilterNode, QueryParts } from './query.js';

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
// `Object.entries` types the names as strings and `fromEntries` the keys;
// these are the names of `KIND_CODES` by their codes.
const KIND_NAMES = Object.fromEntries(
  Object.entries(KIND_CODES).map(([name, code]) => [code, name])
) as Partial<Record<number, Kind['type']>>;

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

/** An `Error` with a `code`, as every error the package throws has. */
export interface CodeError extends Error {
  code: string;
}

/** The kind of a field as the stored schema gives it, and how the field reads it. */
export type Kind =
  | { type: 'bool' }
  | IntKind
  | { type: 'float' }
  | { type: 'string' }
  | { type: 'bytes' }
  | LinkKind
  | { type: 'list'; element: Kind }
  | { type: 'object'; fields: Layout };

/**
 * An int kind. `big` marks a field declared with `t.bigint()`, which reads a
 * `bigint` always, and `anyInt` one that reads a number up to 2^53 and a
 * `bigint` beyond, as a key does.
 */
export interface IntKind {
  type: 'int';
  big?: boolean;
  anyInt?: boolean;
}

/** A link kind: the collection whose primary key it holds. */
export interface LinkKind {
  type: 'link';
  target: CollectionLayout;
}

/**
 * A field of a stored schema: its id, name and kind, whether it may be null,
 * and its default, `undefined` for none.
 */
export interface FieldLayout {
  id: number;
  name: string;
  kind: Kind;
  optional: boolean;
  default: unknown;
}

/** Makes an object of a layout from its fields' values, in the layout's order. */
export type Builder = (values: unknown[]) => Record<string, unknown>;

/**
 * The fields of a collection or an embedded object, as `fieldsOf` makes them
 * ready to read and write: in id order, with the position of each id, the
 * names, whether one is `__proto__`, and the function `builderOf` makes for
 * the layout, `undefined` until it has been made.
 */
export interface Layout {
  list: FieldLayout[];
  positions: Map<number, number>;
  names: Set<string>;
  hasProto: boolean;
  build: Builder | null | undefined;
}

/**
 * A collection of a stored schema: its id, name and fields, its key field,
 * and whether the engine numbers its keys.
 */
export interface CollectionLayout {
  id: number | bigint;
  name: string;
  fields: Layout;
  key: FieldLayout;
  auto: boolean;
}

/** A stored schema as `decodeSchema` reads it: its version and its collections by name. */
export interface SchemaLayout {
  version: number | bigint;
  collections: Map<string, CollectionLayout>;
}

/** A value read with `readAny`, tagged with its type. */
export type Tagged =
  | { tag: 'bool'; value: boolean }
  | { tag: 'int'; value: number | bigint }
  | { tag: 'float'; value: number }
  | { tag: 'string'; value: string }
  | { tag: 'bytes'; value: Uint8Array }
  | { tag: 'list'; value: Tagged[] }
  | { tag: 'object'; value: AnyRecord }
  | { tag: 'link'; value: Tagged };

/** A record read with `readAny`: its values by field id. */
export type AnyRecord = Map<number | bigint, Tagged>;

/** The value a `Tagged` of tag `T` holds. */
type TaggedValue<T extends Tagged['tag']> = Extract<Tagged, { tag: T }>['value'];

/** An `Error` with a `code`, as every error the package throws has. */
function codeError(code: string, message: string): CodeError {
  // Its code is set on the next line.
  const error = new Error(message) as CodeError;

  error.code = code;

  return error;
}

function invalid(message: string): CodeError {
  return codeError('INVALID_ARGUMENT', message);
}

function corrupted(message: string): CodeError {
  return codeError('CORRUPTED', `a record read from the database is damaged: ${message}`);
}

/** How a value looks, for an error message. */
function describe(value: unknown): string {
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
  declare bytes: Uint8Array;
  declare at: number;
  declare floats: DataView | null;

  constructor(size = 256) {
    this.bytes = new Uint8Array(size);
    this.at = 0;
    this.floats = null;
  }

  /** A view for writing floats, made the first time one is written. */
  get view(): DataView {
    if (this.floats === null || this.floats.buffer !== this.bytes.buffer) {
      this.floats = new DataView(this.bytes.buffer);
    }

    return this.floats;
  }

  reserve(extra: number): void {
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
  open(): number {
    this.reserve(11);
    this.bytes[this.at++] = OBJECT;

    const mark = this.at;

    this.at += 10;

    return mark;
  }

  /** Ends the object `open` started at `mark`, moving it next to its length. */
  close(mark: number): void {
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

  byte(value: number): void {
    this.reserve(1);
    this.bytes[this.at++] = value;
  }

  /** An unsigned LEB128 varint of a non-negative number or `bigint`. */
  varint(value: number | bigint): void {
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
  int(value: number | bigint): void {
    if (typeof value === 'number') {
      if (value >= -EXACT_DOUBLE && value <= EXACT_DOUBLE) {
        this.varint(value >= 0 ? value * 2 : -value * 2 - 1);

        return;
      }

      value = BigInt(value);
    }

    this.varint(value >= 0n ? value << 1n : (-value << 1n) - 1n);
  }

  float(value: number): void {
    this.reserve(8);
    this.view.setFloat64(this.at, value, true);
    this.at += 8;
  }

  string(value: string): void {
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

  bytesOf(value: Uint8Array): void {
    this.varint(value.length);
    this.reserve(value.length);
    this.bytes.set(value, this.at);
    this.at += value.length;
  }

  /** What has been written, copied out. */
  finish(): Uint8Array {
    return this.bytes.slice(0, this.at);
  }
}

/**
 * The longest record whose strings are cut from one text of the record
 * (`Reader.string`), which a string cut from it may keep alive: a record
 * with a large value beside its strings makes each of them on its own.
 */
const SHARED_TEXT = 512;

/** Reads what the engine gives back, checking every length and tag. */
/** The bytes of a float being read, and the view that reads them. */
const floatBytes = new Uint8Array(8);
const floatView = new DataView(floatBytes.buffer);

class Reader {
  declare bytes: Uint8Array;
  declare at: number;
  declare end: number;
  declare floats: DataView | null;
  /** Where the bytes being read begin, which `text` starts at. */
  declare start: number;
  /** The bytes from `start` to `end` as Latin-1 text, once a string needs it. */
  declare text: string | null;

  constructor(bytes: Uint8Array, start = 0, end = bytes.length) {
    this.bytes = bytes;
    this.at = start;
    this.end = end;
    this.floats = null;
    this.start = start;
    this.text = null;
  }

  /** Moves the reader to the bytes from `start` to `end`: the next record. */
  restart(start: number, end: number): void {
    this.at = start;
    this.start = start;
    this.end = end;
    this.text = null;
  }

  /** A view for reading floats, made the first time one is read. */
  get view(): DataView {
    if (this.floats === null) {
      this.floats = new DataView(this.bytes.buffer, this.bytes.byteOffset, this.bytes.byteLength);
    }

    return this.floats;
  }

  byte(): number {
    if (this.at >= this.end) {
      throw corrupted('it ends inside a value');
    }

    return this.bytes[this.at++];
  }

  /** A varint, as a number while it fits in one exactly, a `bigint` after. */
  varint(): number | bigint {
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
  count(each = 1): number {
    const count = this.varint();

    if (typeof count !== 'number' || count * each > this.end - this.at) {
      throw corrupted('it counts more than it holds');
    }

    return count;
  }

  int(): number | bigint {
    const zigzag = this.varint();

    if (typeof zigzag === 'number') {
      return zigzag % 2 === 0 ? zigzag / 2 : -(zigzag + 1) / 2;
    }

    const value = (zigzag & 1n) === 0n ? zigzag >> 1n : -((zigzag + 1n) >> 1n);

    return value >= -BIG_SAFE && value <= BIG_SAFE ? Number(value) : value;
  }

  float(): number {
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

  string(): string {
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
      if (ascii && Buffer.isBuffer(this.bytes)) {
        if (this.end - this.start > SHARED_TEXT) {
          return this.bytes.toString('latin1', start, this.at);
        }

        // The strings of a short record are cut from one text of the whole
        // record, made at its first string: the call that makes a string
        // costs several times what cutting one out of another does. A cut
        // may keep that text alive for as long as it lives itself, which
        // `SHARED_TEXT` bounds.
        if (this.text === null) {
          this.text = this.bytes.toString('latin1', this.start, this.end);
        }

        return this.text.substring(start - this.start, this.at - this.start);
      }

      if (ascii) {
        // `apply` takes any array-like, a `Uint8Array` as well as an array.
        return String.fromCharCode.apply(
          null,
          this.bytes.subarray(start, this.at) as unknown as number[]
        );
      }
    }

    try {
      return decoder.decode(this.bytes.subarray(start, this.at));
    } catch {
      throw corrupted('it holds a string that is not UTF-8');
    }
  }

  /** Bytes, copied into a `Uint8Array` of their own. */
  bytesValue(): Uint8Array {
    const length = this.count();
    const value = new Uint8Array(length);

    value.set(this.bytes.subarray(this.at, this.at + length));
    this.at += length;

    return value;
  }
}

/** Whether `value` is a whole number that a 64-bit int holds. */
function isInt(value: unknown): value is number | bigint {
  return (
    (typeof value === 'number' && Number.isSafeInteger(value)) ||
    (typeof value === 'bigint' && value >= I64_MIN && value <= I64_MAX)
  );
}

/**
 * Writes `value` as a value of `kind`, a type of the schema. `where` names it
 * in an error.
 */
function writeValue(writer: Writer, kind: Kind, value: unknown, where: string): void {
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
function writeFields(writer: Writer, fields: Layout, object: object, where: string): void {
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
function own(object: object, name: string): unknown {
  // Any object's properties can be read by name.
  return Object.hasOwn(object, name) ? (object as Record<string, unknown>)[name] : undefined;
}

/**
 * Writes the records of `objects` into one buffer, each after its length,
 * for a batch write.
 */
function encodeRecords(collection: CollectionLayout, objects: readonly unknown[]): Uint8Array {
  const writer = new Writer(64 * objects.length + 64);

  writeRecords(writer, collection, objects);

  return writer.finish();
}

/** The buffer a synchronous write's records are encoded in, reused. */
const recordWriter = new Writer(1024);

/** Whether records are being encoded in `recordWriter`. */
let recordsLent = false;

/** The most records `lendRecords` encodes in the buffer it reuses. */
const LENT_RECORDS = 16;

/** How large the reused buffer may stay after a large record grew it. */
const LENT_BYTES = 64 * 1024;

/**
 * `encodeRecords` for a synchronous write, which hands the records to the
 * engine at once: a view of a buffer kept for the purpose, which lasts until
 * the next records are lent, rather than a buffer of their own. A few
 * records, as a `put` or an `insert` writes, cost the allocation and the
 * copy of a buffer of their own several times what encoding them does. A
 * large batch gets a buffer of its own all the same, and so do the records
 * a getter writes while an object of the batch is being encoded.
 */
function lendRecords(collection: CollectionLayout, objects: readonly unknown[]): Buffer {
  if (recordsLent || objects.length > LENT_RECORDS) {
    const bytes = encodeRecords(collection, objects);

    return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.length);
  }

  recordsLent = true;

  try {
    if (recordWriter.bytes.length > LENT_BYTES) {
      recordWriter.bytes = new Uint8Array(1024);
    }

    recordWriter.at = 0;
    writeRecords(recordWriter, collection, objects);

    return Buffer.from(recordWriter.bytes.buffer, recordWriter.bytes.byteOffset, recordWriter.at);
  } finally {
    recordsLent = false;
  }
}

/** Writes the records of `objects` into `writer`, each after its length. */
function writeRecords(
  writer: Writer,
  collection: CollectionLayout,
  objects: readonly unknown[]
): void {
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
}

/** Skips a value of any type, as a record read with an older schema holds. */
function skipValue(reader: Reader, depth: number): void {
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
function readValue(reader: Reader, kind: Kind, depth: number): unknown {
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
        const values = new Array<unknown>(count);

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
const ANY_INT: IntKind = Object.freeze({ type: 'int', anyInt: true });

/** A default as a field reads it, a fresh copy of a list or bytes. */
function defaultOf(field: FieldLayout): unknown {
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
function readFields(
  reader: Reader,
  fields: Layout,
  lenient: boolean,
  depth: number
): Record<string, unknown> {
  const count = reader.count(2);
  const values = new Array<unknown>(fields.list.length);
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

  for (let index = 0; index < fields.list.length; index++) {
    if (values[index] === undefined) {
      values[index] = missing(fields.list[index], lenient);
    }
  }

  if (fields.build === undefined) {
    fields.build = builderOf(fields);
  }

  if (fields.build !== null) {
    return fields.build(values);
  }

  const object: Record<string, unknown> = {};

  for (let index = 0; index < fields.list.length; index++) {
    const field = fields.list[index];

    if (fields.hasProto && field.name === '__proto__') {
      // Assigning it would set the object's prototype instead.
      Object.defineProperty(object, field.name, {
        value: values[index],
        writable: true,
        enumerable: true,
        configurable: true
      });
    } else {
      object[field.name] = values[index];
    }
  }

  return object;
}

/** The value of a field a record lacks: its default, or null if it may be. */
function missing(field: FieldLayout, lenient: boolean): unknown {
  if (field.default !== undefined) {
    return defaultOf(field);
  }

  if (field.optional || lenient) {
    return null;
  }

  throw corrupted(`it lacks the required field \`${field.name}\``);
}

/** The most fields a layout makes its objects with generated code for. */
const MAX_BUILT_FIELDS = 256;

/**
 * A function that makes an object of the layout `fields` from its fields'
 * values, in the layout's order, with one object literal; or `null`, where
 * `readFields` assigns the fields one by one instead.
 *
 * Assigning each field under its name is a store whose key changes from one
 * field to the next, which the JavaScript engine cannot specialise, and each
 * adds a property to the object; a literal makes the object whole, in its
 * final shape, several times faster. The names come from the file, so they
 * reach the generated code only through `JSON.stringify`, which keeps each
 * inside its string literal, and nothing else from the file is written into
 * it. A layout with a field named `__proto__` is not generated, since that
 * key in a literal sets the object's prototype; nor is one in a process that
 * forbids making code from strings.
 */
function builderOf(fields: Layout): Builder | null {
  if (fields.hasProto || fields.list.length > MAX_BUILT_FIELDS) {
    return null;
  }

  const entries = fields.list.map(
    (field, index) => `${JSON.stringify(field.name)}: values[${index}]`
  );

  try {
    // The body below takes `values` and returns an object literal.
    return new Function('values', `'use strict';\nreturn { ${entries.join(', ')} };`) as Builder;
  } catch (error) {
    // Only a process that forbids it fails to make the function.
    if (error instanceof EvalError) {
      return null;
    }

    throw error;
  }
}

/** The object whose record is `bytes`. */
function decodeRecord(
  collection: CollectionLayout,
  bytes: Uint8Array,
  lenient = false
): Record<string, unknown> {
  const reader = new Reader(bytes);
  const object = readFields(reader, collection.fields, lenient, 0);

  if (reader.at !== reader.end) {
    throw corrupted('it has bytes after its last field');
  }

  return object;
}

/** The objects of records one after another, each after its length. */
function decodeRecords(collection: CollectionLayout, bytes: Uint8Array): Record<string, unknown>[] {
  const reader = new Reader(bytes);
  // One reader for every record, moved from each to the next.
  const record = new Reader(bytes, 0, 0);
  const objects: Record<string, unknown>[] = [];

  while (reader.at < reader.end) {
    const length = reader.count();

    record.restart(reader.at, reader.at + length);

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
function readAny(reader: Reader, depth: number): Tagged {
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
      const values: Tagged[] = [];

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

function readAnyFields(reader: Reader, depth: number): AnyRecord {
  const count = reader.count(2);
  const fields: AnyRecord = new Map();

  for (let index = 0; index < count; index++) {
    fields.set(reader.varint(), readAny(reader, depth));
  }

  if (reader.at !== reader.end) {
    throw corrupted('it has bytes after its last field');
  }

  return fields;
}

/** A tagged value as the JavaScript value it holds. */
function untag(tagged: Tagged): unknown {
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
function field<T extends Tagged['tag']>(record: AnyRecord, id: number, tag: T): TaggedValue<T> {
  const value = record.get(id);

  if (value === undefined || value.tag !== tag) {
    throw corrupted(`the schema lacks a ${tag} in field ${id}`);
  }

  // The check above holds its tag to `tag`, but a type parameter does not
  // narrow the union.
  return value.value as TaggedValue<T>;
}

/** The fields of a collection or an embedded object, ready to read and write. */
function fieldsOf(list: FieldLayout[]): Layout {
  list.sort((a, b) => a.id - b.id);

  return {
    list,
    positions: new Map(list.map((field, index) => [field.id, index])),
    names: new Set(list.map((field) => field.name)),
    hasProto: list.some((field) => field.name === '__proto__'),
    // Made by `builderOf` when the first object of the layout is read.
    build: undefined
  };
}

/**
 * The collections of a stored schema's record: for each, its fields with
 * their ids and types, its key, and its indexes, with links resolved to the
 * collections they name.
 */
function decodeSchema(bytes: Uint8Array): SchemaLayout {
  try {
    return decodeSchemaFields(bytes);
  } catch (error) {
    // What fails here is an `Error`: one of this package's, with a code, or
    // one the language throws where a value has the wrong type.
    if ((error as CodeError).code !== undefined) {
      throw error;
    }

    throw corrupted(`the stored schema does not decode: ${(error as Error).message}`);
  }
}

/** A link kind while its schema is read, until its target is resolved. */
type PendingLink = { type: 'link'; target: CollectionLayout | null | undefined };

function decodeSchemaFields(bytes: Uint8Array): SchemaLayout {
  const record = readAnyFields(new Reader(bytes), 0);
  const byId = new Map<number | bigint, CollectionLayout>();
  const links: [PendingLink, number | bigint][] = [];

  const kindOf = (raw: AnyRecord): Kind => {
    // An int read as a `bigint` is beyond 2^53 and names no kind: indexing
    // with it reads `undefined`, as with any unknown code.
    const type = KIND_NAMES[field(raw, 1, 'int') as number];

    switch (type) {
      case 'link': {
        const kind: PendingLink = { type, target: null };

        links.push([kind, field(raw, 2, 'int')]);

        // Its target is resolved once every collection has been read.
        return kind as LinkKind;
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
  const fieldOf = (raw: Tagged): FieldLayout => {
    // A value that is not an object fails at `get`, which `decodeSchema`
    // reports as damage. An id read as a `bigint`, beyond 2^53, fails the
    // sort in `fieldsOf` beside any other id, and `readFields` refuses a
    // record that holds it.
    const fields = raw.value as AnyRecord;
    const defaultValue = fields.get(5);

    return {
      id: field(fields, 1, 'int') as number,
      name: field(fields, 2, 'string'),
      kind: kindOf(field(fields, 3, 'object')),
      optional: field(fields, 4, 'bool'),
      default: defaultValue === undefined ? undefined : untag(defaultValue)
    };
  };
  const collections = new Map<string, CollectionLayout>();

  for (const raw of field(record, 3, 'list')) {
    // A value that is not an object fails at `get`, which `decodeSchema`
    // reports as damage.
    const fields = raw.value as AnyRecord;
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

    // Its key has been found just above.
    byId.set(collection.id, collection as CollectionLayout);
    collections.set(collection.name, collection as CollectionLayout);
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
 * An entry of a record `encodeSchema` writes: the field id, what the value
 * is, and the value, or for `raw` the function that writes it.
 */
type Entry =
  | [number, 'int', number | bigint]
  | [number, 'bool', boolean]
  | [number, 'string', string]
  | [number, 'type', Spec]
  | [number, 'fields', DeclaredFields]
  | [number, 'default', Spec]
  | [number, 'raw', (writer: Writer) => void];

/** A field `encodeSchema` writes: its id, its name and its declared type. */
type FieldEntry = [number, string, Spec];

/**
 * The record of a schema declared with this package, encoded as the file
 * stores a schema, with ids given in the order of declaration. The engine
 * reads it with `Schema::decode` and gives the file's ids its own way.
 */
function encodeSchema(declared: DeclaredSchema): Uint8Array {
  const names = Object.keys(declared.collections);
  const ids = new Map(names.map((name, index) => [name, index + 1]));
  let nextIndex = 1;
  const writer = new Writer(1024);

  const writeType = (w: Writer, spec: Spec, where: string): void => {
    const entries: Entry[] = [[1, 'int', KIND_CODES[spec.type]]];

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
  const writeFieldList = (
    w: Writer,
    fields: FieldEntry[],
    where: string,
    embedded: boolean
  ): void => {
    w.byte(LIST);
    w.varint(fields.length);

    for (const [id, name, spec] of fields) {
      if (embedded && (spec.index || spec.unique || spec.primaryKey)) {
        throw invalid(
          `\`${where}.${name}\` is inside an embedded object, where no field is a key or indexed`
        );
      }

      const entries: Entry[] = [
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
  const writeEntries = (w: Writer, entries: Entry[], where: string): void => {
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
            Object.entries(value).map(([name, type], index): FieldEntry => [
              index + 1,
              name,
              type.spec
            ]),
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
    const fields = declaredFields.map(([fieldName, type], index): FieldEntry => [
      index + offset,
      fieldName,
      type.spec
    ]);

    if (auto) {
      fields.unshift([1, 'id', { type: 'int', optional: false }]);
    }

    // Without `auto`, `keys[0]` is one of the fields.
    const key = auto ? 1 : fields.find(([, fieldName]) => fieldName === keys[0][0])![0];
    const indexes = fields
      .filter(([, , spec]) => spec.index || spec.unique)
      .map(([id, , spec]): [number, number, boolean] => [nextIndex++, id, spec.unique === true]);

    // Every name has an id.
    return { id: ids.get(name)!, name, fields, key, auto, indexes };
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
function kindOfSpec(spec: Spec, ids: Map<string, number>): Kind {
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
  declare readonly index: number;

  constructor(index: number) {
    this.index = index;
    Object.freeze(this);
  }
}

/** Writes a value a query compares with, by its JavaScript type, or a parameter. */
function writeQueryValue(writer: Writer, value: unknown): void {
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

function writePath(writer: Writer, path: string[]): void {
  writer.byte(LIST);
  writer.varint(path.length);

  for (const name of path) {
    writer.byte(STRING);
    writer.string(name);
  }
}

/** How deeply a filter may nest, as the engine holds every query to. */
const MAX_FILTER_DEPTH = 24;

function writeExpression(writer: Writer, node: FilterNode, depth = 1): void {
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
function encodeParameters(parameters: readonly unknown[]): Buffer {
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
function encodeQuery(collection: string, query: QueryParts, count: boolean, lend = false): Buffer {
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
        // Entry 2 is there only with a filter.
        writeExpression(writer, query.filter!);
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
        // Entry 5 is there only with a limit.
        writer.int(query.limit!);
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

export {
  Reader,
  Param,
  codeError,
  invalid,
  encodeRecords,
  lendRecords,
  decodeRecord,
  decodeRecords,
  decodeSchema,
  encodeSchema,
  encodeQuery,
  encodeParameters
};
