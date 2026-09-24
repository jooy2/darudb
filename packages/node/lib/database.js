'use strict';

/**
 * The database and its transactions: `Database`, the read and write
 * transactions a function runs in, the collections they reach, and
 * `Migrating`, which a migration function gets.
 *
 * Transactions are synchronous and scoped to a function: `read` and `write`
 * begin one, run the function, and end it, committing a write when the
 * function returns and aborting it when the function throws. Nothing can
 * leave a transaction open, and so hold the writer lock, by forgetting it.
 */

const native = require('../native.js');
const {
  codeError,
  invalid,
  encodeRecords,
  decodeRecord,
  decodeRecords,
  decodeSchema,
  encodeSchema,
  encodeQuery
} = require('./codec');
const { Query } = require('./query');

/** Kept from users, so that a `Database` comes only from `Database.open`. */
const CREATE = Symbol('create');

/** Where a collection keeps its transaction and its layout, out of sight. */
const TXN = Symbol('txn');
const LAYOUT = Symbol('layout');

/** A view of `bytes` as the `Buffer` the native layer takes, without a copy. */
function toBuffer(bytes) {
  return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.length);
}

/** Refuses a promise where a transaction's function returns. */
function synchronous(result) {
  if (result !== null && typeof result === 'object' && typeof result.then === 'function') {
    throw invalid(
      "a transaction's function returned a promise; transactions are synchronous, so it has to finish before it returns"
    );
  }
}

/** A migration as the native layer takes it: everything but its function. */
function nativeMigration(migration) {
  if (typeof migration !== 'object' || migration === null) {
    throw invalid('a migration is an object with a `version`');
  }

  const list = (value, name) => {
    if (value === undefined) {
      return [];
    }

    if (!Array.isArray(value)) {
      throw invalid(`a migration's \`${name}\` is an array`);
    }

    return value;
  };

  if (migration.run !== undefined && typeof migration.run !== 'function') {
    throw invalid("a migration's `run` is a function");
  }

  return {
    version: migration.version,
    renameCollections: list(migration.renameCollections, 'renameCollections'),
    renameFields: list(migration.renameFields, 'renameFields'),
    deleteCollections: list(migration.deleteCollections, 'deleteCollections'),
    replaceFields: list(migration.replaceFields, 'replaceFields')
  };
}

/** The IR of a query given in any of the forms `find` and `count` take. */
function irOf(collection, query, parameters, count) {
  if (query === undefined || query === null) {
    return toBuffer(encodeQuery(collection, new Query().parts(), count));
  }

  if (typeof query === 'string') {
    if (parameters !== undefined && !Array.isArray(parameters)) {
      throw codeError('INVALID_QUERY', "a query's parameters are an array");
    }

    return native.parseQuery(collection, query, parameters ?? [], count);
  }

  let built = query;

  if (typeof query === 'function') {
    const fresh = new Query();

    built = query(fresh) ?? fresh;
  }

  if (!(built instanceof Query)) {
    throw codeError(
      'INVALID_QUERY',
      'a query is a function that builds one, a `Query`, or text in the query language'
    );
  }

  return toBuffer(encodeQuery(collection, built.parts(), count));
}

/** A collection of a transaction, for reading its objects. */
class ReadCollection {
  #txn;
  #layout;

  constructor(txn, layout) {
    this.#txn = txn;
    this.#layout = layout;
  }

  /** The collection's name. */
  get name() {
    return this.#layout.name;
  }

  /** The object whose primary key is `key`, or `null`. */
  get(key) {
    const record = this.#txn.getRecord(this.#layout.name, key);

    return record === null ? null : decodeRecord(this.#layout, record);
  }

  /** The objects a query finds, in its order; every object without one. */
  find(query, parameters) {
    const records = this.#txn.find(irOf(this.#layout.name, query, parameters, false));

    return decodeRecords(this.#layout, records);
  }

  /** The first object a query finds, or `null`. */
  findOne(query, parameters) {
    if (typeof query === 'string') {
      return this.find(query, parameters)[0] ?? null;
    }

    const first = new Query();

    if (typeof query === 'function') {
      query(first);
    } else if (query !== undefined && query !== null) {
      throw codeError('INVALID_QUERY', '`findOne` takes a function that builds a query, or text');
    }

    return this.find(first.limit(1))[0] ?? null;
  }

  /** How many objects a query finds, after its offset and within its limit. */
  count(query, parameters) {
    return this.#txn.count(irOf(this.#layout.name, query, parameters, true));
  }

  get [TXN]() {
    return this.#txn;
  }

  get [LAYOUT]() {
    return this.#layout;
  }
}

/** A collection of a write transaction, for reading and writing its objects. */
class WriteCollection extends ReadCollection {
  /** Inserts `object` and returns its primary key. */
  insert(object) {
    return this.insertMany([object])[0];
  }

  /**
   * Inserts `objects`, in one call into the engine, and returns their keys.
   * A refused object stops the batch with its error, and the objects before
   * it stay inserted in the transaction.
   */
  insertMany(objects) {
    return this.#write(objects, false);
  }

  /** Inserts `object`, or replaces the object with its key. */
  put(object) {
    return this.putMany([object])[0];
  }

  /** Inserts or replaces `objects`; see `insertMany`. */
  putMany(objects) {
    return this.#write(objects, true);
  }

  /** Deletes the object whose primary key is `key`, and says whether there was one. */
  delete(key) {
    return this[TXN].delete(this[LAYOUT].name, key);
  }

  #write(objects, replace) {
    if (!Array.isArray(objects)) {
      throw invalid('a batch of objects is an array');
    }

    if (objects.length === 0) {
      return [];
    }

    const records = encodeRecords(this[LAYOUT], objects);

    return this[TXN].writeRecords(this[LAYOUT].name, toBuffer(records), replace);
  }
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

/** A read transaction: one commit, for as long as its function runs. */
class ReadTransaction {
  #txn;
  #layout;

  constructor(txn, layout) {
    this.#txn = txn;
    this.#layout = layout;
  }

  /** Collection `name` of the schema, for reading. */
  collection(name) {
    return new ReadCollection(this.#txn, collectionOf(this.#layout, name));
  }
}

/** A write transaction: changes that commit together when its function returns. */
class WriteTransaction {
  #txn;
  #layout;

  constructor(txn, layout) {
    this.#txn = txn;
    this.#layout = layout;
  }

  /** Collection `name` of the schema, for reading and writing. */
  collection(name) {
    return new WriteCollection(this.#txn, collectionOf(this.#layout, name));
  }
}

/**
 * The write transaction of a migration, as a migration function gets it: the
 * collections of the new schema, and the objects as the schema before read
 * them.
 */
class Migrating {
  #opening;
  #layout;
  #previous;
  #version;

  constructor(opening, layout, previous, version) {
    this.#opening = opening;
    this.#layout = layout;
    this.#previous = previous;
    this.#version = version;
  }

  /** The schema version the file held before the migration. */
  get previousVersion() {
    return this.#opening.previousVersion;
  }

  /** The version this step migrates to. */
  get version() {
    return this.#version;
  }

  /** Collection `name` of the new schema, for reading and writing. */
  collection(name) {
    return new WriteCollection(this.#opening, collectionOf(this.#layout, name));
  }

  /**
   * The object of `collection` whose key is `key`, as the schema before the
   * migration reads it, with the names it gave and the values of fields the
   * migration removed or replaced. Read an object this way before writing it:
   * a written object keeps only the new schema's fields.
   */
  previous(collection, key) {
    const layout = collectionOf(this.#previous, collection);
    const record = this.#opening.previousRecord(collection, key);

    return record === null ? null : decodeRecord(layout, record, true);
  }

  /** The keys of every object of `collection`, named as before the migration. */
  previousKeys(collection) {
    collectionOf(this.#previous, collection);

    return this.#opening.previousKeys(collection);
  }
}

/** An open database. Created with `Database.open`. */
class Database {
  #native;
  #path;
  #layout;

  constructor(token, database, path, layout) {
    if (token !== CREATE) {
      throw invalid('a database is opened with `Database.open`');
    }

    this.#native = database;
    this.#path = path;
    this.#layout = layout;
  }

  /**
   * Opens the database at `path`, creating it if nothing exists there, and
   * stores, checks or migrates its schema.
   */
  static open(path, options = {}) {
    const { create, pageSize, schema, migrations = [] } = options;

    if (!Array.isArray(migrations)) {
      throw invalid('`migrations` is an array');
    }

    const opening = native.NativeOpening.open(path, {
      create,
      pageSize,
      schema: schema === undefined ? undefined : toBuffer(encodeSchema(schema)),
      migrations: migrations.map(nativeMigration)
    });
    let database;

    try {
      if (opening.isMigrating) {
        const layout = decodeSchema(opening.schemaRecord);
        const previous = decodeSchema(opening.previousSchemaRecord);
        const runs = new Map(
          migrations
            .filter((migration) => migration.run !== undefined)
            .map((migration) => [migration.version, migration.run])
        );
        let version;

        while ((version = opening.nextStep()) !== null) {
          const run = runs.get(version);

          if (run !== undefined) {
            synchronous(run(new Migrating(opening, layout, previous, version)));
          }
        }
      }

      database = opening.finish();
    } catch (error) {
      opening.abort();
      throw error;
    }

    const record = database.schemaRecord;

    return new Database(CREATE, database, path, record === null ? null : decodeSchema(record));
  }

  /** The path the database was opened at. Still readable after `close`. */
  get path() {
    return this.#path;
  }

  /** Whether `close` has not been called. */
  get isOpen() {
    return this.#native !== null;
  }

  /** The size of every page in the file, in bytes. */
  get pageSize() {
    return this.#database().pageSize;
  }

  /** The file format version recorded in the file. */
  get formatVersion() {
    return this.#database().formatVersion;
  }

  /** The schema version the file holds, or `null` without a schema. */
  get schemaVersion() {
    this.#database();

    return this.#layout === null ? null : this.#layout.version;
  }

  /**
   * Runs `fn` in a read transaction, which sees one commit for as long as
   * `fn` runs, and returns what `fn` returns.
   */
  read(fn) {
    const txn = this.#database().beginRead();

    try {
      const result = fn(new ReadTransaction(txn, this.#layout));

      synchronous(result);

      return result;
    } finally {
      txn.end();
    }
  }

  /**
   * Runs `fn` in a write transaction, and commits it when `fn` returns, or
   * aborts it when `fn` throws. With `durability: 'deferred'`, the commit
   * returns without waiting for the disk.
   */
  write(fn, options = {}) {
    const durability = options.durability ?? 'sync';

    if (durability !== 'sync' && durability !== 'deferred') {
      throw invalid("`durability` is `'sync'` or `'deferred'`");
    }

    const txn = this.#database().beginWrite();
    let committed = false;

    try {
      const result = fn(new WriteTransaction(txn, this.#layout));

      synchronous(result);
      txn.commit(durability === 'deferred');
      committed = true;

      return result;
    } finally {
      if (!committed) {
        txn.abort();
      }
    }
  }

  /** Makes every commit durable, deferred ones included. */
  sync() {
    this.#database().sync();
  }

  /**
   * Makes deferred commits durable and closes the database. Closing one that
   * is closed does nothing.
   */
  close() {
    if (this.#native !== null) {
      const database = this.#native;

      this.#native = null;
      database.close();
    }
  }

  #database() {
    if (this.#native === null) {
      throw codeError('CLOSED', 'the database has been closed');
    }

    return this.#native;
  }
}

module.exports = { Database };
