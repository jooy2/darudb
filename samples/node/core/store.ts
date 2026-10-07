/**
 * The sample's database: the one file it keeps, and every operation the
 * screens ask of it. The Electron app and the web server both hold one
 * `SampleStore` and hand it what `dispatch.ts` checked.
 *
 * Every call goes through the asynchronous API, which does the engine's work
 * on the thread pool: in Electron's main process a wait for the disk would
 * stall every window, and in a server every request. Objects leave the
 * store as `WireObject`s, with bytes as hex, and come back the same way.
 */
import { mkdir, rm, stat } from 'node:fs/promises';
import { join } from 'node:path';

import { Database, engineVersion } from 'darudb';
import type { AsyncReadCollection, AsyncWriteCollection } from 'darudb';

import { COLLECTIONS } from './fields.ts';
import type { CollectionName, FieldInfo } from './fields.ts';
import type {
  CheckSummary,
  CompactSummary,
  Host,
  Info,
  Key,
  ListRequest,
  ListResult,
  SeedOptions,
  SeedProgress,
  SeedReport,
  WireObject,
  WireValue
} from './protocol.ts';
import { planOf } from './plan.ts';
import { ORGANIZATION_PREFIX, SampleData } from './sample.ts';
import type { Author } from './sample.ts';
import { sampleSchema } from './schema.ts';
import type { SampleSchema } from './schema.ts';

export const FILE_NAME = 'sample.darudb';

/** How many objects a write transaction of a sample run inserts. */
const BATCH_SIZE = 5000;

/** The most problems a check reports to the screens. */
const PROBLEMS_SHOWN = 50;

const HEX_COLOR = /^[0-9a-f]{6}$/i;

/** A failure of the sample's own, with a code like the engine's. */
export class SampleError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

type LooseObject = Record<string, unknown>;

const readCollection = (
  txn: { collection(name: CollectionName): unknown },
  name: CollectionName
): AsyncReadCollection<LooseObject> => txn.collection(name) as AsyncReadCollection<LooseObject>;

const writeCollection = (
  txn: { collection(name: CollectionName): unknown },
  name: CollectionName
): AsyncWriteCollection<LooseObject, LooseObject> =>
  txn.collection(name) as AsyncWriteCollection<LooseObject, LooseObject>;

const toHex = (bytes: Uint8Array): string =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');

const fromHex = (hex: string): Uint8Array =>
  Uint8Array.from(hex.match(/../g) ?? [], (pair) => Number.parseInt(pair, 16));

/** An object as the screens get it: bytes become hex, everything else is JSON already. */
const toWire = (object: LooseObject): WireObject => {
  const wire: WireObject = {};

  for (const [name, value] of Object.entries(object)) {
    wire[name] = value instanceof Uint8Array ? toHex(value) : (value as WireValue);
  }

  return wire;
};

const toValue = (field: FieldInfo, value: WireValue): unknown => {
  if (field.kind === 'color' && value !== null) {
    if (typeof value !== 'string' || !HEX_COLOR.test(value)) {
      throw new SampleError('INVALID_ARGUMENT', `${field.name} must be six hex digits`);
    }

    return fromHex(value);
  }

  return value;
};

/**
 * An object as the engine takes it, from what the screens sent. Only the
 * collection's fields are kept, and the engine checks their values.
 */
const fromWire = (collection: CollectionName, wire: WireObject): LooseObject => {
  const object: LooseObject = {};

  for (const field of COLLECTIONS[collection].fields) {
    if (Object.hasOwn(wire, field.name)) {
      object[field.name] = toValue(field, wire[field.name]);
    }
  }

  return object;
};

/** The text of a list request in the query language: its filter, sort and page. */
const queryText = (request: ListRequest): string => {
  const parts = [request.filter.trim()];

  if (request.sort !== null) {
    parts.push(
      `SORT BY ${request.sort.field} ${request.sort.direction === 'desc' ? 'DESC' : 'ASC'}`
    );
  }

  parts.push(`LIMIT ${request.limit} OFFSET ${request.offset}`);

  return parts.filter((part) => part.length > 0).join(' ');
};

export class SampleStore {
  readonly path: string;
  readonly host: Host;
  #db: Database<SampleSchema>;

  private constructor(path: string, host: Host, db: Database<SampleSchema>) {
    this.path = path;
    this.host = host;
    this.#db = db;
  }

  /** Opens the sample's file in `directory`, creating both when they are not there. */
  static async open(directory: string, host: Host): Promise<SampleStore> {
    await mkdir(directory, { recursive: true });

    const path = join(directory, FILE_NAME);
    const db = await Database.openAsync(path, { schema: sampleSchema });

    return new SampleStore(path, host, db);
  }

  async info(): Promise<Info> {
    const db = this.#db;
    const [organizations, people, posts] = await db.readAsync((txn) =>
      Promise.all([
        txn.collection('organizations').count(),
        txn.collection('people').count(),
        txn.collection('posts').count()
      ])
    );
    const { size } = await stat(this.path);

    return {
      host: this.host,
      path: this.path,
      bytes: size,
      pageSize: db.pageSize,
      formatVersion: db.formatVersion,
      schemaVersion: db.schemaVersion,
      encrypted: db.isEncrypted,
      engineVersion: engineVersion(),
      counts: { organizations, people, posts }
    };
  }

  async list(request: ListRequest): Promise<ListResult> {
    const [objects, total] = await this.#db.readAsync((txn) => {
      const collection = readCollection(txn, request.collection);

      return Promise.all([collection.find(queryText(request)), collection.count(request.filter)]);
    });

    return { objects: objects.map(toWire), total };
  }

  async insert(collection: CollectionName, object: WireObject): Promise<Key> {
    const key = await this.#db.writeAsync((txn) =>
      writeCollection(txn, collection).insert(fromWire(collection, object))
    );

    return key as Key;
  }

  async update(collection: CollectionName, key: Key, changes: WireObject): Promise<boolean> {
    return this.#db.writeAsync((txn) =>
      writeCollection(txn, collection).update(key, fromWire(collection, changes))
    );
  }

  async remove(collection: CollectionName, key: Key): Promise<boolean> {
    return this.#db.writeAsync((txn) => writeCollection(txn, collection).delete(key));
  }

  /**
   * Inserts a run of sample data: `options.people` people, an organization
   * for every fifty of them and three posts each, in write transactions of
   * `BATCH_SIZE` objects. The report keeps the time spent making objects
   * apart from the time spent writing them, which is the engine's.
   */
  async seed(
    options: SeedOptions,
    onProgress: (progress: SeedProgress) => void
  ): Promise<SeedReport> {
    const plan = planOf(options.people);
    const timing = { generateMs: 0, insertMs: 0 };
    const time = <T>(slot: keyof typeof timing, work: () => T): T => {
      const started = performance.now();
      const result = work();

      timing[slot] += performance.now() - started;

      return result;
    };

    onProgress({ stage: 'pools', done: 0, total: 1 });

    const data = time('generateMs', () => new SampleData(options.seed));
    const starts = await this.#nextNumbers();
    const codes: string[] = [];
    const authors: Author[] = [];
    const runBatches = async <T>(
      stage: SeedProgress['stage'],
      total: number,
      make: (number: number) => T,
      write: (batch: T[]) => Promise<void>
    ): Promise<void> => {
      for (let done = 0; done < total; done += BATCH_SIZE) {
        const count = Math.min(BATCH_SIZE, total - done);
        const batch = time('generateMs', () =>
          Array.from({ length: count }, (_, i) => make(done + i))
        );
        const started = performance.now();

        await write(batch);
        timing.insertMs += performance.now() - started;
        onProgress({ stage, done: done + count, total });
      }
    };

    await runBatches(
      'organizations',
      plan.organizations,
      (i) => data.organization(starts.organization + i),
      async (batch) => {
        await this.#db.writeAsync((txn) => txn.collection('organizations').insertMany(batch));
        codes.push(...batch.map((organization) => organization.code));
      }
    );
    await runBatches(
      'people',
      plan.people,
      (i) => data.person(starts.person + i, codes),
      async (batch) => {
        const keys = await this.#db.writeAsync((txn) => txn.collection('people').insertMany(batch));

        keys.forEach((key, i) => authors.push({ key: Number(key), language: batch[i].language }));
      }
    );

    if (authors.length > 0) {
      await runBatches(
        'posts',
        plan.posts,
        (i) => data.post(starts.post + i, authors),
        async (batch) => {
          await this.#db.writeAsync((txn) => txn.collection('posts').insertMany(batch));
        }
      );
    }

    return {
      organizations: plan.organizations,
      people: plan.people,
      posts: authors.length > 0 ? plan.posts : 0,
      generateMs: Math.round(timing.generateMs),
      insertMs: Math.round(timing.insertMs)
    };
  }

  async check(): Promise<CheckSummary> {
    const report = await this.#db.checkAsync();

    return {
      ok: report.ok,
      commitId: report.commitId,
      pagesChecked: report.pagesChecked,
      objectsChecked: report.objectsChecked,
      problems: report.problems.slice(0, PROBLEMS_SHOWN).map((problem) => problem.message)
    };
  }

  async compact(): Promise<CompactSummary> {
    const report = await this.#db.compactAsync();

    return {
      bytesBefore: report.bytesBefore,
      bytesAfter: report.bytesAfter,
      pagesMoved: report.pagesMoved
    };
  }

  /** Closes the file, deletes it, and starts again from an empty one. */
  async reset(): Promise<Info> {
    await this.#db.closeAsync();
    await rm(this.path);
    this.#db = await Database.openAsync(this.path, { schema: sampleSchema });

    return this.info();
  }

  async close(): Promise<void> {
    if (this.#db.isOpen) {
      await this.#db.closeAsync();
    }
  }

  /**
   * The number each kind of sample object continues from: one past the
   * highest organization code, person and post in the file, so that a run
   * never repeats a code, a nickname or an email already there.
   */
  async #nextNumbers(): Promise<{ organization: number; person: number; post: number }> {
    const [organization, person, post] = await this.#db.readAsync((txn) =>
      Promise.all([
        txn
          .collection('organizations')
          .findOne('code STARTSWITH $0 SORT BY code DESC', [ORGANIZATION_PREFIX]),
        txn.collection('people').findOne('SORT BY id DESC'),
        txn.collection('posts').findOne('SORT BY id DESC')
      ])
    );
    const lastCode =
      organization === null
        ? 0
        : Number.parseInt(organization.code.slice(ORGANIZATION_PREFIX.length), 10);

    return {
      organization: (Number.isSafeInteger(lastCode) ? lastCode : 0) + 1,
      person: (person?.id ?? 0) + 1,
      post: (post?.id ?? 0) + 1
    };
  }
}
