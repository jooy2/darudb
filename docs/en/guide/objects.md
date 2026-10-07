---
title: Collections and objects
order: 3
---

# Collections and objects

A database opened with a schema holds collections of typed objects, with indexes the engine keeps in step with them.

## Declare a schema

A schema has a version, from 1 up, and collections. Each collection has fields of a type, a primary key, and any indexes.

::: lang rust

```rust
use darudb::{Collection, OpenOptions, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;
    db.close()
}
```

The types are `Bool`, `Int` (64-bit), `Float` (64-bit), `String`, `Bytes`, a link to another collection's object (`Type::link`), a list of any of those (`Type::list`), and an embedded object with fields of its own (`Type::object(Embedded::new().field(...))`).

- **Required and optional fields.** `field` is required, and writing an object without it fails. `optional` may be null, which is what it holds when it is left out. `with_default` is required, and an object written without it gets the default.
- **Primary keys.** `primary_key` names a field of type `Int`, `String` or `Bytes`.
- **Indexes.** `index` keeps an index on a field, and `unique` an index that also refuses two objects with the same value.

:::

::: lang node

`t` has the field types, `collection` groups fields, and `schema` gives the collections a version. The TypeScript type of every object follows from the declaration.

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

The types are `t.bool()`, `t.int()`, `t.bigint()`, `t.float()`, `t.string()`, `t.bytes()`, `t.link(collection)`, `t.list(type)` and `t.object(fields)`.

- **Required and optional fields.** A field is required unless it says otherwise, and writing an object without it fails. `optional()` lets it be null, which is what it holds when it is left out. `default(value)` keeps it required and fills it in when it is left out.
- **Primary keys.** `primaryKey()` makes an `int`, `bigint`, `string` or `bytes` field the key.
- **Indexes.** `index()` keeps an index on a field, and `unique()` an index that also refuses two objects with the same value.
- **Numbers.** A `t.int()` field holds a number. A value beyond 2^53, which a number does not hold exactly, is refused when written and fails when read; declare such a field with `t.bigint()`, which always reads as a `bigint`. Bytes are a `Uint8Array`.

:::

::: lang dart

Each collection is a class annotated `@Collection()`, with `final` fields and a constructor that takes each of them. `dart run build_runner build` writes, into the library's `.g.dart` part, a schema constant named after the class, `userSchema` for `User`, with the code that reads and writes its objects, a query builder, and a `copyWith`.

```dart
import 'package:darudb/darudb.dart';

part 'models.g.dart';

@Collection('teams')
class Team {
  const Team({required this.name, this.city});

  @PrimaryKey()
  final String name;
  final String? city;
}

@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  final int? zip;
}

@Collection('users')
class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.tags,
    this.team,
    this.address,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  @Index()
  final List<String>? tags;
  final Link<Team>? team;
  final Address? address;
}

final db = Database.open(
  'app.darudb',
  schema: const Schema(1, [teamSchema, userSchema]),
);
```

A field's Dart type is its type: `bool`, `int` (64-bit), `double`, `String`, `Uint8List` for bytes, a `List` of those or of links, a `Link<T>` to an object of another collection, and a class annotated `@Embedded()` for an embedded object.

- **Required and optional fields.** A field whose type is not nullable is required, and writing an object without it fails. A nullable one is optional, and null when it is left out. A constructor parameter's default is the field's default: the field stays required and holds the default when a record leaves it out.
- **Primary keys.** `@PrimaryKey()` makes an `int`, `String` or `Uint8List` field the key. Without one, the class has a field `final int? id`, which is `null` until the object is inserted.
- **Indexes.** `@Index()` keeps an index on a field, and `@Unique()` an index that also refuses two objects with the same value. `@Name('...')` gives a field another name in the file than in Dart.
- **Objects are values.** The fields are final, and a changed object is a copy, made with the generated `copyWith`, written back with `put`.

:::

::: lang python

Each collection is a class decorated with `@darudb.collection`, which makes it a frozen, keyword-only dataclass. Its annotations are the types of its fields, and `field` adds what an annotation cannot say: a default, an index, a primary key, a link or a stored name.

```python
import darudb
from darudb import field


@darudb.collection("teams")
class Team:
    name: str = field(primary_key=True)
    city: str | None = None


@darudb.embedded
class Address:
    city: str
    zip: int | None = None


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    tags: list[str] | None = field(default=None, index=True)
    team: str | None = field(default=None, link=Team)
    address: Address | None = None


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [Team, User]))
```

A field's annotation is its type: `bool`, `int` (64-bit), `float`, `str`, `bytes`, a `list` of those or of links, and a class decorated with `@darudb.embedded` for an embedded object. The classes are read when the `Schema` is made, so an annotation may name a class declared after it.

- **Required and optional fields.** A field without a default whose type is not `X | None` is required, and writing an object without it fails. An `X | None` field is optional, `None` when it is left out, and takes no default but `None`. A default, given as `= value`, `field(default=...)` or `field(default_factory=...)`, keeps the field required and fills it in when it is left out, in an object you make and in one the file holds from before the field existed.
- **Primary keys.** `field(primary_key=True)` makes an `int`, `str` or `bytes` field the key, which is required and has no default. Without one, the class has the field `id: int | None = None`, which is `None` until the object is inserted.
- **Links.** `field(link=Team)` makes an `int`, `str` or `bytes` field, or a list of them, hold primary keys of another collection, named by its class or by its name.
- **Indexes.** `field(index=True)` keeps an index on a field, and `field(unique=True)` an index that also refuses two objects with the same value. `field(name="...")` gives a field another name in the file than in Python. An embedded object's fields have no index.
- **Objects are values.** The class is frozen, and a changed object is a copy, made with `dataclasses.replace`, written back with `put`. An object read from the file is made without calling the class's `__init__`, so a `__post_init__` does not run for it.
- **Values.** A `float` field takes an `int` too, but an `int` field refuses a `float` and a `bool`, and an `int` beyond 64 bits. Bytes go in as `bytes`, `bytearray` or `memoryview`, and come out as `bytes`. A list holds no `None`, and a list of lists or of embedded objects is refused.

:::

The rules the engine keeps are the same in every language:

- **The automatic key.** A collection that names no primary key gets an integer field called `id`, and an object written without an `id` gets the next number, from 1 up. A number is never given twice in one file, even after its object is deleted.
- **Links** hold the primary key of an object in the linked collection. A link to an object that does not exist is allowed, and reads as the key it holds.
- **Indexes** let a query on the field read only the objects it finds. Any number of objects can hold null in a unique field, and an index on a list has an entry for each element.

The first open stores the schema in the file. Every later open compares the declared schema with the stored one: the same version with a different schema fails with `SCHEMA_MISMATCH`, and a file holding a newer version fails with `SCHEMA_TOO_NEW`. Declaring collections or indexes in another order is not a change. To change the schema, raise its version: see [Migrations](./migrations.md).

## Read and write objects

::: lang rust

An object is a set of named values. Inside a write transaction, `collection` gives a collection's objects and the calls that change them.

```rust
use darudb::{Database, Object, Value};

fn write(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;

    let alice = users.insert(Object::new().with("name", "Alice").with("email", "alice@example.com"))?;
    users.insert(Object::new().with("name", "Bob"))?;

    // `put` replaces the object with the same key.
    users.put(Object::new().with("id", alice.clone()).with("name", "Alice").with("age", 31))?;
    // `update` sets the fields it is given and keeps the rest.
    users.update(alice.clone(), Object::new().with("age", 32).with("email", Value::Null))?;

    let mut posts = txn.collection("posts")?;
    posts.insert(
        Object::new()
            .with("slug", "hello")
            .with("author", alice)
            .with("tags", vec![Value::from("intro")]),
    )?;

    txn.commit()
}

fn read(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    if let Some(user) = users.get(1)? {
        println!("{:?}", user.get("name"));
    }

    for user in users.iter()? {
        println!("{:?}", user?);
    }

    println!("{} users", users.len()?);
    Ok(())
}
```

- `insert` returns the new object's key. `put` inserts or replaces. `delete` takes a key and returns whether there was an object.
- `update` takes a key and the fields to change, and returns whether there was an object; it inserts nothing when there is none. Null makes an optional field null and gives a field with a default its default.

:::

::: lang node

`write` runs a function in a write transaction, and `read` in a read transaction. Inside, `collection` gives a collection's objects and the calls that change them.

```ts
db.write((txn) => {
  txn.collection('teams').insert({ name: 'north', city: 'Seoul' });

  const users = txn.collection('users');

  users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31, team: 'north' },
    { name: 'Bob', tags: ['new'] }
  ]);
  users.put({ id: 2, name: 'Robert', age: 18 });
  users.update(1, { age: 32, email: null });
  users.delete(3);
});

const alice = db.read((txn) => txn.collection('users').get(1));
```

- `insert` and `insertMany` return the keys. `put` and `putMany` insert or replace. `delete` says whether there was an object.
- `update` sets the fields it is given, keeps the rest, and says whether there was an object. `null` makes an optional field null and gives a field with a default its default, and a field left `undefined` stays as it is.
- A batch crosses into the engine as one buffer in one call, which is much cheaper than one call per object.

:::

::: lang dart

`txn.collection(userSchema)` gives a collection's objects as the class, and the calls that change them.

```dart
db.write((txn) {
  txn.collection(teamSchema).insert(const Team(name: 'north', city: 'Seoul'));

  final users = txn.collection(userSchema);
  final alice = users.insert(
    const User(name: 'Alice', email: 'alice@example.com', age: 31, team: Link<Team>('north')),
  );

  users.insertMany(const [User(name: 'Bob', tags: ['new'])]);
  // `put` replaces the object with the same key.
  users.put(users.get(alice)!.copyWith(age: 32));
  // `update` sets the fields it is given and keeps the rest.
  users.update(alice, (q) => [q.email.set(null)]);
  users.delete(2);
});

final alice = db.read((txn) => txn.collection(userSchema).get(1));
```

- `insert` returns the new object's key, and `insertMany` the keys of a batch, which crosses into the engine as one buffer in one call. `put` and `putMany` insert or replace. `delete` says whether there was an object.
- `update` takes a key and a function that gives the changes, each made by a field's `set`, and says whether there was an object. `set(null)` makes an optional field null and gives a field with a default its default.
- `copyWith` keeps every field it is not given. It cannot make a field null: construct the object for that.

:::

::: lang python

`txn.collection(User)` gives a collection's objects as instances of the class, and the calls that change them. `txn.collection("users")` reaches the same collection by its name.

```python
with db.write() as txn:
    txn.collection(Team).insert(Team(name="north", city="Seoul"))

    users = txn.collection(User)
    alice = users.insert(User(name="Alice", email="alice@example.com", age=31, team="north"))

    users.insert_many([User(name="Bob", tags=["new"])])
    # `put` replaces the object with the same key.
    users.put(User(id=alice, name="Alice", age=32, team="north"))
    # `update` sets the fields it is given and keeps the rest.
    users.update(alice, email=None, tags=["admin"])
    users.delete(2)

with db.read() as txn:
    alice = txn.collection(User).get(1)
```

- `insert` returns the new object's key, and `insert_many` the keys of a batch, which crosses into the engine in one call. `put` and `put_many` insert or replace. `delete` says whether there was an object.
- `update(key, **changes)` sets the fields it names, by their Python names, keeps the rest, and says whether there was an object. `None` makes an optional field `None` and gives a field with a default its default.
- A batch stops at the first object the engine refuses, with its error, and the objects before it stay inserted in the transaction. An object the package cannot convert, such as one of another class, refuses the whole batch before any of it is written.

:::

These hold in every language:

- `insert` fails with `DUPLICATE_KEY` if the key is taken, or if a unique index finds one of the object's values taken.
- An `update` replaces an embedded object or a list whole, and changing the primary key fails with `INVALID_ARGUMENT`. It costs less than reading the object and putting it back, since the engine changes the record where it lies.
- An object that does not fit the schema, with a value of the wrong type, a field the schema does not have, or a required field missing, fails with `INVALID_ARGUMENT`.
- A refused write changes nothing, and the transaction can go on and commit.
- Objects read back are plain values that outlive the transaction. Every field of the schema is there: a left-out field holds its default, or null.

::: lang rust

## Objects as Rust types

With the crate's `derive` feature, `#[derive(Object)]` makes a struct the objects of a collection. The schema declares the collection from the struct, and `collection_of` reads records straight into it and writes it as records, without the `Object` of named values in between, which costs about as much as finding the record.

```rust
use darudb::{Collection, Database, Filter, Object, OpenOptions, Query, Schema};

#[derive(Object, Debug, Clone)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
}

fn open() -> darudb::Result<Database> {
    OpenOptions::new()
        .schema(Schema::new(1).collection(Collection::of::<User>()))
        .open("app.darudb")
}

fn write_and_read(db: &Database) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection_of::<User>()?;
    let id = users.insert(&User { id: None, name: "Alice".to_owned(), email: None, age: 31 })?;

    drop(users);
    txn.commit()?;

    let read = db.begin_read()?;
    let users = read.collection_of::<User>()?;
    let alice: Option<User> = users.get(id)?;
    let adults: Vec<User> = users.query(&Query::new().filter(Filter::ge("age", 18)))?;

    println!("{alice:?} {adults:?}");
    Ok(())
}
```

An `Option` field is optional, `Vec<T>` a list, `Link<T>` a link, and a struct with `#[derive(Embedded)]` an embedded object. Without a field marked `#[darudb(key)]`, the struct needs `id: Option<i64>`, which is `None` until the object is inserted. [Derive macros](../api/rust/derive.md) lists the attributes. A typed and an untyped handle read and write the same objects, so the two APIs mix freely.

:::

## Several handles and processes

Each handle keeps the schema it was opened with. When another process, or another handle in the same process, migrates the file, the next transaction to reach a collection through the old handle fails with `SCHEMA_MISMATCH`, and the handle has to be opened again with the new schema. A read transaction that began before the migration goes on reading under the old schema, since it sees the commit it began at.
