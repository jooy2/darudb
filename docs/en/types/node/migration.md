---
title: Migration
order: 4
group: database
counterpart: /api/rust/migration
pageClass: reference-page
---

# Migration

`Migration` says what schema version `version` changes from the version before it, beyond what the engine works out by itself.

```ts
interface Migration<S = Schema>
```

The `migrations` field of [OpenOptions](./open-options.md#migrations) takes an array of them. Opening a file that holds an older schema version runs every version step up to the declared one, in version order, in one write transaction. If any part fails, the file keeps its old schema and data. `S` is the type of the schema `Database.open` was given, which reaches `run` as the collections' types.

The engine makes some changes by itself: it creates new collections, adds new fields that are optional or have a default, builds new indexes and drops removed ones, and retires removed fields. A migration names the rest: a renamed collection or field, a deleted collection, and a field whose type changed. A collection that is gone or a field whose type changed, left unnamed, fails with `INVALID_ARGUMENT`, and so does a name the schema before the step does not have. A field renamed without `renameFields` is taken as a removed field and a new one, so its values do not follow it to the new name. Every name in a step is the name before that step, so a field of a collection the step renames is named under the collection's old name. [Migrations](../../guide/migrations.md) walks through a whole migration.

This migration takes a file from version 1, where `users` held `name` and an int `age`, to version 2:

```ts
import { collection, Database, schema, t } from 'darudb';

const v2 = schema(2, {
  people: collection({ fullName: t.string(), age: t.string().default('') })
});

const db = Database.open('app.darudb', {
  schema: v2,
  migrations: [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      run(m) {
        const people = m.collection('people');

        for (const key of m.previousKeys('users')) {
          const before = m.previous('users', key);
          const person = people.get(key);

          if (before !== null && person !== null) {
            people.put({ ...person, age: `${before.age} years` });
          }
        }
      }
    }
  ]
});
```

## Fields

| Field | Type | What it says |
| --- | --- | --- |
| [`version`](#version) | `number` | The schema version this step migrates to, from 2 up |
| [`renameCollections`](#renamecollections) | `[from, to][]` | Collections renamed, with their objects where they are |
| [`renameFields`](#renamefields) | `[collection, from, to][]` | Fields renamed, without rewriting a record |
| [`deleteCollections`](#deletecollections) | `string[]` | Collections that go, with their objects, after `run` |
| [`replaceFields`](#replacefields) | `[collection, field][]` | Fields replaced by a new field of the same name |
| [`run`](#run) | `(migrating) => void` | Your function, in the migration's write transaction |

Only `version` is required.

### version

```ts
version: number;
```

The schema version this step migrates to: a whole number from 2 up to the declared schema's version. Any other number, or two migrations to the same version, fails with `INVALID_ARGUMENT`.

### renameCollections

```ts
renameCollections?: [from: string, to: string][];
```

Pairs of a collection's old name and its new one. The collection keeps its objects where they are, so a rename copies nothing. A new name that another collection has fails with `INVALID_ARGUMENT`.

### renameFields

```ts
renameFields?: [collection: string, from: string, to: string][];
```

The collection, by its name before the step, then the field's old name and its new one. No object is rewritten, since a record holds a field's id rather than its name. A new name that the collection already has fails with `INVALID_ARGUMENT`.

### deleteCollections

```ts
deleteCollections?: string[];
```

Collections that go, with every object in them. A collection the new schema leaves out has to be named here. The objects go last, after `run`, which can still read them with `previous`.

### replaceFields

```ts
replaceFields?: [collection: string, field: string][];
```

Fields replaced by a new field of the same name, as when a field's type changes. A replaced field is a removed field and a new one, so a new field that is required needs a default. The old values stay readable in `run` through `previous`. A primary key cannot be replaced, and naming one fails with `INVALID_ARGUMENT`.

### run

```ts
run?(migrating: Migrating<S>): void;
```

A function that runs in the migration's write transaction, after the renames, once the new schema is in place and its indexes are built. [Migrating](../../api/node/migrating.md) gives it the collections of the new schema, and the objects as the old schema read them through `previous` and `previousKeys`. Read an object that way before writing it: a written object keeps only the new schema's fields. One function runs for each version step that has one, in version order.

- The function has to be synchronous. One that returns a promise fails with `INVALID_ARGUMENT`.
- If it throws, the migration is abandoned, the file keeps its old schema and data, and `open` throws the same error.
- The migration holds the file's writer lock while the function runs, so a write to the same file from inside it, through any `Database`, fails with `INVALID_ARGUMENT` rather than waiting for itself.

## AsyncMigration

```ts
interface AsyncMigration<S = Schema> extends Omit<Migration<S>, 'run'> {
  run?(migrating: AsyncMigrating<S>): Promise<void> | void;
}
```

The migrations of `Database.openAsync`. They have the fields of `Migration`, but `run` may be asynchronous, and it receives an [AsyncMigrating](../../api/node/migrating.md#asyncmigrating), whose collections, `previous` and `previousKeys` return promises. The step ends once what `run` returns has settled and every operation it called has too, so an operation it did not await still belongs to the step. If it rejects, `openAsync` rejects with the same error and the file keeps its old schema and data.
