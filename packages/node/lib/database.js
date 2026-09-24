'use strict';

/**
 * The database and its transactions: `Database`, the read and write
 * transactions a function runs in, the collections they reach, and
 * `Migrating`, which a migration function gets. `lib/async.js` has the
 * asynchronous API's transactions, which `Database` begins too.
 *
 * Transactions are scoped to a function: `read` and `write` begin one, run
 * the function, and end it, committing a write when the function returns and
 * aborting it when the function throws, and `readAsync` and `writeAsync` do
 * the same when the function settles. Nothing can leave a transaction open,
 * and so hold the writer lock, by forgetting it.
 */

const { realpathSync, statSync } = require('node:fs');
const { resolve } = require('node:path');

const native = require('../native.js');
const {
  codeError,
  invalid,
  encodeRecords,
  decodeRecord,
  decodeRecords,
  encodeSchema
} = require('./codec');
const {
  toBuffer,
  synchronous,
  nativeMigration,
  keyOf,
  irOf,
  layoutOf,
  looseLayoutOf,
  collectionOf
} = require('./shared');
const {
  settle,
  hold,
  holdForSync,
  turn,
  inside,
  Serial,
  AsyncReadTransaction,
  AsyncWriteTransaction,
  AsyncMigrating
} = require('./async');

/** Kept from users, so that a `Database` comes only from `Database.open`. */
const CREATE = Symbol('create');

/** Where a collection keeps its transaction and its layout, out of sight. */
const TXN = Symbol('txn');
const LAYOUT = Symbol('layout');

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
    const record = this.#txn.getRecord(this.#layout.name, keyOf(key));

    return record === null ? null : decodeRecord(this.#layout, record);
  }

  /** The objects a query finds, in its order; every object without one. */
  find(query, parameters) {
    const records = this.#txn.find(irOf(this.#layout.name, query, parameters, false), false);

    return decodeRecords(this.#layout, records);
  }

  /** The first object a query finds, or `null`. The engine stops reading there. */
  findOne(query, parameters) {
    const ir = irOf(this.#layout.name, query, parameters, false, true);

    return decodeRecords(this.#layout, this.#txn.find(ir, true))[0] ?? null;
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
    return this[TXN].delete(this[LAYOUT].name, keyOf(key));
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
  #txn;
  #layout;
  #previous;
  #version;

  constructor(txn, layout, previous, version) {
    this.#txn = txn;
    this.#layout = layout;
    this.#previous = previous;
    this.#version = version;
  }

  /** The schema version the file held before the migration. */
  get previousVersion() {
    return this.#txn.previousVersion;
  }

  /** The version this step migrates to. */
  get version() {
    return this.#version;
  }

  /** Collection `name` of the new schema, for reading and writing. */
  collection(name) {
    return new WriteCollection(this.#txn, collectionOf(this.#layout, name));
  }

  /**
   * The object of `collection` whose key is `key`, as the schema before the
   * migration reads it, with the names it gave and the values of fields the
   * migration removed or replaced. Read an object this way before writing it:
   * a written object keeps only the new schema's fields.
   */
  previous(collection, key) {
    const layout = collectionOf(this.#previous, collection);
    const record = this.#txn.previousRecord(collection, keyOf(key));

    return record === null ? null : decodeRecord(layout, record, true);
  }

  /** The keys of every object of `collection`, named as before the migration. */
  previousKeys(collection) {
    collectionOf(this.#previous, collection);

    return this.#txn.previousKeys(collection);
  }
}

/** What `open` and `openAsync` pass to the native layer, checked. */
function nativeOptions(options) {
  const { create, pageSize, busyTimeout, schema, migrations = [] } = options;

  if (!Array.isArray(migrations)) {
    throw invalid('`migrations` is an array');
  }

  return {
    create,
    pageSize,
    busyTimeout,
    schema: schema === undefined ? undefined : toBuffer(encodeSchema(schema)),
    migrations: migrations.map(nativeMigration)
  };
}

/** The migration functions of `options`, by the version they migrate to. */
function runsOf(options) {
  return new Map(
    (options.migrations ?? [])
      .filter((migration) => migration.run !== undefined)
      .map((migration) => [migration.version, migration.run])
  );
}

/**
 * The file at `path`, as the engine tells one open file from another: by
 * device and inode, or on Windows by its real path. Two handles to one file
 * get the same key however each named it, so their writes queue together.
 */
function fileKeyOf(path) {
  try {
    if (process.platform === 'win32') {
      return realpathSync.native(path);
    }

    const { dev, ino } = statSync(path, { bigint: true });

    return `${dev}:${ino}`;
  } catch {
    return resolve(path);
  }
}

/** An open database. Created with `Database.open` or `Database.openAsync`. */
class Database {
  #native;
  #path;
  #layout;
  #file;

  constructor(token, database, path, layout) {
    if (token !== CREATE) {
      throw invalid('a database is opened with `Database.open` or `Database.openAsync`');
    }

    this.#native = database;
    this.#path = path;
    this.#layout = layout;
    this.#file = fileKeyOf(path);
  }

  /**
   * Opens the database at `path`, creating it if nothing exists there, and
   * stores, checks or migrates its schema. Migration functions run
   * synchronously.
   */
  static open(path, options = {}) {
    const opening = native.NativeOpening.open(path, nativeOptions(options));
    let database;

    if (opening.isMigrating) {
      const txn = opening.migration();
      // The migration holds the writer lock, so a write on the file from
      // its functions would wait for it.
      const release = hold(fileKeyOf(path));

      try {
        const layout = layoutOf(txn.schemaRecord, options.schema);
        const previous = looseLayoutOf(txn.previousSchemaRecord);
        const runs = runsOf(options);
        let version;

        while ((version = txn.nextStep()) !== null) {
          const run = runs.get(version);

          if (run !== undefined) {
            synchronous(run(new Migrating(txn, layout, previous, version)));
          }
        }

        database = txn.finish();
      } catch (error) {
        txn.end();
        throw error;
      } finally {
        release();
      }
    } else {
      database = opening.database();
    }

    return Database.#of(database, path, options.schema);
  }

  /**
   * Opens the database at `path` like `open`, on the thread pool, and
   * resolves to it. Migration functions may be asynchronous, and get the
   * asynchronous API.
   */
  static async openAsync(path, options = {}) {
    const opening = settle(await native.NativeOpening.openAsync(path, nativeOptions(options)));
    let database;

    if (opening.isMigrating) {
      const txn = opening.migration();
      const serial = new Serial(txn);
      const file = fileKeyOf(path);
      const release = hold(file);

      try {
        const layout = layoutOf(txn.schemaRecord, options.schema);
        const previous = looseLayoutOf(txn.previousSchemaRecord);
        const previousVersion = txn.previousVersion;
        const runs = runsOf(options);
        let version;

        // Every operation has settled whenever `nextStep` runs, so it never
        // waits on the event loop for the transaction.
        while ((version = txn.nextStep()) !== null) {
          const run = runs.get(version);

          if (run !== undefined) {
            await inside(file, () =>
              run(new AsyncMigrating(serial, layout, previous, previousVersion, version))
            );
            await serial.drain();
          }
        }

        await serial.close();
        database = settle(await txn.finishAsync());
      } catch (error) {
        await serial.close();
        txn.end();
        throw error;
      } finally {
        release();
      }
    } else {
      database = opening.database();
    }

    return Database.#of(database, path, options.schema);
  }

  static #of(database, path, schema) {
    const record = database.schemaRecord;

    return new Database(CREATE, database, path, record === null ? null : layoutOf(record, schema));
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
    const deferred = deferredOf(options);
    const database = this.#database();

    return holdForSync(this.#file, 'a synchronous write transaction', () => {
      const txn = database.beginWrite();
      let committed = false;

      try {
        const result = fn(new WriteTransaction(txn, this.#layout));

        synchronous(result);
        txn.commit(deferred);
        committed = true;

        return result;
      } finally {
        if (!committed) {
          txn.end();
        }
      }
    });
  }

  /**
   * Runs `fn`, which may be asynchronous, in a read transaction whose
   * operations run on the thread pool, and resolves to what `fn` resolves
   * to. The transaction sees one commit until `fn` settles.
   */
  async readAsync(fn) {
    // Begun here rather than on the pool: a read waits for no writer, and
    // takes about as long as the trip there would, once the file's header
    // is in the operating system's cache.
    const txn = this.#database().beginRead();
    const serial = new Serial(txn);

    try {
      return await fn(new AsyncReadTransaction(serial, this.#layout));
    } finally {
      await serial.close();
      txn.end();
    }
  }

  /**
   * Runs `fn`, which may be asynchronous, in a write transaction whose
   * operations run on the thread pool, commits it when `fn` resolves, or
   * aborts it when `fn` rejects. This process's asynchronous writes on one
   * file run one after another.
   */
  async writeAsync(fn, options = {}) {
    const deferred = deferredOf(options);
    const database = this.#database();
    const release = await turn(this.#file);

    try {
      const txn = settle(await database.beginWriteAsync());
      const serial = new Serial(txn);
      let result;

      try {
        result = await inside(this.#file, () =>
          fn(new AsyncWriteTransaction(serial, this.#layout))
        );
      } catch (error) {
        await serial.close();
        txn.end();
        throw error;
      }

      await serial.close();
      settle(await txn.commitAsync(deferred));

      return result;
    } finally {
      release();
    }
  }

  // `sync` and `close` wait for the writer when a deferred commit is not yet
  // durable, so they follow the rules of a write: the synchronous ones are
  // refused while an asynchronous write holds the file, and the asynchronous
  // ones wait their turn after this process's writes.

  /** Makes every commit durable, deferred ones included. */
  sync() {
    const database = this.#database();

    holdForSync(this.#file, '`sync`', () => database.sync());
  }

  /** `sync` on the thread pool, after this process's writes on the file. */
  async syncAsync() {
    const database = this.#database();
    const release = await turn(this.#file);

    try {
      settle(await database.syncAsync());
    } finally {
      release();
    }
  }

  /**
   * Makes deferred commits durable and closes the database. Closing one that
   * is closed does nothing.
   */
  close() {
    if (this.#native !== null) {
      const database = this.#native;

      holdForSync(this.#file, '`close`', () => {
        this.#native = null;
        database.close();
      });
    }
  }

  /**
   * `close` on the thread pool, after this process's writes on the file.
   * The database is closed to new work at once.
   */
  async closeAsync() {
    if (this.#native !== null) {
      const database = this.#native;
      // Taken before the database closes to new work, so that a call from
      // inside a write's function is refused with the database still open.
      const turning = turn(this.#file);

      this.#native = null;

      const release = await turning;

      try {
        settle(await database.closeAsync());
      } finally {
        release();
      }
    }
  }

  #database() {
    if (this.#native === null) {
      throw codeError('CLOSED', 'the database has been closed');
    }

    return this.#native;
  }
}

/** Whether a write's options ask for a deferred commit. */
function deferredOf(options) {
  const durability = options.durability ?? 'sync';

  if (durability !== 'sync' && durability !== 'deferred') {
    throw invalid("`durability` is `'sync'` or `'deferred'`");
  }

  return durability === 'deferred';
}

module.exports = { Database };
