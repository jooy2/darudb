'use strict';

/**
 * The asynchronous API's transactions and collections, and what keeps the
 * two APIs from waiting for each other.
 *
 * Every operation of an asynchronous transaction runs on the libuv thread
 * pool and resolves a promise. A transaction runs its operations in the
 * order they were called, whether or not each was awaited, sending those
 * called together to the pool as one batch (`Serial`), and ends only once
 * the last has settled; so an operation never meets a transaction that ended
 * under it.
 *
 * A write transaction holds the file's writer lock, and a second one on the
 * same file waits for it. On the thread pool that wait could take every
 * thread while the transaction holding the lock needs one to finish, so this
 * process's own writes on one file queue here instead, in JavaScript, and
 * reach the pool one at a time; so do `syncAsync` and `closeAsync`, which
 * wait for the writer when a deferred commit is not yet durable. A
 * synchronous write, `sync` or `close` while an asynchronous write holds the
 * lock would wait on the event loop that the asynchronous one needs, and is
 * refused at once.
 */

const { AsyncLocalStorage } = require('node:async_hooks');

const {
  Reader,
  codeError,
  invalid,
  encodeRecords,
  decodeRecord,
  decodeRecords
} = require('./codec');
const { toBuffer, keyOf, irOf, collectionOf } = require('./shared');

/**
 * Each file this process's writes are using or waiting for, by the key
 * `Database` gives it: the promise the last queued asynchronous write
 * settles, how many are queued, and how many writes hold the file's writer
 * lock here or are about to take it. A file leaves the map when nothing uses
 * it.
 */
const files = new Map();

/**
 * The asynchronous writes whose functions the current code runs inside, as
 * marks that stay open until each write's function settles. A callback the
 * function scheduled keeps the context after that, and its mark is closed by
 * then.
 */
const writing = new AsyncLocalStorage();

function fileOf(key) {
  let file = files.get(key);

  if (file === undefined) {
    file = { tail: Promise.resolve(), queued: 0, holders: 0 };
    files.set(key, file);
  }

  return file;
}

/** Counts a holder of the file, and returns the function that uncounts it. */
function hold(key) {
  const file = fileOf(key);
  let held = true;

  file.holders += 1;

  return () => {
    if (held) {
      held = false;
      file.holders -= 1;

      if (file.queued === 0 && file.holders === 0) {
        files.delete(key);
      }
    }
  };
}

/**
 * Runs `run`, synchronous work that may wait for this process's writer on
 * the file, as a holder of the file, refusing if a write holds it here:
 * `run` would wait for that one on the thread it needs to finish. `doing`
 * names the work in the error.
 */
function holdForSync(key, doing, run) {
  if ((files.get(key)?.holders ?? 0) > 0) {
    throw invalid(
      `${doing} would wait for the write transaction on this file that is under way in this process, which needs this thread to finish; write transactions do not nest`
    );
  }

  const release = hold(key);

  try {
    return run();
  } finally {
    release();
  }
}

/**
 * Takes a place in the queue of this process's asynchronous writes on the
 * file, and resolves to the function that lets the next one go once the
 * earlier ones have. Throws at once, rather than queueing, when the caller
 * runs inside a write on the file: it would wait for that one forever.
 */
function turn(key) {
  if (writing.getStore()?.some((mark) => mark.open && mark.key === key) === true) {
    throw invalid(
      'this would wait for the asynchronous write transaction on this file whose function calls it; write transactions do not nest'
    );
  }

  const file = fileOf(key);
  const before = file.tail;
  let next;
  const mine = new Promise((resolve) => {
    next = resolve;
  });

  file.queued += 1;
  file.tail = before.then(() => mine);

  return before.then(() => {
    const release = hold(key);

    file.queued -= 1;

    return () => {
      release();
      next();
    };
  });
}

/**
 * Runs `fn` inside the asynchronous write on the file `key`, for as long as
 * the promise it returns is pending, and resolves to what it resolves to.
 */
async function inside(key, fn) {
  const mark = { key, open: true };

  try {
    return await writing.run([...(writing.getStore() ?? []), mark], fn);
  } finally {
    mark.open = false;
  }
}

/** Throws what a native promise resolved to, if it is a failure. */
function settle(value) {
  if (
    value !== null &&
    typeof value === 'object' &&
    !(value instanceof Uint8Array) &&
    !Array.isArray(value) &&
    typeof value.code === 'string' &&
    typeof value.message === 'string'
  ) {
    throw codeError(value.code, value.message);
  }

  return value;
}

// What each operation of a batch is, as `src/lib.rs` numbers them.
const GET = 0;
const FIND = 1;
const FIND_FIRST = 2;
const COUNT = 3;
const INSERT = 4;
const PUT = 5;
const DELETE = 6;
const PREVIOUS_RECORD = 7;
const PREVIOUS_KEYS = 8;

// How `src/lib.rs` tags each result of a batch, and each key among them.
const TAG_BYTES = 0;
const TAG_NULL = 1;
const TAG_NUMBER = 2;
const TAG_FALSE = 3;
const TAG_TRUE = 4;
const TAG_KEYS = 5;
const TAG_FAILURE = 6;
const KEY_INT = 0;
const KEY_STRING = 1;
const KEY_BYTES = 2;

const SAFE = BigInt(Number.MAX_SAFE_INTEGER);

/**
 * One transaction's operations, run in the order they were called.
 *
 * Operations called in the same turn of the event loop, or while a batch is
 * on the thread pool, go to the engine together as the next batch, in one
 * call and one trip through the pool; the engine runs a batch's operations
 * one after another. A trip costs far more than most operations, so calling
 * many at once and awaiting them together costs little more than one, while
 * awaiting each in turn pays a trip for each.
 */
class Serial {
  #native;
  #queued = [];
  #scheduled = false;
  #running = false;
  #open = true;
  #idle = [];

  constructor(native) {
    this.#native = native;
  }

  /**
   * Queues an operation of kind `kind`, with the collection, key and bytes
   * it takes, and resolves to its result.
   */
  call(kind, collection, key, payload) {
    if (!this.#open) {
      return Promise.reject(codeError('CLOSED', 'the transaction has ended'));
    }

    return new Promise((resolve, reject) => {
      this.#queued.push({ kind, collection, key, payload, resolve, reject });

      if (!this.#running && !this.#scheduled) {
        this.#scheduled = true;
        queueMicrotask(() => {
          this.#scheduled = false;
          this.#send();
        });
      }
    });
  }

  /** Waits for every operation called so far to settle. */
  drain() {
    if (!this.#running && !this.#scheduled && this.#queued.length === 0) {
      return Promise.resolve();
    }

    return new Promise((resolve) => {
      this.#idle.push(resolve);
    });
  }

  /** Takes no more operations, and waits for the ones called to settle. */
  close() {
    this.#open = false;

    return this.drain();
  }

  #send() {
    const batch = this.#queued;

    this.#queued = [];

    if (batch.length === 0) {
      this.#rest();

      return;
    }

    this.#running = true;

    let sent;

    try {
      sent = this.#native.runAsync(
        Buffer.from(batch.map((op) => op.kind)),
        batch.map((op) => op.collection),
        batch.map((op) => op.key),
        batch.map((op) => op.payload)
      );
    } catch (error) {
      sent = Promise.reject(error);
    }

    sent
      .then(settle)
      .then(
        (results) => deliver(batch, results),
        (error) => {
          for (const op of batch) {
            op.reject(error);
          }
        }
      )
      .finally(() => {
        this.#running = false;

        if (this.#queued.length > 0) {
          this.#send();
        } else {
          this.#rest();
        }
      });
  }

  #rest() {
    const idle = this.#idle;

    this.#idle = [];

    for (const resolve of idle) {
      resolve();
    }
  }
}

/** Settles each operation of `batch` with its result in `results`. */
function deliver(batch, results) {
  const reader = new Reader(results);

  for (const op of batch) {
    let result;

    try {
      result = readResult(reader, results);
    } catch (error) {
      // A batch the engine wrote wrongly: nothing after this can be trusted.
      for (const rest of batch.slice(batch.indexOf(op))) {
        rest.reject(error);
      }

      return;
    }

    if (result instanceof Failure) {
      op.reject(codeError(result.code, result.message));
    } else {
      op.resolve(result);
    }
  }
}

class Failure {
  constructor(code, message) {
    this.code = code;
    this.message = message;
  }
}

/**
 * The next result of a batch. Bytes come as a view of `results`, which the
 * caller decodes before the batch is gone; keys of bytes are copied.
 */
function readResult(reader, results) {
  switch (reader.byte()) {
    case TAG_BYTES: {
      const length = reader.count();
      const start = reader.at;

      reader.at += length;

      return results.subarray(start, reader.at);
    }
    case TAG_NULL:
      return null;
    case TAG_NUMBER:
      return reader.float();
    case TAG_FALSE:
      return false;
    case TAG_TRUE:
      return true;
    case TAG_KEYS: {
      const keys = new Array(reader.count());

      for (let index = 0; index < keys.length; index++) {
        keys[index] = readKey(reader, results);
      }

      return keys;
    }
    case TAG_FAILURE:
      return new Failure(reader.string(), reader.string());
    default:
      throw codeError('INTERNAL', 'a batch holds a result of no known kind');
  }
}

function readKey(reader, results) {
  switch (reader.byte()) {
    case KEY_INT: {
      if (reader.end - reader.at < 8) {
        throw codeError('INTERNAL', 'a batch ends inside a key');
      }

      const int = reader.view.getBigInt64(reader.at, true);

      reader.at += 8;

      return int >= -SAFE && int <= SAFE ? Number(int) : int;
    }
    case KEY_STRING:
      return reader.string();
    case KEY_BYTES: {
      const length = reader.count();
      const start = reader.at;

      reader.at += length;

      return Buffer.from(results.subarray(start, reader.at));
    }
    default:
      throw codeError('INTERNAL', 'a batch holds a key of no known kind');
  }
}

/** Where a collection keeps its operations and its layout, out of sight. */
const SERIAL = Symbol('serial');
const LAYOUT = Symbol('layout');

/** A collection of an asynchronous transaction, for reading its objects. */
class AsyncReadCollection {
  #serial;
  #layout;

  constructor(serial, layout) {
    this.#serial = serial;
    this.#layout = layout;
  }

  /** The collection's name. */
  get name() {
    return this.#layout.name;
  }

  /** The object whose primary key is `key`, or `null`. */
  async get(key) {
    const record = await this.#serial.call(GET, this.#layout.name, keyOf(key), null);

    return record === null ? null : decodeRecord(this.#layout, record);
  }

  /** The objects a query finds, in its order; every object without one. */
  async find(query, parameters) {
    const ir = irOf(this.#layout.name, query, parameters, false);

    return decodeRecords(this.#layout, await this.#serial.call(FIND, '', null, ir));
  }

  /** The first object a query finds, or `null`. The engine stops reading there. */
  async findOne(query, parameters) {
    const ir = irOf(this.#layout.name, query, parameters, false, true);
    const records = await this.#serial.call(FIND_FIRST, '', null, ir);

    return decodeRecords(this.#layout, records)[0] ?? null;
  }

  /** How many objects a query finds, after its offset and within its limit. */
  async count(query, parameters) {
    const ir = irOf(this.#layout.name, query, parameters, true);

    return this.#serial.call(COUNT, '', null, ir);
  }

  get [SERIAL]() {
    return this.#serial;
  }

  get [LAYOUT]() {
    return this.#layout;
  }
}

/** A collection of an asynchronous write transaction, for reading and writing. */
class AsyncWriteCollection extends AsyncReadCollection {
  /** Inserts `object` and resolves to its primary key. */
  async insert(object) {
    return (await this.insertMany([object]))[0];
  }

  /**
   * Inserts `objects` in one call into the engine, and resolves to their
   * keys. A refused object stops the batch with its error, and the objects
   * before it stay inserted in the transaction.
   */
  async insertMany(objects) {
    return this.#write(objects, false);
  }

  /** Inserts `object`, or replaces the object with its key. */
  async put(object) {
    return (await this.putMany([object]))[0];
  }

  /** Inserts or replaces `objects`; see `insertMany`. */
  async putMany(objects) {
    return this.#write(objects, true);
  }

  /** Deletes the object whose primary key is `key`, and resolves to whether there was one. */
  async delete(key) {
    return this[SERIAL].call(DELETE, this[LAYOUT].name, keyOf(key), null);
  }

  #write(objects, replace) {
    if (!Array.isArray(objects)) {
      throw invalid('a batch of objects is an array');
    }

    if (objects.length === 0) {
      return [];
    }

    const records = toBuffer(encodeRecords(this[LAYOUT], objects));

    return this[SERIAL].call(replace ? PUT : INSERT, this[LAYOUT].name, null, records);
  }
}

/** An asynchronous read transaction: one commit, for as long as its function runs. */
class AsyncReadTransaction {
  #serial;
  #layout;

  constructor(serial, layout) {
    this.#serial = serial;
    this.#layout = layout;
  }

  /** Collection `name` of the schema, for reading. */
  collection(name) {
    return new AsyncReadCollection(this.#serial, collectionOf(this.#layout, name));
  }
}

/** An asynchronous write transaction: changes that commit together when its function settles. */
class AsyncWriteTransaction {
  #serial;
  #layout;

  constructor(serial, layout) {
    this.#serial = serial;
    this.#layout = layout;
  }

  /** Collection `name` of the schema, for reading and writing. */
  collection(name) {
    return new AsyncWriteCollection(this.#serial, collectionOf(this.#layout, name));
  }
}

/**
 * The write transaction of a migration, as an asynchronous migration
 * function gets it: the collections of the new schema, and the objects as
 * the schema before the migration read them.
 */
class AsyncMigrating {
  #serial;
  #layout;
  #previous;
  #previousVersion;
  #version;

  // The versions come in as values: reading one from the native transaction
  // would wait on the event loop for whatever operation holds it.
  constructor(serial, layout, previous, previousVersion, version) {
    this.#serial = serial;
    this.#layout = layout;
    this.#previous = previous;
    this.#previousVersion = previousVersion;
    this.#version = version;
  }

  /** The schema version the file held before the migration. */
  get previousVersion() {
    return this.#previousVersion;
  }

  /** The version this step migrates to. */
  get version() {
    return this.#version;
  }

  /** Collection `name` of the new schema, for reading and writing. */
  collection(name) {
    return new AsyncWriteCollection(this.#serial, collectionOf(this.#layout, name));
  }

  /**
   * The object of `collection` whose key is `key`, as the schema before the
   * migration reads it. Read an object this way before writing it: a written
   * object keeps only the new schema's fields.
   */
  async previous(collection, key) {
    const layout = collectionOf(this.#previous, collection);
    const record = await this.#serial.call(PREVIOUS_RECORD, collection, keyOf(key), null);

    return record === null ? null : decodeRecord(layout, record, true);
  }

  /** The keys of every object of `collection`, named as before the migration. */
  async previousKeys(collection) {
    collectionOf(this.#previous, collection);

    return this.#serial.call(PREVIOUS_KEYS, collection, null, null);
  }
}

module.exports = {
  settle,
  hold,
  holdForSync,
  turn,
  inside,
  Serial,
  AsyncReadTransaction,
  AsyncWriteTransaction,
  AsyncMigrating
};
