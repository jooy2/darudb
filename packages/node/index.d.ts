/**
 * DaruDB for Node.js: an embedded database that keeps an application's data
 * in one local file.
 *
 * A schema declared with `t`, `collection` and `schema` types every object
 * the database reads and writes: `Database.open` with a schema gives a
 * `Database` whose collections know their objects' fields.
 */

/** The file format version this build of the engine reads and writes. */
export declare const FORMAT_VERSION: number;

/** The version of the DaruDB engine inside this package. */
export declare function engineVersion(): string;

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

/** The types of fields. */
export declare const t: {
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
};

/** A collection: its fields. */
export interface Collection<F extends Fields = Fields> {
  readonly fields: F;
}

/**
 * A collection with `fields`. A field marked `primaryKey()` is the key;
 * without one, the collection gets an `id` that the engine numbers from 1.
 */
export declare function collection<F extends Fields>(fields: F): Collection<F>;

/** The collections of a database, at a version. */
export interface Schema<C extends Record<string, Collection<any>> = Record<string, Collection>> {
  readonly version: number;
  readonly collections: C;
}

/**
 * A schema: `collections` by name, at `version`, from 1 up. Raise the
 * version whenever the schema changes.
 */
export declare function schema<C extends Record<string, Collection<any>>>(
  version: number,
  collections: C
): Schema<C>;

type CollectionsOf<S> = S extends Schema<infer C> ? C : Record<string, Collection>;
type NameOf<S> = keyof CollectionsOf<S> & string;
type FieldsOf<S, N extends NameOf<S>> =
  CollectionsOf<S>[N] extends Collection<infer F> ? F : Fields;

/** A value a query compares with. */
export type QueryValue = boolean | number | bigint | string | Uint8Array;

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
  eq<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]> | null): Condition;
  eq(path: DottedPath, value: QueryValue | null): Condition;
  ne<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]> | null): Condition;
  ne(path: DottedPath, value: QueryValue | null): Condition;
  lt<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]>): Condition;
  lt(path: DottedPath, value: QueryValue): Condition;
  le<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]>): Condition;
  le(path: DottedPath, value: QueryValue): Condition;
  gt<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]>): Condition;
  gt(path: DottedPath, value: QueryValue): Condition;
  ge<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]>): Condition;
  ge(path: DottedPath, value: QueryValue): Condition;
  between<K extends FieldNames<O>>(
    field: K,
    low: ElementOf<O[K]>,
    high: ElementOf<O[K]>
  ): Condition;
  between(path: DottedPath, low: QueryValue, high: QueryValue): Condition;
  in<K extends FieldNames<O>>(field: K, values: readonly ElementOf<O[K]>[]): Condition;
  in(path: DottedPath, values: readonly QueryValue[]): Condition;
  /** A string field contains `value`, or a list holds the element `value`. */
  contains<K extends FieldNames<O>>(field: K, value: ElementOf<O[K]>): Condition;
  contains(path: DottedPath, value: QueryValue): Condition;
  startsWith<K extends FieldNames<O>>(field: K, value: string): Condition;
  startsWith(path: DottedPath, value: string): Condition;
  endsWith<K extends FieldNames<O>>(field: K, value: string): Condition;
  endsWith(path: DottedPath, value: string): Condition;
  isNull(field: FieldNames<O> | DottedPath): Condition;
  isNotNull(field: FieldNames<O> | DottedPath): Condition;
  and(...conditions: Condition[]): Condition;
  or(...conditions: Condition[]): Condition;
  not(condition: Condition): Condition;
}

/** The conditions a filter is made of, for building one outside a query. */
export declare const conditions: Conditions;

/**
 * What to find, in what order, and how many. Without a sort, objects come in
 * primary key order, and objects that sort equal come in primary key order
 * too. Each method adds to the query and returns it.
 */
export declare class Query<O = Record<string, unknown>> {
  constructor();
  where<K extends FieldNames<O>>(
    field: K,
    op: '==' | '!=',
    value: ElementOf<O[K]> | null
  ): Query<O>;
  where<K extends FieldNames<O>>(field: K, op: Comparison, value: ElementOf<O[K]>): Query<O>;
  where<K extends FieldNames<O>>(
    field: K,
    op: 'between',
    value: readonly [ElementOf<O[K]>, ElementOf<O[K]>]
  ): Query<O>;
  where<K extends FieldNames<O>>(field: K, op: 'in', value: readonly ElementOf<O[K]>[]): Query<O>;
  where<K extends FieldNames<O>>(
    field: K,
    op: 'contains' | 'startsWith' | 'endsWith',
    value: ElementOf<O[K]>
  ): Query<O>;
  where(
    path: DottedPath,
    op: Comparison | 'contains' | 'startsWith' | 'endsWith',
    value: QueryValue | null
  ): Query<O>;
  where(path: DottedPath, op: 'between', value: readonly [QueryValue, QueryValue]): Query<O>;
  where(path: DottedPath, op: 'in', value: readonly QueryValue[]): Query<O>;
  where(condition: Condition | ((conditions: Conditions<O>) => Condition)): Query<O>;
  /** Sorts by a field, ascending unless told otherwise, after any sort before. */
  sortBy(field: FieldNames<O> | DottedPath, direction?: 'asc' | 'desc'): Query<O>;
  /** Returns at most `count` objects. */
  limit(count: number): Query<O>;
  /** Skips the first `count` objects. */
  offset(count: number): Query<O>;
}

/** A query as `find` and `count` take it. */
export type QueryInput<O> = ((query: Query<O>) => Query<O> | void) | Query<O>;

/** A collection of a transaction, for reading its objects. */
export interface ReadCollection<O> {
  /** The collection's name. */
  readonly name: string;
  /** The object whose primary key is `key`, or `null`. */
  get(key: Key): O | null;
  /** The objects a query finds, in its order; every object without one. */
  find(query?: QueryInput<O>): O[];
  /** The objects a query in the query language finds, with `$0`, `$1` and on. */
  find(text: string, parameters?: readonly (QueryValue | null)[]): O[];
  /** The first object a query finds, or `null`. */
  findOne(query?: QueryInput<O>): O | null;
  findOne(text: string, parameters?: readonly (QueryValue | null)[]): O | null;
  /** How many objects a query finds, after its offset and within its limit. */
  count(query?: QueryInput<O>): number;
  count(text: string, parameters?: readonly (QueryValue | null)[]): number;
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
  /** The collections the database holds. */
  schema?: S;
  /** How an older schema version becomes this one. */
  migrations?: Migration<S>[];
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

/** An open database. There is no constructor: use `Database.open`. */
export declare class Database<S extends Schema<any> = Schema> {
  private constructor();
  /**
   * Opens the database at `path`, creating it if nothing exists there, and
   * stores, checks or migrates its schema.
   */
  static open<S extends Schema<any>>(
    path: string,
    options: OpenOptions<S> & { schema: S }
  ): Database<S>;
  static open(path: string, options?: OpenOptions<never>): Database;
  /** The path the database was opened at. Still readable after `close`. */
  readonly path: string;
  /** Whether `close` has not been called. */
  readonly isOpen: boolean;
  /** The size of every page in the file, in bytes. */
  readonly pageSize: number;
  /** The file format version recorded in the file. */
  readonly formatVersion: number;
  /** The schema version the file holds, or `null` without a schema. */
  readonly schemaVersion: number | null;
  /** Runs `fn` in a read transaction and returns what it returns. */
  read<R>(fn: (txn: ReadTransaction<S>) => R): R;
  /**
   * Runs `fn` in a write transaction, commits it when `fn` returns, aborts it
   * when `fn` throws, and returns what `fn` returns.
   */
  write<R>(fn: (txn: WriteTransaction<S>) => R, options?: WriteOptions): R;
  /** Makes every commit durable, deferred ones included. */
  sync(): void;
  /** Makes deferred commits durable and closes the database. */
  close(): void;
}
