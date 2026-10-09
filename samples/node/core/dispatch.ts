/**
 * The one entry point the hosts call with what a screen sent: a method's
 * name and its arguments. A renderer and an HTTP request are both outside
 * the process that holds the database, so the arguments are checked here,
 * by shape and by range, before the store sees them; the engine then checks
 * every value it is given. The reply is a `Reply`, never a thrown error,
 * since neither IPC nor HTTP carries an error's code across on its own.
 */
import { COLLECTIONS, isCollectionName } from './fields.ts';
import type { CollectionName } from './fields.ts';
import { METHODS } from './protocol.ts';
import type {
  Key,
  ListRequest,
  Method,
  Reply,
  SeedOptions,
  SeedProgress,
  WireObject
} from './protocol.ts';
import { SampleError } from './store.ts';
import type { SampleStore } from './store.ts';

/** The most objects a page of the list shows. */
const PAGE_LIMIT_MAX = 500;

/** The most people one sample run makes. */
const SEED_PEOPLE_MAX = 1_000_000;

/** The longest filter accepted, in characters. */
const FILTER_LENGTH_MAX = 2000;

const invalid = (message: string): SampleError => new SampleError('INVALID_ARGUMENT', message);

const recordOf = (value: unknown, what: string): Record<string, unknown> => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw invalid(`${what} must be an object`);
  }

  return value as Record<string, unknown>;
};

const collectionOf = (value: unknown): CollectionName => {
  if (!isCollectionName(value)) {
    throw invalid(`there is no collection ${JSON.stringify(value)}`);
  }

  return value;
};

const integerOf = (value: unknown, what: string, low: number, high: number): number => {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < low || value > high) {
    throw invalid(`${what} must be an integer from ${low} to ${high}`);
  }

  return value;
};

/** A primary key of `collection`: a number where the engine numbers objects, a string otherwise. */
const keyOf = (collection: CollectionName, value: unknown): Key => {
  if (COLLECTIONS[collection].autoKey) {
    return integerOf(value, 'the key', 1, Number.MAX_SAFE_INTEGER);
  }

  if (typeof value !== 'string' || value.length === 0) {
    throw invalid('the key must be a string');
  }

  return value;
};

const listRequestOf = (args: unknown): ListRequest => {
  const request = recordOf(args, 'the request');
  const collection = collectionOf(request.collection);
  const filter = request.filter ?? '';

  if (typeof filter !== 'string' || filter.length > FILTER_LENGTH_MAX) {
    throw invalid(`the filter must be text of at most ${FILTER_LENGTH_MAX} characters`);
  }

  let sort: ListRequest['sort'] = null;

  if (request.sort !== null && request.sort !== undefined) {
    const { field, direction } = recordOf(request.sort, 'the sort');

    // The field goes into the query's text, so only the names the list
    // offers are let through.
    if (typeof field !== 'string' || !COLLECTIONS[collection].sortable.includes(field)) {
      throw invalid(`${collection} cannot be sorted by ${JSON.stringify(field)}`);
    }

    if (direction !== 'asc' && direction !== 'desc') {
      throw invalid('the sort direction must be asc or desc');
    }

    sort = { field, direction };
  }

  return {
    collection,
    filter,
    sort,
    offset: integerOf(request.offset ?? 0, 'the offset', 0, Number.MAX_SAFE_INTEGER),
    limit: integerOf(request.limit ?? 50, 'the limit', 1, PAGE_LIMIT_MAX)
  };
};

const seedOptionsOf = (args: unknown): SeedOptions => {
  const options = recordOf(args, 'the options');

  return {
    people: integerOf(options.people, 'the number of people', 1, SEED_PEOPLE_MAX),
    seed: integerOf(options.seed, 'the seed', 0, 0xffffffff)
  };
};

const objectOf = (value: unknown, what: string): WireObject => recordOf(value, what) as WireObject;

const encryptedOf = (args: unknown): boolean => {
  const { encrypted } = recordOf(args, 'the arguments');

  if (typeof encrypted !== 'boolean') {
    throw invalid('encrypted must be true or false');
  }

  return encrypted;
};

const run = (
  store: SampleStore,
  method: Method,
  args: unknown,
  onProgress: (progress: SeedProgress) => void
): Promise<unknown> => {
  switch (method) {
    case 'info':
      return store.info();
    case 'list':
      return store.list(listRequestOf(args));
    case 'insert': {
      const { collection, object } = recordOf(args, 'the arguments');
      const name = collectionOf(collection);

      return store.insert(name, objectOf(object, 'the object'));
    }
    case 'update': {
      const { collection, key, changes } = recordOf(args, 'the arguments');
      const name = collectionOf(collection);

      return store.update(name, keyOf(name, key), objectOf(changes, 'the changes'));
    }
    case 'remove': {
      const { collection, key } = recordOf(args, 'the arguments');
      const name = collectionOf(collection);

      return store.remove(name, keyOf(name, key));
    }
    case 'seed':
      return store.seed(seedOptionsOf(args), onProgress);
    case 'check':
      return store.check();
    case 'compact':
      return store.compact();
    case 'reset':
      return store.reset(encryptedOf(args));
  }
};

export const isMethod = (value: unknown): value is Method =>
  typeof value === 'string' && (METHODS as readonly string[]).includes(value);

/** Runs `method` on `store`, and says how it went. */
export const dispatch = async (
  store: SampleStore,
  method: unknown,
  args: unknown,
  onProgress: (progress: SeedProgress) => void = () => {}
): Promise<Reply<unknown>> => {
  try {
    if (!isMethod(method)) {
      throw invalid(`there is no method ${JSON.stringify(method)}`);
    }

    return { ok: true, value: await run(store, method, args, onProgress) };
  } catch (error) {
    const code =
      typeof error === 'object' && error !== null && typeof Reflect.get(error, 'code') === 'string'
        ? (Reflect.get(error, 'code') as string)
        : 'INTERNAL';

    return { ok: false, code, message: error instanceof Error ? error.message : String(error) };
  }
};
