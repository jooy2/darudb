/**
 * DaruDB for Node.js: an embedded database that keeps an application's data
 * in one local file.
 *
 * A schema declared with `t`, `collection` and `schema` types every object
 * the database reads and writes: `Database.open` with a schema gives a
 * `Database` whose collections know their objects' fields. `types.ts` holds
 * the types, and this file gives them to what the other modules make.
 *
 * The types say what an object holds by its schema, which the code cannot
 * know: the code takes and gives plain values and checks them as it goes.
 * So each export is cast to its type here, once, and the checks at the end
 * of the file make sure the code has every member the types promise.
 */

// Loaded with `require` rather than named imports: `tsc` makes a name
// imported and exported again a getter on `exports`, and these stay values.
import native = require('../native.js');
import schemaModule = require('./schema.js');
import queryModule = require('./query.js');
import databaseModule = require('./database.js');
import type * as async from './async.js';
import type * as sync from './database.js';
import type * as query from './query.js';
import type * as api from './types.js';

export type {
  AnyField,
  AsyncMigrating,
  AsyncMigration,
  AsyncOpenOptions,
  AsyncReadCollection,
  AsyncReadTransaction,
  AsyncWriteCollection,
  AsyncWriteTransaction,
  Collection,
  Comparison,
  Condition,
  Conditions,
  DatabaseOpener,
  EmbeddedInputOf,
  EmbeddedOf,
  EmbeddedType,
  FieldMode,
  FieldType,
  Fields,
  InsertOf,
  Key,
  KeyType,
  KeyableType,
  LinkType,
  Migrating,
  Migration,
  ObjectOf,
  OpenOptions,
  Param,
  Prepared,
  QueryConstructor,
  QueryInput,
  QueryParameters,
  QueryValue,
  ReadCollection,
  ReadTransaction,
  Schema,
  Typed,
  TypeBuilders,
  WriteCollection,
  WriteOptions,
  WriteTransaction
} from './types.js';

/** The file format version this build of the engine reads and writes. */
export const FORMAT_VERSION: number = native.FORMAT_VERSION;

/** The version of the DaruDB engine inside this package. */
export const engineVersion: () => string = native.engineVersion;

/** The types of fields. */
export const t = schemaModule.t as unknown as api.TypeBuilders;

/**
 * A collection with `fields`. A field marked `primaryKey()` is the key;
 * without one, the collection gets an `id` that the engine numbers from 1.
 */
export const collection = schemaModule.collection as unknown as <F extends api.Fields>(
  fields: F
) => api.Collection<F>;

/**
 * A schema: `collections` by name, at `version`, from 1 up. Raise the
 * version whenever the schema changes.
 */
export const schema = schemaModule.schema as unknown as <
  C extends Record<string, api.Collection<any>>
>(
  version: number,
  collections: C
) => api.Schema<C>;

/**
 * What to find, in what order, and how many. Without a sort, objects come in
 * primary key order, and objects that sort equal come in primary key order
 * too. Each method adds to the query and returns it.
 */
export type Query<O = Record<string, unknown>> = api.Query<O>;
/**
 * What to find, in what order, and how many. Without a sort, objects come in
 * primary key order, and objects that sort equal come in primary key order
 * too. Each method adds to the query and returns it.
 */
export const Query = queryModule.Query as unknown as api.QueryConstructor;

/** The conditions a filter is made of, for building one outside a query. */
export const conditions = queryModule.conditions as unknown as api.Conditions;

/** Parameter `index` of a prepared query, counted from 0. */
export const param: (index: number) => api.Param = queryModule.param;

/**
 * An open database. There is no constructor: use `Database.open` or
 * `Database.openAsync`.
 *
 * Every method has an asynchronous twin whose name ends in `Async`, which
 * does the engine's work on the thread pool and resolves a promise, so the
 * event loop never waits for the disk or for another process's writer.
 */
export type Database<S extends api.Schema<any> = api.Schema> = api.Database<S>;
/**
 * An open database. There is no constructor: use `Database.open` or
 * `Database.openAsync`.
 *
 * Every method has an asynchronous twin whose name ends in `Async`, which
 * does the engine's work on the thread pool and resolves a promise, so the
 * event loop never waits for the disk or for another process's writer.
 */
export const Database = databaseModule.Database as unknown as api.DatabaseOpener;

/**
 * `true` when `Code` has every member `Api` declares, but the markers a
 * field type carries only for the type checker; otherwise the names of
 * those it lacks, which fail the check below with the names in the error.
 */
type Covers<Code, Api> = [Lacks<Code, Api>] extends [never] ? true : { lacks: Lacks<Code, Api> };

type Lacks<Code, Api> = Exclude<keyof Api, keyof Code | keyof api.Typed<any, any, any, any>>;

type Check<Covered extends true> = Covered;

// Every member the types declare is there in the code the casts above cover.
// eslint-disable-next-line @typescript-eslint/no-unused-vars -- only compiled
type Checked = [
  Check<Covers<typeof schemaModule.t, api.TypeBuilders>>,
  Check<Covers<ReturnType<typeof schemaModule.t.int>, api.KeyableType<number>>>,
  Check<Covers<ReturnType<typeof schemaModule.t.link>, api.LinkType>>,
  Check<Covers<query.Query, api.Query>>,
  Check<Covers<typeof queryModule.conditions, api.Conditions>>,
  Check<Covers<typeof databaseModule.Database, api.DatabaseOpener>>,
  Check<Covers<sync.Database, api.Database>>,
  Check<Covers<sync.ReadTransaction, api.ReadTransaction<api.Schema>>>,
  Check<Covers<sync.WriteTransaction, api.WriteTransaction<api.Schema>>>,
  Check<Covers<sync.Migrating, api.Migrating<api.Schema>>>,
  Check<Covers<sync.ReadCollection, api.ReadCollection<unknown>>>,
  Check<Covers<sync.WriteCollection, api.WriteCollection<unknown, unknown>>>,
  Check<Covers<async.AsyncReadTransaction, api.AsyncReadTransaction<api.Schema>>>,
  Check<Covers<async.AsyncWriteTransaction, api.AsyncWriteTransaction<api.Schema>>>,
  Check<Covers<async.AsyncMigrating, api.AsyncMigrating<api.Schema>>>,
  Check<Covers<async.AsyncReadCollection, api.AsyncReadCollection<unknown>>>,
  Check<Covers<async.AsyncWriteCollection, api.AsyncWriteCollection<unknown, unknown>>>
];
