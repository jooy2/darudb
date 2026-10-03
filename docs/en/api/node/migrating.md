---
title: Migrating
order: 12
---

# Migrating

`Migrating` is the write transaction a migration function runs in: the collections of the new schema, and the objects as the schema before the migration read them.

```ts
interface Migrating<S> extends WriteTransaction<S>
```

When `Database.open` finds the file at an older schema version, it migrates the file in one write transaction. It applies every step's renames and the changes the engine makes by itself, then calls the `run` function of each [Migration](../../types/node/migration.md) between the two versions, in version order, with a `Migrating`. Its `collection` gives the new schema's collections, as a [WriteTransaction](./write-transaction.md) does. If a function throws, nothing of the migration is kept, the file keeps its old schema and data, and `open` throws the same error. [Migrations](../../guide/migrations.md) explains what the engine changes by itself and what needs a function.

A migration function of `open` is synchronous: one that returns a promise is refused with `INVALID_ARGUMENT`, and the open fails. While the functions run, the migration holds the file's writer lock, so a write on the same file from another handle of the process fails with `INVALID_ARGUMENT`.

```ts
const app2 = schema(2, {
  teams: collection({ name: t.string().primaryKey(), city: t.string().optional() }),
  people: collection({
    fullName: t.string(),
    email: t.string().optional().unique(),
    age: t.string().default('')
  })
});

const db = Database.open('app.darudb', {
  schema: app2,
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

## Properties

### previousVersion

```ts
readonly previousVersion: number;
```

The schema version the file held before the migration. It is the same in every step.

### version

```ts
readonly version: number;
```

The version this step migrates to.

## Methods

### previous

```ts
previous(collection: string, key: Key): Record<string, unknown> | null;
```

The object of `collection` whose primary key is `key`, as the schema before the migration reads it, or `null`. `collection` and the object's fields have the names they had before the migration, and the fields the migration removed or replaced keep their values. A collection the migration deletes can still be read this way until it commits. A collection the old schema did not have fails with `INVALID_ARGUMENT`.

It reads the object as it is now, and writing an object keeps only the new schema's fields, so read an object this way before writing it. No declaration says which ints are `bigint`s, so an int reads as a number, or as a `bigint` beyond 2^53.

### previousKeys

```ts
previousKeys(collection: string): Key[];
```

The primary keys of every object of `collection`, named as before the migration, in key order.

## AsyncMigrating

```ts
interface AsyncMigrating<S> extends AsyncWriteTransaction<S> {
  readonly previousVersion: number;
  readonly version: number;
  previous(collection: string, key: Key): Promise<Record<string, unknown> | null>;
  previousKeys(collection: string): Promise<Key[]>;
}
```

The write transaction of a migration of `Database.openAsync`, whose functions, declared as an [AsyncMigration](../../types/node/migration.md), may be asynchronous. `previous` and `previousKeys` return promises, and `collection` gives an [AsyncWriteCollection](./write-collection.md#asyncwritecollection). Operations run in the order they were called, and a step ends once the function's promise has settled and every operation it called has too, awaited or not.

```ts
const db = await Database.openAsync('app.darudb', {
  schema: app2,
  migrations: [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      async run(m) {
        const people = m.collection('people');

        for (const key of await m.previousKeys('users')) {
          const before = await m.previous('users', key);

          await people.update(key, { age: `${before?.age} years` });
        }
      }
    }
  ]
});
```
