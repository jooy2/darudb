/**
 * What the screens know of each collection: its key, its fields and what
 * kind of value each holds, which columns the list shows, and which fields
 * it can sort by. It mirrors `schema.ts`, which is the one the engine reads.
 *
 * The module has no Node.js imports, so the renderer and the browser page
 * import it as they are, and it is also what `store.ts` converts objects by
 * on their way to the screens and back.
 */

export const COLLECTION_NAMES = ['organizations', 'people', 'posts'] as const;

export type CollectionName = (typeof COLLECTION_NAMES)[number];

/**
 * How a field's value is shown and edited. `tags` is a list of strings,
 * `location` the embedded object of `people`, and `color` three bytes.
 */
export type FieldKind =
  'string' | 'int' | 'float' | 'bool' | 'date' | 'color' | 'link' | 'tags' | 'location';

export interface FieldInfo {
  readonly name: string;
  readonly kind: FieldKind;
  /** Whether the field may be null. */
  readonly optional: boolean;
  /** Whether the list shows it as a column. */
  readonly column: boolean;
  /** The collection a link holds a key of. */
  readonly target?: CollectionName;
}

export interface CollectionInfo {
  readonly name: CollectionName;
  /** The field that holds the primary key. */
  readonly key: string;
  /** Whether the engine assigns the key, as it does for a collection without a key field. */
  readonly autoKey: boolean;
  /** Every field but an automatic key. */
  readonly fields: readonly FieldInfo[];
  /** The fields the list can sort by: the key and the indexed fields. */
  readonly sortable: readonly string[];
}

const field = (
  name: string,
  kind: FieldKind,
  options: { optional?: boolean; column?: boolean; target?: CollectionName } = {}
): FieldInfo => ({
  name,
  kind,
  optional: options.optional ?? false,
  column: options.column ?? true,
  target: options.target
});

export const COLLECTIONS: Readonly<Record<CollectionName, CollectionInfo>> = {
  organizations: {
    name: 'organizations',
    key: 'code',
    autoKey: false,
    fields: [
      field('code', 'string'),
      field('name', 'string'),
      field('kind', 'string'),
      field('industry', 'string', { optional: true }),
      field('language', 'string'),
      field('founded', 'int')
    ],
    sortable: ['code', 'name', 'kind', 'founded']
  },
  people: {
    name: 'people',
    key: 'id',
    autoKey: true,
    fields: [
      field('color', 'color'),
      field('name', 'string'),
      field('nickname', 'string'),
      field('email', 'string', { optional: true }),
      field('age', 'int'),
      field('gender', 'string', { column: false }),
      field('language', 'string'),
      field('location', 'location', { optional: true }),
      field('organization', 'link', { optional: true, target: 'organizations' }),
      field('tags', 'tags'),
      field('active', 'bool'),
      field('score', 'float', { column: false }),
      field('joinedAt', 'date', { column: false })
    ],
    sortable: ['id', 'name', 'nickname', 'email', 'age', 'language', 'organization', 'joinedAt']
  },
  posts: {
    name: 'posts',
    key: 'id',
    autoKey: true,
    fields: [
      field('author', 'link', { target: 'people' }),
      field('title', 'string'),
      field('body', 'string', { column: false }),
      field('language', 'string'),
      field('tags', 'tags', { optional: true }),
      field('likes', 'int'),
      field('pinned', 'bool'),
      field('createdAt', 'date')
    ],
    sortable: ['id', 'author', 'language', 'likes', 'createdAt']
  }
};

export const isCollectionName = (value: unknown): value is CollectionName =>
  typeof value === 'string' && (COLLECTION_NAMES as readonly string[]).includes(value);
