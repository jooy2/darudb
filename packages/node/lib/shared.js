'use strict';

/**
 * What the synchronous and the asynchronous API share: checking what a
 * caller passes before it reaches the native layer, building the IR of a
 * query in any form it is given, preparing queries and keeping the texts
 * prepared, and the layouts of a stored schema that records are read and
 * written with.
 */

const native = require('../native.js');
const { codeError, invalid, decodeSchema, encodeQuery, encodeParameters } = require('./codec');
const { Query } = require('./query');

/** A view of `bytes` as the `Buffer` the native layer takes, without a copy. */
function toBuffer(bytes) {
  return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.length);
}

/**
 * The buffer synchronous reads are copied into, reused: the native layer
 * returns how many bytes it wrote there, or a `Buffer` of their own when
 * they do not fit. What it holds is decoded at once, before the next call
 * writes it again; decoding copies out every string and byte value.
 */
const scratch = Buffer.allocUnsafe(1 << 16);

/** The bytes a synchronous read delivered, as `scratch` says. */
function delivered(out) {
  return typeof out === 'number' ? scratch.subarray(0, out) : out;
}

/** Refuses a promise where a transaction's function returns. */
function synchronous(result) {
  if (result !== null && typeof result === 'object' && typeof result.then === 'function') {
    // The function goes on running after the transaction has ended, and
    // fails there; its rejection is this error's, not an unhandled one.
    result.then(undefined, () => {});

    throw invalid(
      "a transaction's function returned a promise; transactions are synchronous, so it has to finish before it returns"
    );
  }
}

/** The largest schema version the engine's migrations take. */
const MAX_VERSION = 2 ** 32 - 1;

/** A migration as the native layer takes it: everything but its function. */
function nativeMigration(migration) {
  if (typeof migration !== 'object' || migration === null) {
    throw invalid('a migration is an object with a `version`');
  }

  if (
    !Number.isSafeInteger(migration.version) ||
    migration.version < 1 ||
    migration.version > MAX_VERSION
  ) {
    throw invalid(`a migration's version is a whole number from 1 to ${MAX_VERSION}`);
  }

  // Each entry is an array of `size` names.
  const list = (value, name, size) => {
    if (value === undefined) {
      return [];
    }

    const fits = (entry) =>
      size === 1
        ? typeof entry === 'string'
        : Array.isArray(entry) &&
          entry.length === size &&
          entry.every((part) => typeof part === 'string');

    if (!Array.isArray(value) || !value.every(fits)) {
      throw invalid(
        `a migration's \`${name}\` is an array of ${size === 1 ? 'names' : `arrays of ${size} names`}`
      );
    }

    return value;
  };

  if (migration.run !== undefined && typeof migration.run !== 'function') {
    throw invalid("a migration's `run` is a function");
  }

  return {
    version: migration.version,
    renameCollections: list(migration.renameCollections, 'renameCollections', 2),
    renameFields: list(migration.renameFields, 'renameFields', 3),
    deleteCollections: list(migration.deleteCollections, 'deleteCollections', 1),
    replaceFields: list(migration.replaceFields, 'replaceFields', 2)
  };
}

/** Whether `value` is a value the native layer takes as a key or a parameter. */
function isScalar(value) {
  return (
    typeof value === 'number' ||
    typeof value === 'bigint' ||
    typeof value === 'string' ||
    typeof value === 'boolean' ||
    value instanceof Uint8Array
  );
}

/** Refuses what cannot be a primary key before it reaches the native layer. */
function keyOf(key) {
  if (!isScalar(key) || typeof key === 'boolean') {
    throw invalid(
      `a primary key is an int, a string or bytes, not ${key === null ? 'null' : typeof key}`
    );
  }

  return key;
}

/**
 * A query's parameters, checked and encoded as the engine reads them. The
 * buffer is lent, as `encodeParameters` says.
 */
function parametersOf(parameters) {
  if (parameters === undefined) {
    return encodeParameters([]);
  }

  if (!Array.isArray(parameters)) {
    throw codeError('INVALID_QUERY', "a query's parameters are an array");
  }

  for (const parameter of parameters) {
    if (parameter !== null && parameter !== undefined && !isScalar(parameter)) {
      throw codeError(
        'INVALID_QUERY',
        `a query's parameter is a single value, not ${typeof parameter}`
      );
    }
  }

  return encodeParameters(parameters);
}

/**
 * The IR of a query given in any of the forms `find` and `count` take. With
 * `first`, a query built here keeps only its first object; text leaves that
 * to the native layer, and so does a prepared query.
 */
function irOf(collection, query, parameters, count, first = false, lend = false) {
  const prepared = preparedOf(collection, query);

  if (prepared !== null) {
    return prepared.bind(parametersOf(parameters), count);
  }

  if (typeof query === 'string') {
    return native.parseQuery(collection, query, parametersOf(parameters), count);
  }

  let built = query ?? new Query();

  if (typeof query === 'function') {
    const fresh = new Query();

    built = query(fresh) ?? fresh;
  }

  if (!(built instanceof Query)) {
    throw codeError(
      'INVALID_QUERY',
      'a query is a function that builds one, a `Query`, text in the query language, or a prepared query'
    );
  }

  const parts = built.parts();

  if (first) {
    parts.limit = parts.limit === null ? 1 : Math.min(parts.limit, 1);
  }

  return encodeQuery(collection, parts, count, lend);
}

const PREPARE = Symbol('prepare');
const NATIVE = Symbol('native');

/**
 * A query parsed once, on one collection, that runs with values for its
 * parameters each time: `Database.prepare` makes one. It holds no database.
 */
class Prepared {
  #collection;
  #native;

  constructor(token, collection, prepared) {
    if (token !== PREPARE) {
      throw invalid('a query is prepared with `Database.prepare`');
    }

    this.#collection = collection;
    this.#native = prepared;
  }

  /** The collection the query runs on. */
  get collection() {
    return this.#collection;
  }

  get [NATIVE]() {
    return this.#native;
  }
}

/** Prepares `query`, text or built, on `collection`. */
function prepare(collection, query) {
  const prepared =
    typeof query === 'string'
      ? native.NativePrepared.fromText(collection, query)
      : native.NativePrepared.fromIr(irOf(collection, query, undefined, false, false, true));

  return new Prepared(PREPARE, collection, prepared);
}

/** How many texts `preparedOf` keeps prepared, and how long each may be. */
const KEPT_TEXTS = 256;
const KEPT_LENGTH = 4096;

/**
 * Texts of queries prepared once and kept, by text, each with the native
 * query and the collection it was prepared on. The oldest goes first when
 * the map is full. A longer text is parsed each time instead, so that what
 * is kept stays small.
 */
const texts = new Map();

/**
 * The native prepared query to run `query` on `collection` with: a
 * `Prepared`, which has to be on `collection`, or text, which is prepared
 * the first time and kept. `null` for a query to encode as IR.
 */
function preparedOf(collection, query) {
  if (query instanceof Prepared) {
    if (query.collection !== collection) {
      throw codeError(
        'INVALID_QUERY',
        `the query was prepared on \`${query.collection}\`, not on \`${collection}\``
      );
    }

    return query[NATIVE];
  }

  if (typeof query !== 'string' || query.length > KEPT_LENGTH) {
    return null;
  }

  const kept = texts.get(query);

  if (kept !== undefined && kept.collection === collection) {
    return kept.prepared;
  }

  const prepared = native.NativePrepared.fromText(collection, query);

  if (kept === undefined && texts.size >= KEPT_TEXTS) {
    texts.delete(texts.keys().next().value);
  }

  texts.set(query, { collection, prepared });

  return prepared;
}

/** Calls `mark` with every int kind among `fields`, in lists and embedded objects too. */
function eachInt(fields, mark, declared) {
  for (const field of fields.list) {
    const spec = declared === undefined ? undefined : declared[field.name]?.spec;
    let kind = field.kind;
    let specKind = spec;

    if (kind.type === 'list') {
      kind = kind.element;
      specKind = spec?.element;
    }

    if (kind.type === 'int') {
      mark(kind, specKind);
    } else if (kind.type === 'object') {
      eachInt(kind.fields, mark, specKind?.fields);
    }
  }
}

/**
 * The layout of the stored schema's record, with the fields declared with
 * `t.bigint()` marked, since the file stores one type of int.
 */
function layoutOf(record, declared) {
  const layout = decodeSchema(record);

  for (const [name, collection] of layout.collections) {
    const fields = declared?.collections[name]?.fields;

    eachInt(
      collection.fields,
      (kind, spec) => {
        if (spec?.big) {
          kind.big = true;
        }
      },
      fields
    );
  }

  return layout;
}

/** The layout of a schema no declaration describes: every int may be a `bigint`. */
function looseLayoutOf(record) {
  const layout = decodeSchema(record);

  for (const collection of layout.collections.values()) {
    eachInt(collection.fields, (kind) => {
      kind.anyInt = true;
    });
  }

  return layout;
}

/** The collection `name` of `layout`, or an error. */
function collectionOf(layout, name) {
  if (layout === null) {
    throw invalid(
      'the database was opened without a schema, so it has no collections; declare one with the `schema` option'
    );
  }

  const collection = layout.collections.get(name);

  if (collection === undefined) {
    throw invalid(`the schema has no collection called \`${name}\``);
  }

  return collection;
}

module.exports = {
  toBuffer,
  scratch,
  delivered,
  synchronous,
  nativeMigration,
  keyOf,
  parametersOf,
  irOf,
  prepare,
  preparedOf,
  layoutOf,
  looseLayoutOf,
  collectionOf
};
