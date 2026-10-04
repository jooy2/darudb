/**
 * The database and its transactions: `Database`, the read and write
 * transactions a function runs in, the collections they reach, and
 * `Migrating`, which a migration function gets. `lib/async.ts` has the
 * asynchronous API's transactions, which `Database` begins too.
 *
 * Transactions are scoped to a function: `read` and `write` begin one, run
 * the function, and end it, committing a write when the function returns and
 * aborting it when the function throws, and `readAsync` and `writeAsync` do
 * the same when the function settles. Nothing can leave a transaction open,
 * and so hold the writer lock, by forgetting it.
 */

import { statSync } from 'node:fs';
import { resolve } from 'node:path';

import native = require('../native.js');
import {
  codeError,
  invalid,
  lendRecords,
  lendChanges,
  recordBytes,
  decodeRecord,
  decodeRecords,
  decodeFirst,
  encodeSchema,
  parameterBytes
} from './codec.js';
import type { CollectionLayout, SchemaLayout } from './codec.js';
import type { DeclaredSchema } from './schema.js';
import {
  toBuffer,
  scratch,
  synchronous,
  nativeMigration,
  keyOf,
  lendParameters,
  irOf,
  prepare,
  preparedOf,
  layoutOf,
  looseLayoutOf,
  nameOf,
  collectionOf
} from './shared.js';
import type { Migration, Prepared, QueryInput } from './shared.js';
import {
  settle,
  hold,
  holdForSync,
  turn,
  inside,
  Serial,
  AsyncReadTransaction,
  AsyncWriteTransaction,
  AsyncMigrating
} from './async.js';

/**
 * The options `open` and `openAsync` take, as `OpenOptions` and
 * `AsyncOpenOptions` in `index.d.ts` declare them. A migration's function
 * gets `M`.
 */
interface OpenOptions<M> extends Secret {
  create?: boolean;
  pageSize?: number;
  busyTimeout?: number;
  cacheSize?: number;
  schema?: DeclaredSchema;
  migrations?: Migration<M>[];
  passwordHashing?: { memoryKib: number; iterations: number; parallelism: number };
}

/** What opens an encrypted database: a key of 32 bytes, or a password. */
interface Secret {
  key?: Uint8Array;
  password?: string | Uint8Array;
}

/** How a write commits, as `write` and `writeAsync` take it. */
interface WriteOptions {
  durability?: 'sync' | 'deferred';
}

/** Kept from users, so that a `Database` comes only from `Database.open`. */
const CREATE = Symbol('create');

/** Where a collection keeps its transaction and its layout, out of sight. */
const TXN = Symbol('txn');
const LAYOUT = Symbol('layout');

/**
 * A transaction as the native functions that read and write objects take
 * it: its handle, which each transaction object gets once, rather than the
 * object, whose methods cost more to call (`NativeTransaction.handle`).
 */
type Handle = native.ExternalObject<'NativeTransaction'>;

/** A collection of a transaction, for reading its objects. */
class ReadCollection {
  #txn: Handle;
  #layout: CollectionLayout;

  constructor(txn: Handle, layout: CollectionLayout) {
    this.#txn = txn;
    this.#layout = layout;
  }

  /** The collection's name. */
  get name(): string {
    return this.#layout.name;
  }

  /**
   * The object whose primary key is `key`, or `null`. What the native layer
   * delivers is read where it lies, `scratch` up to the length it gives, with
   * no view made of it for every read.
   */
  get(key: unknown): Record<string, unknown> | null {
    const record = native.getRecord(this.#txn, nameOf(this.#layout), keyOf(key), scratch);

    if (record === null) {
      return null;
    }

    return typeof record === 'number'
      ? decodeRecord(this.#layout, scratch, false, record)
      : decodeRecord(this.#layout, record);
  }

  /** The objects a query finds, in its order; every object without one. */
  find(query: QueryInput, parameters?: unknown): Record<string, unknown>[] {
    const records = this.#find(query, parameters, false);

    return typeof records === 'number'
      ? decodeRecords(this.#layout, scratch, records)
      : decodeRecords(this.#layout, records);
  }

  /** The first object a query finds, or `null`. The engine stops reading there. */
  findOne(query: QueryInput, parameters?: unknown): Record<string, unknown> | null {
    const records = this.#find(query, parameters, true);

    return typeof records === 'number'
      ? decodeFirst(this.#layout, scratch, records)
      : decodeFirst(this.#layout, records);
  }

  /** How many objects a query finds, after its offset and within its limit. */
  count(query: QueryInput, parameters?: unknown): number {
    const name = this.#layout.name;
    const prepared = preparedOf(name, query);

    if (prepared !== null) {
      const length = lendParameters(parameters);

      return native.countPrepared(this.#txn, prepared.handle, parameterBytes(), length);
    }

    return native.count(this.#txn, irOf(name, query, parameters, true, false, true));
  }

  /**
   * The records a query finds, only the first with `first`: their length in
   * `scratch`, or a `Buffer` of their own.
   */
  #find(query: QueryInput, parameters: unknown, first: boolean): number | Buffer {
    const name = this.#layout.name;
    const prepared = preparedOf(name, query);

    if (prepared !== null) {
      const length = lendParameters(parameters);

      return native.findPrepared(
        this.#txn,
        prepared.handle,
        parameterBytes(),
        length,
        first,
        scratch
      );
    }

    // The IR is lent: the engine reads it before the call returns.
    return native.find(
      this.#txn,
      irOf(name, query, parameters, false, first, true),
      first,
      scratch
    );
  }

  get [TXN](): Handle {
    return this.#txn;
  }

  get [LAYOUT](): CollectionLayout {
    return this.#layout;
  }
}

/** A collection of a write transaction, for reading and writing its objects. */
class WriteCollection extends ReadCollection {
  /** Inserts `object` and returns its primary key. */
  insert(object: unknown) {
    return this.#writeOne(object, false);
  }

  /**
   * Inserts `objects`, in one call into the engine, and returns their keys.
   * A refused object stops the batch with its error, and the objects before
   * it stay inserted in the transaction.
   */
  insertMany(objects: unknown) {
    return this.#write(objects, false);
  }

  /** Inserts `object`, or replaces the object with its key. */
  put(object: unknown) {
    return this.#writeOne(object, true);
  }

  /** Inserts or replaces `objects`; see `insertMany`. */
  putMany(objects: unknown) {
    return this.#write(objects, true);
  }

  /**
   * Sets the fields `changes` has in the object whose primary key is `key`,
   * and says whether there was one. The rest of the object stays as it is,
   * and the engine changes it where it lies.
   */
  update(key: unknown, changes: unknown): boolean {
    const layout = this[LAYOUT];

    const length = lendChanges(layout, changes);

    return native.updateRecord(this[TXN], nameOf(layout), keyOf(key), recordBytes(), length);
  }

  /** Deletes the object whose primary key is `key`, and says whether there was one. */
  delete(key: unknown): boolean {
    return native.deleteObject(this[TXN], nameOf(this[LAYOUT]), keyOf(key));
  }

  #writeOne(object: unknown, replace: boolean) {
    const length = lendRecords(this[LAYOUT], [object]);

    return native.writeRecord(this[TXN], nameOf(this[LAYOUT]), recordBytes(), length, replace);
  }

  #write(objects: unknown, replace: boolean) {
    if (!Array.isArray(objects)) {
      throw invalid('a batch of objects is an array');
    }

    if (objects.length === 0) {
      return [];
    }

    const length = lendRecords(this[LAYOUT], objects);

    return native.writeRecords(this[TXN], nameOf(this[LAYOUT]), recordBytes(), length, replace);
  }
}

/** A read transaction: one commit, for as long as its function runs. */
class ReadTransaction {
  #txn: Handle;
  #layout: SchemaLayout | null;

  constructor(txn: Handle, layout: SchemaLayout | null) {
    this.#txn = txn;
    this.#layout = layout;
  }

  /** Collection `name` of the schema, for reading. */
  collection(name: string): ReadCollection {
    return new ReadCollection(this.#txn, collectionOf(this.#layout, name));
  }
}

/** A write transaction: changes that commit together when its function returns. */
class WriteTransaction {
  #txn: Handle;
  #layout: SchemaLayout | null;

  constructor(txn: Handle, layout: SchemaLayout | null) {
    this.#txn = txn;
    this.#layout = layout;
  }

  /** Collection `name` of the schema, for reading and writing. */
  collection(name: string): WriteCollection {
    return new WriteCollection(this.#txn, collectionOf(this.#layout, name));
  }
}

/**
 * The write transaction of a migration, as a migration function gets it: the
 * collections of the new schema, and the objects as the schema before read
 * them.
 */
class Migrating {
  #txn: native.NativeTransaction;
  #handle: Handle;
  #layout: SchemaLayout;
  #previous: SchemaLayout;
  #version: number;

  constructor(
    txn: native.NativeTransaction,
    layout: SchemaLayout,
    previous: SchemaLayout,
    version: number
  ) {
    this.#txn = txn;
    this.#handle = txn.handle;
    this.#layout = layout;
    this.#previous = previous;
    this.#version = version;
  }

  /** The schema version the file held before the migration. */
  get previousVersion(): number {
    return this.#txn.previousVersion;
  }

  /** The version this step migrates to. */
  get version(): number {
    return this.#version;
  }

  /** Collection `name` of the new schema, for reading and writing. */
  collection(name: string): WriteCollection {
    return new WriteCollection(this.#handle, collectionOf(this.#layout, name));
  }

  /**
   * The object of `collection` whose key is `key`, as the schema before the
   * migration reads it, with the names it gave and the values of fields the
   * migration removed or replaced. Read an object this way before writing it:
   * a written object keeps only the new schema's fields.
   */
  previous(collection: string, key: unknown): Record<string, unknown> | null {
    const layout = collectionOf(this.#previous, collection);
    const record = this.#txn.previousRecord(collection, keyOf(key));

    return record === null ? null : decodeRecord(layout, record, true);
  }

  /** The keys of every object of `collection`, named as before the migration. */
  previousKeys(collection: string) {
    collectionOf(this.#previous, collection);

    return this.#txn.previousKeys(collection);
  }
}

/** What `open` and `openAsync` pass to the native layer, checked. */
function nativeOptions(options: OpenOptions<never>): native.NativeOptions {
  const {
    create,
    pageSize,
    busyTimeout,
    cacheSize,
    schema,
    migrations = [],
    passwordHashing
  } = options;

  if (!Array.isArray(migrations)) {
    throw invalid('`migrations` is an array');
  }

  if (cacheSize !== undefined && (!Number.isSafeInteger(cacheSize) || cacheSize < 0)) {
    throw invalid('`cacheSize` is a whole number of bytes from 0 up');
  }

  checkPasswordHashing(passwordHashing);

  const passed = {
    create,
    pageSize,
    busyTimeout,
    cacheSize,
    schema: schema === undefined ? undefined : toBuffer(encodeSchema(schema)),
    migrations: migrations.map(nativeMigration),
    passwordHashing
  };

  // Last, so that nothing above throws with the copies not yet wiped.
  return { ...passed, ...secretOf(options) };
}

/** Refuses a `passwordHashing` option that is not three whole numbers. */
function checkPasswordHashing(passwordHashing: OpenOptions<never>['passwordHashing']): void {
  if (passwordHashing !== undefined) {
    const { memoryKib, iterations, parallelism } = passwordHashing ?? {};

    if (![memoryKib, iterations, parallelism].every(isU32)) {
      throw invalid(
        '`passwordHashing` has `memoryKib`, `iterations` and `parallelism`, each a whole number from 0 up'
      );
    }
  }
}

/** Whether `value` fits an unsigned 32-bit integer. */
function isU32(value: unknown): boolean {
  return Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= 0xffffffff;
}

/** The migration functions of `options`, by the version they migrate to. */
function runsOf<M>(options: OpenOptions<M>): Map<number, Migration<M>['run']> {
  return new Map(
    (options.migrations ?? [])
      .filter((migration) => migration.run !== undefined)
      .map((migration) => [migration.version, migration.run])
  );
}

/**
 * The file at `path`, as the engine tells one open file from another: by
 * device and inode, which on Windows Node.js fills with the volume serial
 * number and the file index, the two the engine reads there. Two handles to
 * one file get the same key however each named it, through a link or another
 * spelling, so their writes queue together. `bigint`, because a file index
 * does not fit in a number.
 */
function fileKeyOf(path: string): string {
  try {
    const { dev, ino } = statSync(path, { bigint: true });

    return `${dev}:${ino}`;
  } catch {
    return resolve(path);
  }
}

/** An open database. Created with `Database.open` or `Database.openAsync`. */
class Database {
  #native: native.NativeDatabase | null;
  #path: string;
  #layout: SchemaLayout | null;
  #file: string;

  constructor(
    token: symbol,
    database: native.NativeDatabase,
    path: string,
    layout: SchemaLayout | null
  ) {
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
  static open(path: string, options: OpenOptions<Migrating> = {}): Database {
    const passed = nativeOptions(options);
    let opening: native.NativeOpening;

    try {
      opening = native.NativeOpening.open(path, passed);
    } finally {
      wipe(passed);
    }
    let database: native.NativeDatabase;

    if (opening.isMigrating) {
      const txn = opening.migration();
      // The migration holds the writer lock, so a write on the file from
      // its functions would wait for it.
      const release = hold(fileKeyOf(path));

      try {
        const layout = layoutOf(txn.schemaRecord, options.schema);
        const previous = looseLayoutOf(txn.previousSchemaRecord);
        const runs = runsOf(options);
        let version: number | null;

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
  static async openAsync(
    path: string,
    options: OpenOptions<AsyncMigrating> = {}
  ): Promise<Database> {
    const passed = nativeOptions(options);
    let task: ReturnType<typeof native.NativeOpening.openAsync>;

    // The engine copies the secret when the call is made, before the pool
    // runs it.
    try {
      task = native.NativeOpening.openAsync(path, passed);
    } finally {
      wipe(passed);
    }

    const opening = settle(await task);
    let database: native.NativeDatabase;

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
        let version: number | null;

        // Every operation has settled whenever `nextStep` runs, so it never
        // waits on the event loop for the transaction.
        while ((version = txn.nextStep()) !== null) {
          const run = runs.get(version);

          if (run !== undefined) {
            // `inside` calls the function at once, while `version` is still
            // this step's, and not null.
            await inside(file, () =>
              run(new AsyncMigrating(serial, layout, previous, previousVersion, version!))
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

  static #of(
    database: native.NativeDatabase,
    path: string,
    schema: DeclaredSchema | undefined
  ): Database {
    const record = database.schemaRecord;

    return new Database(CREATE, database, path, record === null ? null : layoutOf(record, schema));
  }

  /** The path the database was opened at. Still readable after `close`. */
  get path(): string {
    return this.#path;
  }

  /** Whether `close` has not been called. */
  get isOpen(): boolean {
    return this.#native !== null;
  }

  /** The size of every page in the file, in bytes. */
  get pageSize(): number {
    return this.#database().pageSize;
  }

  /** The file format version recorded in the file. */
  get formatVersion(): number {
    return this.#database().formatVersion;
  }

  /** Whether the file is encrypted. */
  get isEncrypted(): boolean {
    return this.#database().isEncrypted;
  }

  /**
   * Changes the key of an encrypted database to `key`, 32 bytes. It commits,
   * so it is refused while an asynchronous write of this process holds the
   * file, as a synchronous write is. When it returns, the old key or
   * password no longer opens the file.
   */
  setKey(key: Uint8Array): void {
    const database = this.#database();
    const copy = copyOfKey(key);

    try {
      holdForSync(this.#file, '`setKey`', () => database.setKey(copy));
    } finally {
      copy.fill(0);
    }
  }

  /** `setKey` on the thread pool, after this process's writes on the file. */
  async setKeyAsync(key: Uint8Array): Promise<void> {
    const database = this.#database();
    const copy = copyOfKey(key);
    const release = await turn(this.#file);

    try {
      const task = database.setKeyAsync(copy);

      copy.fill(0);
      settle(await task);
    } finally {
      copy.fill(0);
      release();
    }
  }

  /**
   * Changes the key of an encrypted database to one derived from `password`,
   * at the hashing cost the database was opened with; see `setKey`.
   */
  setPassword(password: string | Uint8Array): void {
    const database = this.#database();
    const copy = copyOfPassword(password);

    try {
      holdForSync(this.#file, '`setPassword`', () => database.setPassword(copy));
    } finally {
      copy.fill(0);
    }
  }

  /** `setPassword` on the thread pool, after this process's writes on the file. */
  async setPasswordAsync(password: string | Uint8Array): Promise<void> {
    const database = this.#database();
    const copy = copyOfPassword(password);
    const release = await turn(this.#file);

    try {
      const task = database.setPasswordAsync(copy);

      copy.fill(0);
      settle(await task);
    } finally {
      copy.fill(0);
      release();
    }
  }

  /**
   * The schema version the file holds, or `null` without a schema. The
   * layout is the schema this handle declared, whose version `schema` only
   * takes as a safe integer, so it is a number even where the record's
   * decoder would allow a `bigint`.
   */
  get schemaVersion(): number | null {
    this.#database();

    return this.#layout === null ? null : Number(this.#layout.version);
  }

  /**
   * Prepares `query` on collection `collection` of the schema: text in the
   * query language, or a query built with `param` in place of its values.
   * It is parsed once here, and each run gives values for its parameters.
   */
  prepare(collection: string, query: QueryInput): Prepared {
    this.#database();
    collectionOf(this.#layout, collection);

    return prepare(collection, query);
  }

  /**
   * Runs `fn` in a read transaction, which sees one commit for as long as
   * `fn` runs, and returns what `fn` returns.
   */
  read<R>(fn: (txn: ReadTransaction) => R): R {
    const txn = this.#database().beginReadHandle();

    try {
      const result = fn(new ReadTransaction(txn, this.#layout));

      synchronous(result);

      return result;
    } finally {
      native.endTransaction(txn);
    }
  }

  /**
   * Runs `fn` in a write transaction, and commits it when `fn` returns, or
   * aborts it when `fn` throws. With `durability: 'deferred'`, the commit
   * returns without waiting for the disk.
   */
  write<R>(fn: (txn: WriteTransaction) => R, options: WriteOptions = {}): R {
    const deferred = deferredOf(options);
    const database = this.#database();

    return holdForSync(this.#file, 'a synchronous write transaction', () => {
      const txn = database.beginWriteHandle();
      let committed = false;

      try {
        const result = fn(new WriteTransaction(txn, this.#layout));

        synchronous(result);
        native.commitTransaction(txn, deferred);
        committed = true;

        return result;
      } finally {
        if (!committed) {
          native.endTransaction(txn);
        }
      }
    });
  }

  /**
   * Runs `fn`, which may be asynchronous, in a read transaction whose
   * operations run on the thread pool, and resolves to what `fn` resolves
   * to. The transaction sees one commit until `fn` settles.
   */
  async readAsync<R>(fn: (txn: AsyncReadTransaction) => R): Promise<Awaited<R>> {
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
  async writeAsync<R>(
    fn: (txn: AsyncWriteTransaction) => R,
    options: WriteOptions = {}
  ): Promise<Awaited<R>> {
    const deferred = deferredOf(options);
    const database = this.#database();
    const release = await turn(this.#file);

    try {
      const txn = settle(await database.beginWriteAsync());
      const serial = new Serial(txn);
      let result: Awaited<R>;

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
  sync(): void {
    const database = this.#database();

    holdForSync(this.#file, '`sync`', () => database.sync());
  }

  /** `sync` on the thread pool, after this process's writes on the file. */
  async syncAsync(): Promise<void> {
    const database = this.#database();
    const release = await turn(this.#file);

    try {
      settle(await database.syncAsync());
    } finally {
      release();
    }
  }

  /**
   * Rescues what it can of the damaged database at `from` into a new
   * database at `into`, and reports what it rescued and what it could not.
   * It needs the file alone: a file open in this process or another fails
   * with `BUSY`. It never replaces a file already at `into`.
   */
  static salvage(from: string, into: string, options: SalvageOptions = {}): SalvageReport {
    const passed = salvageOptionsOf(options);

    try {
      return salvageReportOf(native.salvage(pathOf(from), pathOf(into), passed));
    } finally {
      wipe(passed);
    }
  }

  /** `salvage` on the thread pool. */
  static async salvageAsync(
    from: string,
    into: string,
    options: SalvageOptions = {}
  ): Promise<SalvageReport> {
    const passed = salvageOptionsOf(options);
    let task: ReturnType<typeof native.salvageAsync>;

    try {
      task = native.salvageAsync(pathOf(from), pathOf(into), passed);
    } finally {
      wipe(passed);
    }

    return salvageReportOf(settle(await task));
  }

  /**
   * Checks the published commit completely, and reports every problem it
   * finds. It reads while other handles and processes write.
   */
  check(): CheckReport {
    return reportOf(this.#database().check());
  }

  /** `check` on the thread pool, which never waits for a writer. */
  async checkAsync(): Promise<CheckReport> {
    return reportOf(settle(await this.#database().checkAsync()));
  }

  /**
   * Writes a copy of the published commit to a new file at `path`, which
   * the same key or password opens, or under a new data key with a `key`
   * or `password` in `options`, while other handles and processes may
   * write. It never replaces a file already at `path`.
   */
  backup(path: string, options: BackupOptions = {}): BackupReport {
    const passed = backupOptionsOf(options);

    try {
      return this.#database().backup(pathOf(path), passed);
    } finally {
      wipe(passed);
    }
  }

  /** `backup` on the thread pool. */
  async backupAsync(path: string, options: BackupOptions = {}): Promise<BackupReport> {
    const passed = backupOptionsOf(options);
    let task: ReturnType<native.NativeDatabase['backupAsync']>;

    try {
      task = this.#database().backupAsync(pathOf(path), passed);
    } finally {
      wipe(passed);
    }

    return settle(await task);
  }

  /**
   * Makes the file smaller in place, in write transactions of its own, so
   * it is refused while an asynchronous write of this process holds the
   * file, as a synchronous write is.
   */
  compact(): CompactReport {
    const database = this.#database();

    return holdForSync(this.#file, '`compact`', () => database.compact());
  }

  /** `compact` on the thread pool, after this process's writes on the file. */
  async compactAsync(): Promise<CompactReport> {
    const database = this.#database();
    const release = await turn(this.#file);

    try {
      return settle(await database.compactAsync());
    } finally {
      release();
    }
  }

  /**
   * Makes deferred commits durable and closes the database. Closing one that
   * is closed does nothing.
   */
  close(): void {
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
  async closeAsync(): Promise<void> {
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

  #database(): native.NativeDatabase {
    if (this.#native === null) {
      throw codeError('CLOSED', 'the database has been closed');
    }

    return this.#native;
  }
}

/** What a compaction did, as `compact` and `compactAsync` give it. */
interface CompactReport {
  bytesBefore: number;
  bytesAfter: number;
  pagesMoved: number;
}

/** What a backup wrote, as `backup` and `backupAsync` give it. */
interface BackupReport {
  commitId: number;
  trees: number;
  entries: number;
  bytes: number;
}

/** The options `backup` and `backupAsync` take. */
interface BackupOptions extends Secret {
  passwordHashing?: OpenOptions<never>['passwordHashing'];
}

/** What `backup` and `backupAsync` pass to the native layer, checked. */
function backupOptionsOf(options: BackupOptions): native.NativeBackupOptions {
  checkPasswordHashing(options.passwordHashing);

  // Last, so that nothing above throws with the copies not yet wiped.
  return { passwordHashing: options.passwordHashing, ...secretOf(options) };
}

/** The options `salvage` and `salvageAsync` take. */
interface SalvageOptions extends Secret {
  busyTimeout?: number;
}

/** What `salvage` and `salvageAsync` pass to the native layer, checked. */
function salvageOptionsOf(options: SalvageOptions): native.NativeSalvageOptions {
  return { busyTimeout: options.busyTimeout, ...secretOf(options) };
}

/**
 * The key or password of `options`, each copied into a buffer of the
 * package's own, which `wipe` fills with zeros once the engine has taken its
 * copy. The caller's own buffer is left as it is, for the caller to wipe.
 */
function secretOf(options: Secret): { key?: Buffer; password?: Buffer } {
  const { key, password } = options;

  if (key !== undefined && password !== undefined) {
    throw invalid('give a `key` or a `password`, not both');
  }

  return {
    key: key === undefined ? undefined : copyOfKey(key),
    password: password === undefined ? undefined : copyOfPassword(password)
  };
}

/** A copy of `key`, which has to be 32 bytes. */
function copyOfKey(key: unknown): Buffer {
  if (!(key instanceof Uint8Array) || key.length !== 32) {
    throw invalid('a `key` is a Uint8Array of 32 bytes');
  }

  return Buffer.from(key);
}

/** A password's bytes, UTF-8 for a string, in a buffer of the package's own. */
function copyOfPassword(password: unknown): Buffer {
  if (typeof password === 'string') {
    return Buffer.from(password, 'utf8');
  }

  if (password instanceof Uint8Array) {
    return Buffer.from(password);
  }

  throw invalid('a `password` is a string or a Uint8Array');
}

/** Fills the copies `secretOf` made with zeros. */
function wipe(passed: { key?: Buffer; password?: Buffer }): void {
  passed.key?.fill(0);
  passed.password?.fill(0);
}

/** What a salvage rescued, as `salvage` and `salvageAsync` give it. */
interface SalvageReport {
  whole: boolean;
  commitId: number | null;
  pagesScanned: number;
  pagesDamaged: number;
  pagesUnread: number;
  entriesRecovered: number;
  valuesLost: number;
  objectsDropped: number;
  trees: number;
  entries: number;
  bytes: number;
}

/** The report the native layer gives, with `null` for a commit it has none of. */
function salvageReportOf(report: native.NativeSalvageReport): SalvageReport {
  return {
    whole: report.whole,
    commitId: report.commitId ?? null,
    pagesScanned: report.pagesScanned,
    pagesDamaged: report.pagesDamaged,
    pagesUnread: report.pagesUnread,
    entriesRecovered: report.entriesRecovered,
    valuesLost: report.valuesLost,
    objectsDropped: report.objectsDropped,
    trees: report.trees,
    entries: report.entries,
    bytes: report.bytes
  };
}

/** A path a tool reads or writes, which has to be a string. */
function pathOf(path: unknown): string {
  if (typeof path !== 'string' || path.length === 0) {
    throw invalid('a path is a string that is not empty');
  }

  return path;
}

/** What the integrity check found, as `check` and `checkAsync` give it. */
interface CheckReport {
  ok: boolean;
  commitId: number;
  pageCount: number;
  pagesChecked: number;
  objectsChecked: number;
  problems: { page: number | null; tree: string | null; message: string }[];
}

/** The report the native layer gives, with `ok` and nulls where it has nothing. */
function reportOf(report: native.NativeCheckReport): CheckReport {
  return {
    ok: report.problems.length === 0,
    commitId: report.commitId,
    pageCount: report.pageCount,
    pagesChecked: report.pagesChecked,
    objectsChecked: report.objectsChecked,
    problems: report.problems.map((problem) => ({
      page: problem.page ?? null,
      tree: problem.tree ?? null,
      message: problem.message
    }))
  };
}

/** Whether a write's options ask for a deferred commit. */
function deferredOf(options: WriteOptions): boolean {
  const durability = options.durability ?? 'sync';

  if (durability !== 'sync' && durability !== 'deferred') {
    throw invalid("`durability` is `'sync'` or `'deferred'`");
  }

  return durability === 'deferred';
}

export { Database };
// For `index.ts` to check the declared API against, and nothing at run time.
export type { ReadCollection, WriteCollection, ReadTransaction, WriteTransaction, Migrating };
