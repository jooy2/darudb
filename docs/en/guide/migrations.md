---
title: Migrations
order: 8
---

# Migrations

Changing a schema means raising its version, and opening a file that holds an older version migrates it in one write transaction that either commits whole or leaves the file as it was.

<Diagram name="migrations" alt="Opening with schema version n compares it with the stored schema. With nothing stored, the schema is stored; the same version and schema need nothing; the same version with another schema fails with SCHEMA_MISMATCH; a newer version fails with SCHEMA_TOO_NEW; an older version m is migrated in one write transaction that renames, adds the new collections, fields and indexes, runs your function for each version step from m + 1 to n, and stores version n. If any step fails, the file is left as it was." />

## What the engine does by itself

The engine makes some changes without being told: a new collection, a new optional field or one with a default, a removed field, and a new or removed index. Records are not rewritten. An object written before a field existed reads its default, which is why a required field keeps its default once it has one.

Anything else is named in a migration for the version that makes it.

## Name the other changes

::: lang rust

```rust
use darudb::{Collection, Migration, OpenOptions, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let v2 = Schema::new(2).collection(
        Collection::new("people")
            .field("full_name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::String, "")
            .unique("email"),
    );
    let migration = Migration::to(2)
        .rename_collection("users", "people")
        .rename_field("users", "name", "full_name")
        .replace_field("users", "age")
        .delete_collection("posts")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let before = migrating.previous("users", key.clone())?;
                let age = before.and_then(|user| user.get("age")?.as_int()).unwrap_or(0);
                let mut people = migrating.collection("people")?;

                if let Some(mut person) = people.get(key)? {
                    person.set("age", format!("{age} years"));
                    people.put(person)?;
                }
            }

            Ok(())
        });

    let db = OpenOptions::new().schema(v2).migration(migration).open("app.darudb")?;
    db.close()
}
```

A function that fails returns its error, and the open fails with it. `Error::MigrationFailed` carries the application's own reason.

:::

::: lang node

```ts
const app2 = schema(2, {
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
      deleteCollections: ['teams'],
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

If the function throws, `open` throws the same error. With `Database.openAsync`, the function may be asynchronous, and `previous` and `previousKeys` return promises.

:::

::: lang dart

```dart
@Collection('people')
class Person {
  const Person({this.id, required this.fullName, this.email, this.age = ''});

  final int? id;
  @Name('full_name')
  final String fullName;
  @Unique()
  final String? email;
  final String age;
}

final db = Database.open(
  'app.darudb',
  schema: const Schema(2, [personSchema]),
  migrations: [
    Migration(
      2,
      renameCollections: const {'users': 'people'},
      renameFields: const {
        'users': {'name': 'full_name'},
      },
      replaceFields: const {
        'users': ['age'],
      },
      deleteCollections: const ['teams'],
      run: (m) {
        final people = m.collection(personSchema);

        for (final key in m.previousKeys('users')) {
          final before = m.previous('users', key);
          final person = people.get(key as int);

          if (before != null && person != null) {
            people.put(person.copyWith(age: '${before['age']} years'));
          }
        }
      },
    ),
  ],
);
```

`previous` gives an object as the old schema read it, as a `Map` of its fields by their old names, since the class of the old schema is usually gone from the program. If the function throws, `open` throws the same error. With `Database.openAsync`, the function may be asynchronous; the calls it makes on the context stay synchronous.

:::

::: lang python

```python
import darudb
from darudb import Migration, field


@darudb.collection("people")
class Person:
    id: int | None = None
    full_name: str
    email: str | None = field(default=None, unique=True)
    age: str = ""


def to_v2(m: darudb.Migrating) -> None:
    people = m.collection(Person)

    for key in m.previous_keys("users"):
        before = m.previous("users", key)

        if before is not None:
            people.update(key, age=f"{before['age']} years")


db = darudb.Database.open(
    "app.darudb",
    schema=darudb.Schema(2, [Person]),
    migrations=[
        Migration(
            2,
            rename_collections=[("users", "people")],
            rename_fields=[("users", "name", "full_name")],
            replace_fields=[("users", "age")],
            delete_collections=["teams"],
            run=to_v2,
        )
    ],
)
```

`previous` gives an object as the old schema read it, as a `dict` of its fields by their old names, since the class of the old schema is usually gone from the program. If `run` raises, `open` raises the same exception. With `Database.open_async`, `run` may be a coroutine function, given an `AsyncMigrating` whose calls are awaited.

:::

- **Renames** keep the data where it is, so they cost nothing however many objects there are. A rename names the collection by its name before the migration.
- **A replaced field** is a new field with the old name, for a change of type. **A deleted collection** goes with its objects and indexes.
- **The migration function** runs in the migration's write transaction, after the renames, under the new schema. `previous` reads an object as the old schema did, with the old names and the values of removed and replaced fields, so read an object that way before writing it: a written object keeps only the new schema's fields. A deleted collection can still be read that way until the migration commits.
- **A failed migration** leaves the file with its old schema and its data.

## Several versions at once

Migrations to several versions run in version order. A file two versions behind runs both steps, and one already at the declared version runs none. Give the migration of every version the application has had, so that a file from any of them can be opened.

A file whose schema version is newer than the declared one fails with `SCHEMA_TOO_NEW`: an application cannot open a file a newer release of itself wrote.
