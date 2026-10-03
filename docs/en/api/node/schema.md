---
title: schema
order: 2
---

# schema

`schema` declares a database's collections at a version, and returns the `Schema` that `Database.open` takes.

```ts
const schema: <C extends Record<string, Collection<any>>>(
  version: number,
  collections: C
) => Schema<C>;
```

`version` is a whole number from 1 up; raise it whenever the schema changes. `collections` gives each [collection](./collection.md) under the name it has in the file. The TypeScript types follow from the declaration, so a `Database` opened with the schema knows every collection's name and the type of its objects.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  teams: collection({
    name: t.string().primaryKey(),
    city: t.string().optional()
  }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index(),
    tags: t.list(t.string()).optional().index(),
    team: t.link('teams').optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
  })
});

const db = Database.open('app.darudb', { schema: app });
```

The examples on the other pages of this section use this `db`.

`schema` throws `INVALID_ARGUMENT` at once for a version that is not a whole number from 1 up, or for a collection that `collection` did not make. The rest is checked when the database opens, which throws `INVALID_ARGUMENT` for a schema the engine cannot store, such as a link to a collection the schema does not have or a collection with two primary keys.

The first open stores the schema in the file, and every later open compares the declared schema with the stored one:

- **The same version and the same schema**: nothing to do. Declaring collections or indexes in another order is not a change.
- **The same version and another schema**: `SCHEMA_MISMATCH`, since the schema changed without a new version.
- **A newer version in the file**: `SCHEMA_TOO_NEW`, since a newer application wrote it.
- **An older version in the file**: a migration, which [Migrations](../../guide/migrations.md) explains.

The returned `Schema` is frozen, and any number of opens can share it:

```ts
interface Schema<C extends Record<string, Collection<any>> = Record<string, Collection>>
```

## Properties

### version

```ts
readonly version: number;
```

The schema version.

### collections

```ts
readonly collections: C;
```

The collections, by name, as they were given.
