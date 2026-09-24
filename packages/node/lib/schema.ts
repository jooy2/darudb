/**
 * Declaring a schema: `t` for the types of fields, `collection` and
 * `schema`. A declaration is data; `codec.encodeSchema` turns it into the
 * record the engine reads, and the engine checks it, so the rules about what
 * a schema may hold live in one place.
 */

import { invalid } from './codec.js';

/**
 * A declared field's type and how the field holds it, as `t` and the
 * methods of `FieldType` make it.
 */
export type Spec = (
  | { type: 'bool' }
  | { type: 'int'; big?: boolean }
  | { type: 'float' }
  | { type: 'string' }
  | { type: 'bytes' }
  | { type: 'link'; target: string }
  | { type: 'list'; element: Spec }
  | { type: 'object'; fields: DeclaredFields }
) & {
  optional?: boolean;
  default?: unknown;
  index?: boolean;
  unique?: boolean;
  primaryKey?: boolean;
};

/** The declared fields of a collection or an embedded object, by name. */
export type DeclaredFields = Readonly<Record<string, FieldType>>;

/** A declared collection, as `collection` makes it. */
export interface DeclaredCollection {
  readonly fields: DeclaredFields;
  readonly [COLLECTION]: boolean;
}

/** A declared schema, as `schema` makes it. */
export interface DeclaredSchema {
  readonly version: number;
  readonly collections: Readonly<Record<string, DeclaredCollection>>;
}

/** The types a primary key can have. */
const KEY_TYPES = new Set(['int', 'string', 'bytes']);

/**
 * A field's type, with how the field holds it. Each method returns a copy,
 * and refuses at once what the engine would refuse when the file opens.
 */
class FieldType {
  declare readonly spec: Spec;

  constructor(spec: Spec) {
    this.spec = Object.freeze(spec);
    Object.freeze(this);
  }

  /** The field may be null, and is null when left out. */
  optional(): FieldType {
    if (this.spec.primaryKey) {
      throw invalid('a primary key is required, never optional');
    }

    return new FieldType({ ...this.spec, optional: true });
  }

  /** The field is required, and holds `value` when left out. */
  default(value: unknown): FieldType {
    if (this.spec.type === 'link' || this.spec.type === 'object') {
      throw invalid(`a field of type ${this.spec.type} has no default`);
    }

    if (this.spec.primaryKey) {
      throw invalid('a primary key has no default: every object brings its own');
    }

    return new FieldType({ ...this.spec, default: value });
  }

  /** Queries on the field read an index rather than every object. */
  index(): FieldType {
    return new FieldType({ ...this.spec, index: true });
  }

  /** An index that also refuses two objects with the same value. */
  unique(): FieldType {
    return new FieldType({ ...this.spec, unique: true });
  }

  /** The field is the collection's primary key: an int, a string or bytes. */
  primaryKey(): FieldType {
    if (!KEY_TYPES.has(this.spec.type)) {
      throw invalid(`a primary key is an int, a string or bytes, not a ${this.spec.type}`);
    }

    if (this.spec.optional || this.spec.default !== undefined) {
      throw invalid('a primary key is required, without a default');
    }

    return new FieldType({ ...this.spec, primaryKey: true });
  }
}

function fieldTypes(fields: Record<string, FieldType>, where: string): DeclaredFields {
  if (typeof fields !== 'object' || fields === null) {
    throw invalid(`${where} takes an object of field types`);
  }

  for (const [name, type] of Object.entries(fields)) {
    if (!(type instanceof FieldType)) {
      throw invalid(`\`${name}\` in ${where} is not a type from \`t\``);
    }
  }

  return Object.freeze({ ...fields });
}

/** The types of fields. */
const t = Object.freeze({
  bool: () => new FieldType({ type: 'bool' }),
  int: () => new FieldType({ type: 'int' }),
  /** An int read as a `bigint` always, for values beyond 2^53. */
  bigint: () => new FieldType({ type: 'int', big: true }),
  float: () => new FieldType({ type: 'float' }),
  string: () => new FieldType({ type: 'string' }),
  bytes: () => new FieldType({ type: 'bytes' }),
  /** The primary key of an object of collection `collection`. */
  link: (collection: string) => new FieldType({ type: 'link', target: collection }),
  /** A list of values of `element`, a scalar type or a link. */
  list: (element: FieldType) => {
    if (!(element instanceof FieldType)) {
      throw invalid('`t.list` takes a type from `t`');
    }

    return new FieldType({ type: 'list', element: element.spec });
  },
  /** An embedded object with fields of its own. */
  object: (fields: Record<string, FieldType>) =>
    new FieldType({ type: 'object', fields: fieldTypes(fields, '`t.object`') })
});

/** Marks what `collection` made, so that `schema` takes nothing else. */
const COLLECTION = Symbol('collection');

/**
 * A collection: its fields by name. A field marked `primaryKey()` is the
 * key; without one, the collection gets an `id` that the engine numbers.
 */
function collection(fields: Record<string, FieldType>): DeclaredCollection {
  return Object.freeze({ fields: fieldTypes(fields, '`collection`'), [COLLECTION]: true });
}

/** The collections of a database, at `version`, from 1 up. */
function schema(version: number, collections: Record<string, DeclaredCollection>): DeclaredSchema {
  if (!Number.isSafeInteger(version) || version < 1) {
    throw invalid('a schema version is a whole number from 1 up');
  }

  if (typeof collections !== 'object' || collections === null) {
    throw invalid('`schema` takes an object of collections');
  }

  for (const [name, value] of Object.entries(collections)) {
    if (value === null || typeof value !== 'object' || value[COLLECTION] !== true) {
      throw invalid(`\`${name}\` in \`schema\` is not made by \`collection\``);
    }
  }

  return Object.freeze({ version, collections: Object.freeze({ ...collections }) });
}

export { t, collection, schema, FieldType };
