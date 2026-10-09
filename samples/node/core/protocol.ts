/**
 * What the screens and the process that holds the database say to each
 * other. The Electron app carries it over IPC and the web page over HTTP,
 * so everything here is plain JSON: an object's bytes travel as hex, and
 * an error as its code and message.
 *
 * Types only, apart from the list of methods, so the renderer imports the
 * module as it is.
 */
import type { CollectionName } from './fields.ts';

export type Key = number | string;

/** The embedded `location` of a person. */
export interface WireLocation {
  country: string;
  region: string | null;
  city: string | null;
}

/** A field's value as it crosses: bytes as a string of hex digits. */
export type WireValue = string | number | boolean | null | string[] | WireLocation;

/** An object as it crosses, its key among its fields. */
export type WireObject = Record<string, WireValue>;

export type Host = 'electron' | 'web';

export interface Info {
  host: Host;
  path: string;
  /** The size of the file, in bytes. */
  bytes: number;
  pageSize: number;
  formatVersion: number;
  schemaVersion: number | null;
  encrypted: boolean;
  engineVersion: string;
  /** How many objects each collection holds. */
  counts: Record<CollectionName, number>;
}

export interface Sort {
  field: string;
  direction: 'asc' | 'desc';
}

export interface ListRequest {
  collection: CollectionName;
  /** A condition in the query language, or empty for every object. */
  filter: string;
  sort: Sort | null;
  offset: number;
  limit: number;
}

export interface ListResult {
  objects: WireObject[];
  /** How many objects the filter finds, past the page shown. */
  total: number;
}

export interface SeedOptions {
  /** How many people to make: organizations and posts follow from it. */
  people: number;
  /** Where the sample data starts from: the same seed makes the same data. */
  seed: number;
}

export type SeedStage = 'pools' | 'organizations' | 'people' | 'posts';

export interface SeedProgress {
  stage: SeedStage;
  done: number;
  total: number;
}

export interface SeedReport {
  organizations: number;
  people: number;
  posts: number;
  /** Time spent making the objects, in milliseconds. */
  generateMs: number;
  /** Time spent in write transactions, in milliseconds. */
  insertMs: number;
}

export interface CheckSummary {
  ok: boolean;
  commitId: number;
  pagesChecked: number;
  objectsChecked: number;
  problems: string[];
}

export interface CompactSummary {
  bytesBefore: number;
  bytesAfter: number;
  pagesMoved: number;
}

/** Each method's arguments and result. */
export interface Methods {
  info: { args: null; result: Info };
  list: { args: ListRequest; result: ListResult };
  insert: { args: { collection: CollectionName; object: WireObject }; result: Key };
  update: {
    args: { collection: CollectionName; key: Key; changes: WireObject };
    result: boolean;
  };
  remove: { args: { collection: CollectionName; key: Key }; result: boolean };
  seed: { args: SeedOptions; result: SeedReport };
  check: { args: null; result: CheckSummary };
  compact: { args: null; result: CompactSummary };
  /** Makes the file again, empty, encrypted or not. */
  reset: { args: { encrypted: boolean }; result: Info };
}

export type Method = keyof Methods;

export const METHODS: readonly Method[] = [
  'info',
  'list',
  'insert',
  'update',
  'remove',
  'seed',
  'check',
  'compact',
  'reset'
];

export type Reply<T> = { ok: true; value: T } | { ok: false; code: string; message: string };
