/**
 * The types of the package's API, which `index.ts` gives its exports: a
 * schema declared with `t`, `collection` and `schema` types every object
 * the database reads and writes, and `Database.open` with a schema gives a
 * `Database` whose collections know their objects' fields.
 *
 * The types carry what the type checker needs and nothing the code reads:
 * a field type's value type, for one, is a property that never exists.
 */

/**
 * A primary key: an int, as a number or a `bigint` beyond 2^53, a string, or
 * bytes.
 */
export type Key = number | bigint | string | Uint8Array;

/** How a field holds its value: required, optional, or required with a default. */
export type FieldMode = 'required' | 'optional' | 'default';

/**
 * What every field type carries for the type checker, and never at run
 * time: `T` is the value an object read from the database holds, `M` how
 * the field holds it, `K` whether it is the primary key, and `I` the value
 * written.
 */
export interface Typed<T, M extends FieldMode, K extends boolean, I> {
  /** @internal */
  readonly __value?: T;
  /** @internal */
  readonly __mode?: M;
  /** @internal */
  readonly __key?: K;
  /** @internal */
  readonly __input?: I;
}

/** A field's type, with its modifiers. Each returns a copy. */
export interface FieldType<T, M extends FieldMode = 'required', I = T> extends Typed<
  T,
  M,
  false,
  I
> {
  /** The field may be null, and is null when left out. */
  optional(): FieldType<T, 'optional', I>;
  /** The field is required, and holds `value` when left out. */
  default(value: I): FieldType<T, 'default', I>;
  /** Queries on the field read an index rather than every object. */
  index(): FieldType<T, M, I>;
  /** An index that also refuses two objects with the same value. Any number may hold null. */
  unique(): FieldType<T, M, I>;
}

/** The type of a field that can be the primary key: an int, a string or bytes. */
export interface KeyableType<T, I = T> extends FieldType<T, 'required', I> {
  /** The field is the collection's primary key. */
  primaryKey(): KeyType<T, I>;
}

/** The primary key's field: required, without a default. */
export interface KeyType<T, I = T> extends Typed<T, 'required', true, I> {
  index(): KeyType<T, I>;
  unique(): KeyType<T, I>;
}

/** A link, which has no default: it would name an object. */
export interface LinkType<M extends FieldMode = 'required'> extends Typed<Key, M, false, Key> {
  optional(): LinkType<'optional'>;
  index(): LinkType<M>;
  unique(): LinkType<M>;
}

/** An embedded object, whose fields have their own defaults, and no index. */
export interface EmbeddedType<T, I, M extends FieldMode = 'required'> extends Typed<
  T,
  M,
  false,
  I
> {
  optional(): EmbeddedType<T, I, 'optional'>;
}

/** Any field's type. */
export type AnyField = Typed<any, FieldMode, boolean, any>;

/** The fields of a collection or an embedded object, by name. */
export type Fields = Record<string, AnyField>;

type Simplify<T> = { [K in keyof T]: T[K] } & {};

/** The value an object read from the database holds in a field. */
type ValueOf<F> =
  F extends Typed<infer T, infer M, any, any> ? (M extends 'optional' ? T | null : T) : never;

/** The value written to a field. */
type InputOf<F> = F extends Typed<any, any, any, infer I> ? I : never;

type RequiredKeys<F extends Fields> = {
  [K in keyof F]: F[K] extends Typed<any, 'required', any, any> ? K : never;
}[keyof F];

type HasKey<F extends Fields> = true extends {
  [K in keyof F]: F[K] extends Typed<any, any, true, any> ? true : false;
}[keyof F]
  ? true
  : false;

/** The fields of an embedded object as it is read: every one, null where optional and empty. */
export type EmbeddedOf<F extends Fields> = Simplify<{ [K in keyof F]: ValueOf<F[K]> }>;

/** The fields of an embedded object as it is written. */
export type EmbeddedInputOf<F extends Fields> = Simplify<
  { [K in RequiredKeys<F>]: InputOf<F[K]> } & {
    [K in Exclude<keyof F, RequiredKeys<F>>]?: InputOf<F[K]> | null;
  }
>;

/** An object as the database holds it, with the `id` of a collection without a key field. */
export type ObjectOf<F extends Fields> = Simplify<
  EmbeddedOf<F> & (HasKey<F> extends true ? unknown : { id: number })
>;

/** An object as it is written: required fields without a default, and any of the rest. */
export type InsertOf<F extends Fields> = Simplify<
  EmbeddedInputOf<F> & (HasKey<F> extends true ? unknown : { id?: number })
>;

/** The types of fields, as `t` makes them. */
export interface TypeBuilders {
  bool(): FieldType<boolean>;
  /**
   * A 64-bit int read as a number. A value beyond 2^53, which a number does
   * not hold exactly, is refused when written and fails when read: declare
   * such a field with `bigint`.
   */
  int(): KeyableType<number>;
  /** A 64-bit int read as a `bigint`, whatever its size. */
  bigint(): KeyableType<bigint, bigint | number>;
  float(): FieldType<number>;
  string(): KeyableType<string>;
  bytes(): KeyableType<Uint8Array>;
  /** The primary key of an object of collection `collection`. */
  link(collection: string): LinkType;
  /** A list of values of `element`, a type without modifiers. */
  list<T, I>(
    element: FieldType<T, 'required', I> | KeyableType<T, I>
  ): FieldType<T[], 'required', I[]>;
  /** A list of links: a to-many link. */
  list(element: LinkType): FieldType<Key[]>;
  /** An embedded object with fields of its own. */
  object<F extends Fields>(fields: F): EmbeddedType<EmbeddedOf<F>, EmbeddedInputOf<F>>;
}

/** A collection: its fields. */
export interface Collection<F extends Fields = Fields> {
  readonly fields: F;
}

/** The collections of a database, at a version. */
export interface Schema<C extends Record<string, Collection<any>> = Record<string, Collection>> {
  readonly version: number;
  readonly collections: C;
}

type CollectionsOf<S> = S extends Schema<infer C> ? C : Record<string, Collection>;
type NameOf<S> = keyof CollectionsOf<S> & string;
type FieldsOf<S, N extends NameOf<S>> =
  CollectionsOf<S>[N] extends Collection<infer F> ? F : Fields;

/** A value a query compares with. */
export type QueryValue = boolean | number | bigint | string | Uint8Array;

/**
 * A parameter in place of a value, in a query that `Database.prepare`
 * prepares: each run gives its value. `param(0)` is the first.
 */
export interface Param {
  readonly index: number;
}

/** A value of a condition, or a parameter in its place. */
type Operand<T> = T | Param;

/** The value a condition on a field of type `T` compares with: an element for a list. */
type ElementOf<T> = T extends readonly (infer E)[] ? E : Exclude<T, null>;

/** The fields of `O` a condition names directly. */
type FieldNames<O> = keyof O & string;

/** A path through an embedded object or a link, which the engine checks. */
type DottedPath = `${string}.${string}`;

/** A comparison operator of `Query.where`. */
export type Comparison = '==' | '!=' | '<' | '<=' | '>' | '>=';

/** A condition of a filter, made by `conditions` or `Query.where`'s function. */
export interface Condition {
  /** @internal */
  readonly node: unknown;
}

/** The conditions a filter is made of. */
export interface Conditions<O = Record<string, unknown>> {
  eq<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]> | null>): Condition;
  eq(path: DottedPath, value: Operand<QueryValue | null>): Condition;
  ne<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]> | null>): Condition;
  ne(path: DottedPath, value: Operand<QueryValue | null>): Condition;
  lt<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
  lt(path: DottedPath, value: Operand<QueryValue>): Condition;
  le<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
  le(path: DottedPath, value: Operand<QueryValue>): Condition;
  gt<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
  gt(path: DottedPath, value: Operand<QueryValue>): Condition;
  ge<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
  ge(path: DottedPath, value: Operand<QueryValue>): Condition;
  between<K extends FieldNames<O>>(
    field: K,
    low: Operand<ElementOf<O[K]>>,
    high: Operand<ElementOf<O[K]>>
  ): Condition;
  between(path: DottedPath, low: Operand<QueryValue>, high: Operand<QueryValue>): Condition;
  in<K extends FieldNames<O>>(field: K, values: readonly Operand<ElementOf<O[K]>>[]): Condition;
  in(path: DottedPath, values: readonly Operand<QueryValue>[]): Condition;
  /** A string field contains `value`, or a list holds the element `value`. */
  contains<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
  contains(path: DottedPath, value: Operand<QueryValue>): Condition;
  startsWith<K extends FieldNames<O>>(field: K, value: Operand<string>): Condition;
  startsWith(path: DottedPath, value: Operand<string>): Condition;
  endsWith<K extends FieldNames<O>>(field: K, value: Operand<string>): Condition;
  endsWith(path: DottedPath, value: Operand<string>): Condition;
  isNull(field: FieldNames<O> | DottedPath): Condition;
  isNotNull(field: FieldNames<O> | DottedPath): Condition;
  and(...conditions: Condition[]): Condition;
  or(...conditions: Condition[]): Condition;
  not(condition: Condition): Condition;
}

/**
 * What to find, in what order, and how many. Without a sort, objects come in
 * primary key order, and objects that sort equal come in primary key order
 * too. Each method adds to the query and returns it.
 */
export interface Query<O = Record<string, unknown>> {
  where<K extends FieldNames<O>>(
    field: K,
    op: '==' | '!=',
    value: Operand<ElementOf<O[K]> | null>
  ): Query<O>;
  where<K extends FieldNames<O>>(
    field: K,
    op: Comparison,
    value: Operand<ElementOf<O[K]>>
  ): Query<O>;
  where<K extends FieldNames<O>>(
    field: K,
    op: 'between',
    value: readonly [Operand<ElementOf<O[K]>>, Operand<ElementOf<O[K]>>]
  ): Query<O>;
  where<K extends FieldNames<O>>(
    field: K,
    op: 'in',
    value: readonly Operand<ElementOf<O[K]>>[]
  ): Query<O>;
  where<K extends FieldNames<O>>(
    field: K,
    op: 'contains' | 'startsWith' | 'endsWith',
    value: Operand<ElementOf<O[K]>>
  ): Query<O>;
  where(
    path: DottedPath,
    op: Comparison | 'contains' | 'startsWith' | 'endsWith',
    value: Operand<QueryValue | null>
  ): Query<O>;
  where(
    path: DottedPath,
    op: 'between',
    value: readonly [Operand<QueryValue>, Operand<QueryValue>]
  ): Query<O>;
  where(path: DottedPath, op: 'in', value: readonly Operand<QueryValue>[]): Query<O>;
  where(condition: Condition | ((conditions: Conditions<O>) => Condition)): Query<O>;
  /** Sorts by a field, ascending unless told otherwise, after any sort before. */
  sortBy(field: FieldNames<O> | DottedPath, direction?: 'asc' | 'desc'): Query<O>;
  /** Returns at most `count` objects. */
  limit(count: number): Query<O>;
  /** Skips the first `count` objects. */
  offset(count: number): Query<O>;
}

/** How `Query` makes a query. */
export interface QueryConstructor {
  new <O = Record<string, unknown>>(): Query<O>;
}

/** A query as `find` and `count` take it. */
export type QueryInput<O> = ((query: Query<O>) => Query<O> | void) | Query<O>;

/** What a prepared query's type carries for the objects it finds. */
declare const objects: unique symbol;

/**
 * A query parsed once, on one collection, that each run gives values for
 * its parameters: `Database.prepare` makes one. It holds no database or
 * transaction, so it runs in any.
 */
export interface Prepared<O> {
  /** The collection the query runs on. */
  readonly collection: string;
  readonly [objects]?: O;
}

/** The values of a query's parameters, `$0` or `param(0)` first. */
export type QueryParameters = readonly (QueryValue | null)[];

/** A collection of a transaction, for reading its objects. */
export interface ReadCollection<O> {
  /** The collection's name. */
  readonly name: string;
  /** The object whose primary key is `key`, or `null`. */
  get(key: Key): O | null;
  /** The objects a query finds, in its order; every object without one. */
  find(query?: QueryInput<O>): O[];
  /** The objects a query in the query language finds, with `$0`, `$1` and on. */
  find(text: string, parameters?: QueryParameters): O[];
  /** The objects a prepared query finds with these values for its parameters. */
  find(prepared: Prepared<O>, parameters?: QueryParameters): O[];
  /** The first object a query finds, or `null`. */
  findOne(query?: QueryInput<O>): O | null;
  findOne(text: string, parameters?: QueryParameters): O | null;
  findOne(prepared: Prepared<O>, parameters?: QueryParameters): O | null;
  /** How many objects a query finds, after its offset and within its limit. */
  count(query?: QueryInput<O>): number;
  count(text: string, parameters?: QueryParameters): number;
  count(prepared: Prepared<O>, parameters?: QueryParameters): number;
}

/** A collection of a write transaction, for reading and writing its objects. */
export interface WriteCollection<O, I> extends ReadCollection<O> {
  /** Inserts `object` and returns its primary key. */
  insert(object: I): Key;
  /**
   * Inserts `objects` in one call into the engine and returns their keys. A
   * refused object stops the batch with its error, and the objects before it
   * stay inserted in the transaction.
   */
  insertMany(objects: readonly I[]): Key[];
  /** Inserts `object`, or replaces the object with its key. */
  put(object: I): Key;
  putMany(objects: readonly I[]): Key[];
  /**
   * Sets the fields `changes` has in the object whose primary key is `key`,
   * and says whether there was one; the rest of the object stays as it is.
   * `null` makes an optional field null and gives a field with a default
   * its default, and a field left `undefined` stays as it is. An embedded
   * object or a list is replaced whole. It is refused as `put` is, and for
   * a primary key other than `key`.
   */
  update(key: Key, changes: Partial<I>): boolean;
  /** Deletes the object whose primary key is `key`, and says whether there was one. */
  delete(key: Key): boolean;
}

/** A read transaction: one commit, for as long as its function runs. */
export interface ReadTransaction<S> {
  collection<N extends NameOf<S>>(name: N): ReadCollection<ObjectOf<FieldsOf<S, N>>>;
}

/** A write transaction: changes that commit together when its function returns. */
export interface WriteTransaction<S> {
  collection<N extends NameOf<S>>(
    name: N
  ): WriteCollection<ObjectOf<FieldsOf<S, N>>, InsertOf<FieldsOf<S, N>>>;
}

/** A collection of an asynchronous transaction, for reading its objects. */
export interface AsyncReadCollection<O> {
  /** The collection's name. */
  readonly name: string;
  /** The object whose primary key is `key`, or `null`. */
  get(key: Key): Promise<O | null>;
  /** The objects a query finds, in its order; every object without one. */
  find(query?: QueryInput<O>): Promise<O[]>;
  /** The objects a query in the query language finds, with `$0`, `$1` and on. */
  find(text: string, parameters?: QueryParameters): Promise<O[]>;
  /** The objects a prepared query finds with these values for its parameters. */
  find(prepared: Prepared<O>, parameters?: QueryParameters): Promise<O[]>;
  /** The first object a query finds, or `null`. */
  findOne(query?: QueryInput<O>): Promise<O | null>;
  findOne(text: string, parameters?: QueryParameters): Promise<O | null>;
  findOne(prepared: Prepared<O>, parameters?: QueryParameters): Promise<O | null>;
  /** How many objects a query finds, after its offset and within its limit. */
  count(query?: QueryInput<O>): Promise<number>;
  count(text: string, parameters?: QueryParameters): Promise<number>;
  count(prepared: Prepared<O>, parameters?: QueryParameters): Promise<number>;
}

/** A collection of an asynchronous write transaction, for reading and writing. */
export interface AsyncWriteCollection<O, I> extends AsyncReadCollection<O> {
  /** Inserts `object` and resolves to its primary key. */
  insert(object: I): Promise<Key>;
  /**
   * Inserts `objects` in one call into the engine and resolves to their
   * keys. A refused object stops the batch with its error, and the objects
   * before it stay inserted in the transaction.
   */
  insertMany(objects: readonly I[]): Promise<Key[]>;
  /** Inserts `object`, or replaces the object with its key. */
  put(object: I): Promise<Key>;
  putMany(objects: readonly I[]): Promise<Key[]>;
  /**
   * Sets the fields `changes` has in the object whose primary key is `key`,
   * and resolves to whether there was one; see `WriteCollection.update`.
   */
  update(key: Key, changes: Partial<I>): Promise<boolean>;
  /** Deletes the object whose primary key is `key`, and resolves to whether there was one. */
  delete(key: Key): Promise<boolean>;
}

/**
 * An asynchronous read transaction: one commit, until its function settles.
 * Its operations run one at a time, in the order they were called.
 */
export interface AsyncReadTransaction<S> {
  collection<N extends NameOf<S>>(name: N): AsyncReadCollection<ObjectOf<FieldsOf<S, N>>>;
}

/**
 * An asynchronous write transaction: changes that commit together when its
 * function resolves, once every operation called has settled.
 */
export interface AsyncWriteTransaction<S> {
  collection<N extends NameOf<S>>(
    name: N
  ): AsyncWriteCollection<ObjectOf<FieldsOf<S, N>>, InsertOf<FieldsOf<S, N>>>;
}

/**
 * The write transaction of a migration, as a migration function gets it:
 * the collections of the new schema, and the objects as the schema before
 * the migration read them.
 */
export interface Migrating<S> extends WriteTransaction<S> {
  /** The schema version the file held before the migration. */
  readonly previousVersion: number;
  /** The version this step migrates to. */
  readonly version: number;
  /**
   * The object of `collection`, named as before the migration, as the old
   * schema reads it. Read an object this way before writing it: a written
   * object keeps only the new schema's fields.
   */
  previous(collection: string, key: Key): Record<string, unknown> | null;
  /** The keys of every object of `collection`, named as before the migration. */
  previousKeys(collection: string): Key[];
}

/**
 * The write transaction of a migration, as an asynchronous migration
 * function gets it from `Database.openAsync`.
 */
export interface AsyncMigrating<S> extends AsyncWriteTransaction<S> {
  /** The schema version the file held before the migration. */
  readonly previousVersion: number;
  /** The version this step migrates to. */
  readonly version: number;
  /** `Migrating.previous`, resolved. */
  previous(collection: string, key: Key): Promise<Record<string, unknown> | null>;
  /** The keys of every object of `collection`, named as before the migration. */
  previousKeys(collection: string): Promise<Key[]>;
}

/** What schema version `version` changes from the version before it. */
export interface Migration<S = Schema> {
  version: number;
  /** Pairs of the old name and the new. Objects stay where they are. */
  renameCollections?: [from: string, to: string][];
  /** The collection's name before the migration, the field's old name and the new. */
  renameFields?: [collection: string, from: string, to: string][];
  /** Collections that go, with their objects. */
  deleteCollections?: string[];
  /** Fields replaced by a new field of the same name, as when a type changes. */
  replaceFields?: [collection: string, field: string][];
  /** Runs in the migration's write transaction, after the renames. */
  run?(migrating: Migrating<S>): void;
}

/** A migration of `Database.openAsync`, whose function may be asynchronous. */
export interface AsyncMigration<S = Schema> extends Omit<Migration<S>, 'run'> {
  /**
   * Runs in the migration's write transaction, after the renames. The step
   * ends once what it returns has settled, and every operation it called
   * with it.
   */
  run?(migrating: AsyncMigrating<S>): Promise<void> | void;
}

/** Options for `Database.open`. */
export interface OpenOptions<S = Schema> {
  /** Whether to create the database when nothing exists at the path. `true` by default. */
  create?: boolean;
  /** The page size of a new database: a power of two from 4096 to 65536. */
  pageSize?: number;
  /**
   * How long, in milliseconds, opening and a write transaction wait for
   * another process's writer before failing with `BUSY`. 5000 by default.
   */
  busyTimeout?: number;
  /**
   * How much memory the page cache may take, in bytes. 32 MiB by default. It
   * holds at least 16 pages, and fills only as pages are read. Every
   * `Database` of one file in a process shares the cache of the first.
   */
  cacheSize?: number;
  /** The collections the database holds. */
  schema?: S;
  /** How an older schema version becomes this one. */
  migrations?: Migration<S>[];
  /**
   * A key of 32 bytes that encrypts a new database, or opens an encrypted
   * one. Keep it where it cannot be lost, such as the operating system's
   * keystore: without it, the data cannot be read. A plain database cannot
   * be opened with a key. The package copies it when `open` is called, so
   * the caller may wipe its own buffer once the call returns.
   */
  key?: Uint8Array;
  /**
   * A password that encrypts a new database, or opens an encrypted one,
   * hashed with Argon2id into the key. Give a `key` or a `password`, not
   * both. A `Uint8Array` can be wiped once `open` returns; a string stays
   * in memory until the garbage collector reclaims it.
   */
  password?: string | Uint8Array;
  /**
   * How much work hashing a password takes when a new database is encrypted
   * with one or `setPassword` changes it. 19456 KiB, 2 iterations and a
   * parallelism of 1 by default, which takes tens of milliseconds. A file
   * records the cost it was made with, so opening it takes that cost
   * whatever this says.
   */
  passwordHashing?: PasswordHashing;
}

/** What hashing a password costs, as Argon2id counts it. */
export interface PasswordHashing {
  /** Memory, in KiB, up to 1 GiB. */
  memoryKib: number;
  iterations: number;
  parallelism: number;
}

/** Options for `Database.openAsync`, whose migration functions may be asynchronous. */
export interface AsyncOpenOptions<S = Schema> extends Omit<OpenOptions<S>, 'migrations'> {
  /** How an older schema version becomes this one. */
  migrations?: AsyncMigration<S>[];
}

/** How a write commits: waiting for the disk, or not. */
export interface WriteOptions {
  /**
   * `'sync'`, the default, returns once the commit is durable. `'deferred'`
   * returns at once: readers see it, a crash of the process loses none of
   * it, and it becomes durable at the next sync or within a second.
   */
  durability?: 'sync' | 'deferred';
}

/**
 * An open database. There is no constructor: use `Database.open` or
 * `Database.openAsync`.
 *
 * Every method that uses the file has an asynchronous twin whose name ends
 * in `Async`, which does the engine's work on the thread pool and resolves a
 * promise, so the event loop never waits for the disk or for another
 * process's writer. `prepare` has none: it only parses a query.
 */
export interface Database<S extends Schema<any> = Schema> {
  /** The path the database was opened at. Still readable after `close`. */
  readonly path: string;
  /** Whether `close` has not been called. */
  readonly isOpen: boolean;
  /** The size of every page in the file, in bytes. */
  readonly pageSize: number;
  /** The file format version recorded in the file. */
  readonly formatVersion: number;
  /** Whether the file is encrypted. */
  readonly isEncrypted: boolean;
  /** The schema version the file holds, or `null` without a schema. */
  readonly schemaVersion: number | null;
  /**
   * Prepares a query on collection `collection`: text in the query language
   * with `$0`, `$1` and on, or a query built with `param` in place of values.
   * It is parsed once here, and each `find`, `findOne` or `count` gives its
   * parameters' values.
   */
  prepare<N extends NameOf<S>>(
    collection: N,
    query: QueryInput<ObjectOf<FieldsOf<S, N>>> | string
  ): Prepared<ObjectOf<FieldsOf<S, N>>>;
  /** Runs `fn` in a read transaction and returns what it returns. */
  read<R>(fn: (txn: ReadTransaction<S>) => R): R;
  /**
   * Runs `fn` in a write transaction, commits it when `fn` returns, aborts it
   * when `fn` throws, and returns what `fn` returns.
   */
  write<R>(fn: (txn: WriteTransaction<S>) => R, options?: WriteOptions): R;
  /**
   * Runs `fn`, which may be asynchronous, in a read transaction, and
   * resolves to what it resolves to.
   */
  readAsync<R>(fn: (txn: AsyncReadTransaction<S>) => R): Promise<Awaited<R>>;
  /**
   * Runs `fn`, which may be asynchronous, in a write transaction, commits it
   * when `fn` resolves, aborts it when `fn` rejects, and resolves to what
   * `fn` resolves to. This process's writes on one file run one after
   * another.
   */
  writeAsync<R>(
    fn: (txn: AsyncWriteTransaction<S>) => R,
    options?: WriteOptions
  ): Promise<Awaited<R>>;
  /**
   * Checks the published commit completely: every page against its check,
   * the order of every key, every count, that every page is used, free or
   * retained exactly once, and every object against its indexes. It
   * reports every problem it finds rather than throwing, and reads while
   * other handles and processes write.
   */
  check(): CheckReport;
  /** `check` on the thread pool. */
  checkAsync(): Promise<CheckReport>;
  /**
   * Writes a copy of the published commit to a new file at `path`, while
   * other handles and processes may write. The copy holds no free space,
   * has the file's page size, and opens with the same key or password, or
   * with the `key` or `password` of `options`, which encrypt it under a new
   * data key. It never replaces a file: a path that is taken fails with
   * `INVALID_ARGUMENT`.
   */
  backup(path: string, options?: BackupOptions): BackupReport;
  /** `backup` on the thread pool. */
  backupAsync(path: string, options?: BackupOptions): Promise<BackupReport>;
  /**
   * Makes the file smaller in place: the trees whose pages inserts left part
   * empty are written again, full, and the file's end moves into free pages
   * nearer its start and goes back to the file system. It writes, so it waits
   * for the writer lock, and it is refused while an asynchronous write of
   * this process holds the file.
   */
  compact(): CompactReport;
  /** `compact` on the thread pool, after this process's writes on the file. */
  compactAsync(): Promise<CompactReport>;
  /**
   * Changes the key of an encrypted database to `key`, 32 bytes. It re-encrypts
   * no page, and when it returns, the old key or password no longer opens the
   * file. It commits, so it is refused while an asynchronous write of this
   * process holds the file. A plain database fails with `INVALID_ARGUMENT`.
   */
  setKey(key: Uint8Array): void;
  /** `setKey` on the thread pool, after this process's writes on the file. */
  setKeyAsync(key: Uint8Array): Promise<void>;
  /**
   * Changes the key of an encrypted database to one derived from `password`,
   * at the hashing cost the database was opened with; see `setKey`.
   */
  setPassword(password: string | Uint8Array): void;
  /** `setPassword` on the thread pool, after this process's writes on the file. */
  setPasswordAsync(password: string | Uint8Array): Promise<void>;
  /** Makes every commit durable, deferred ones included. */
  sync(): void;
  /** `sync` on the thread pool, after this process's writes on the file. */
  syncAsync(): Promise<void>;
  /** Makes deferred commits durable and closes the database. */
  close(): void;
  /**
   * `close` on the thread pool, after this process's writes on the file. The
   * database refuses new work at once.
   */
  closeAsync(): Promise<void>;
}

/** What `Database.check` found: the commit it checked and every problem. */
export interface CheckReport {
  /** Whether the check found nothing wrong. */
  ok: boolean;
  /** The transaction id of the commit checked, the one published when it began. */
  commitId: number;
  /** The pages that commit counts, the header page included. */
  pageCount: number;
  /** The pages read and verified. */
  pagesChecked: number;
  /** The objects read and checked against their indexes. */
  objectsChecked: number;
  /** Every problem found, in the order found. */
  problems: CheckProblem[];
}

/** What `Database.compact` did. */
export interface CompactReport {
  /** The size of the file before, in bytes. */
  bytesBefore: number;
  /** The size of the file after, in bytes. */
  bytesAfter: number;
  /** The pages moved out of the file's end. */
  pagesMoved: number;
}

/**
 * Options for `Database.backup`. Without a key or password, a copy of an
 * encrypted file keeps its data key. With one, the copy is encrypted under a
 * new random data key, which the key or password wraps: changing a file's key
 * or password only wraps its data key again, so a backup is the way to leave
 * behind a data key that may have been exposed. A plain database's copy is
 * encrypted the same way. Give a `key` or a `password`, not both.
 */
export interface BackupOptions {
  /** A key of 32 bytes for the copy. The package copies it when the call is made. */
  key?: Uint8Array;
  /** A password for the copy, hashed with Argon2id into the key. */
  password?: string | Uint8Array;
  /** How much work hashing the copy's password takes, as `OpenOptions` says. */
  passwordHashing?: PasswordHashing;
}

/** What `Database.backup` wrote. */
export interface BackupReport {
  /** The transaction id of the commit copied, the one published when the backup began. */
  commitId: number;
  /** The trees copied, the engine's own included. */
  trees: number;
  /** The entries copied. */
  entries: number;
  /** The size of the new file, in bytes. */
  bytes: number;
}

/** One thing the integrity check found wrong. */
export interface CheckProblem {
  /** The page the problem is in, when it is in one page. */
  page: number | null;
  /** The tree or the collection it was found in, when it was found in one. */
  tree: string | null;
  /** What is wrong. */
  message: string;
}

/** How `Database` opens a database: it has no constructor. */
export interface DatabaseOpener {
  /**
   * Opens the database at `path`, creating it if nothing exists there, and
   * stores, checks or migrates its schema.
   */
  open<S extends Schema<any>>(path: string, options: OpenOptions<S> & { schema: S }): Database<S>;
  open(path: string, options?: OpenOptions<never>): Database;
  /** `open` on the thread pool, with asynchronous migration functions. */
  openAsync<S extends Schema<any>>(
    path: string,
    options: AsyncOpenOptions<S> & { schema: S }
  ): Promise<Database<S>>;
  openAsync(path: string, options?: AsyncOpenOptions<never>): Promise<Database>;
  /**
   * Rescues what it can of the damaged database at `from` into a new
   * database at `into`, and reports what it rescued and what it could not.
   * It reads the file page by page, so it works on a file that does not
   * open. It starts from the newest commit the file records, takes what
   * that commit cannot read from older versions of the same pages where the
   * file still has them, and builds every index again from the objects, so
   * the new file passes the integrity check.
   *
   * It needs the file alone: a file open in this process or another fails
   * with `BUSY`, and so does opening the file while it runs. It never
   * replaces a file: a path that is taken fails with `INVALID_ARGUMENT`.
   */
  salvage(from: string, into: string, options?: SalvageOptions): SalvageReport;
  /** `salvage` on the thread pool. */
  salvageAsync(from: string, into: string, options?: SalvageOptions): Promise<SalvageReport>;
}

/** Options for `Database.salvage`. */
export interface SalvageOptions {
  /** The key of an encrypted file, which opens the new file too. */
  key?: Uint8Array;
  /** The password of an encrypted file, which opens the new file too. */
  password?: string | Uint8Array;
  /**
   * How long, in milliseconds, to wait for other processes to close the
   * file before failing with `BUSY`. 5000 by default.
   */
  busyTimeout?: number;
}

/** What `Database.salvage` rescued, and what it could not. */
export interface SalvageReport {
  /**
   * Whether the new file holds exactly the commit salvage started from:
   * every page of it was read, and no object was dropped.
   */
  whole: boolean;
  /**
   * The transaction id of the commit salvage started from, or `null` when
   * no commit record could be used and every tree came from the pages found.
   */
  commitId: number | null;
  /** The pages of the file read, the header page left out. */
  pagesScanned: number;
  /** The pages that failed their check, other than pages never written. */
  pagesDamaged: number;
  /**
   * The pages of the commit that could not be read, each value too large
   * for a page counted as one: what they held was taken from older versions
   * of the same pages, where the file still had them.
   */
  pagesUnread: number;
  /** The entries taken from those older versions. */
  entriesRecovered: number;
  /** The keys left out because no version of their value could be read. */
  valuesLost: number;
  /**
   * The objects left out: those that could not be read, those whose value
   * of a unique index another object had taken, and every object of a file
   * whose schema was lost.
   */
  objectsDropped: number;
  /** The trees of the new file, the engine's own included. */
  trees: number;
  /** The entries of the new file, the indexes' included. */
  entries: number;
  /** The size of the new file, in bytes. */
  bytes: number;
}
