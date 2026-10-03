---
title: Realm
order: 3
---

# Migrating from Realm

This page moves the data of a Realm file into DaruDB: how object schemas and property types map onto collections and fields, a Node.js script that copies every object, and what changes in an application that used live objects.

## How the two map

| Realm | DaruDB |
| --- | --- |
| An object schema, or class | A collection |
| An object | An object |
| `primaryKey` | A primary key, of type integer, string or bytes |
| `indexed: true` | An index |
| `bool`, `int`, `float` or `double`, `string`, `data` | A boolean, integer, float, string or bytes field |
| `date` | An integer field of milliseconds since the epoch, or ISO 8601 text |
| `objectId` | Its 24 hexadecimal characters as a string, or its 12 bytes |
| `uuid`, `decimal128` | A string field: DaruDB has neither type |
| A link to an object | A link, which holds the other object's primary key |
| A list | A list of values or of links |
| A set | A list, whose values the application keeps distinct |
| A dictionary | No counterpart: an embedded object when its keys are fixed, or JSON text |
| `mixed` | No counterpart: a field for each type it holds, or JSON text |
| An embedded object | An embedded object |
| A list of embedded objects | No counterpart in this version: a collection of its own, linking back to the parent |
| `linkingObjects`, a backlink | Nothing stored: a query on the link field, which an index makes cheap |

- **A link needs a key.** A link holds the primary key of the object it names, so a class that other objects link to needs a primary key. Give one a key with a Realm migration before copying, or copy it as an embedded object when each of its objects belongs to one parent.
- **Defaults are not in the file.** A Realm file records its classes and properties but not their default values, so declare the DaruDB defaults by hand.
- **Large integers.** The JavaScript SDK reads every `int` as a number, so a value beyond 2^53 has lost precision before the script sees it.

## Declare the schema

The example moves this Realm schema: users with an embedded address, and posts that link to them.

```js
const Address = { name: 'Address', embedded: true, properties: { city: 'string', zip: 'string?' } };

const User = {
  name: 'User',
  primaryKey: '_id',
  properties: {
    _id: 'objectId',
    name: 'string',
    email: { type: 'string', optional: true, indexed: true },
    age: { type: 'int', default: 0, indexed: true },
    score: 'double?',
    avatar: 'data?',
    joined: 'date',
    tags: 'string[]',
    address: 'Address?',
    posts: { type: 'linkingObjects', objectType: 'Post', property: 'author' }
  }
};

const Post = {
  name: 'Post',
  primaryKey: 'slug',
  properties: {
    slug: 'string',
    author: 'User?',
    readers: 'User[]',
    price: 'decimal128?',
    ref: 'uuid?',
    meta: 'string{}'
  }
};
```

`_id` becomes a string key named `id`, the date a number of milliseconds, the `decimal128` and the `uuid` text, and the dictionary JSON text. The backlink `posts` is not copied: an index on `author` makes the posts of a user one query.

The copy runs in Node.js, through the `realm` package, so the DaruDB schema is declared in TypeScript for it:

```ts
import { collection, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    id: t.string().primaryKey(),
    name: t.string(),
    email: t.string().optional().index(),
    age: t.int().default(0).index(),
    score: t.float().optional(),
    avatar: t.bytes().optional(),
    joined: t.int(),
    tags: t.list(t.string()),
    address: t.object({ city: t.string(), zip: t.string().optional() }).optional()
  }),
  posts: collection({
    slug: t.string().primaryKey(),
    author: t.link('users').optional().index(),
    readers: t.list(t.link('users')),
    price: t.string().optional(),
    ref: t.string().optional(),
    meta: t.string()
  })
});
```

::: lang rust

A Rust program opens the copied file with the same declaration in Rust. The two have to agree, in version, collections, fields, types and indexes, or opening fails with `SCHEMA_MISMATCH`:

```rust
use darudb::{Collection, Embedded, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .primary_key("id", Type::String)
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .optional("score", Type::Float)
                .optional("avatar", Type::Bytes)
                .field("joined", Type::Int)
                .field("tags", Type::list(Type::String))
                .optional(
                    "address",
                    Type::object(Embedded::new().field("city", Type::String).optional("zip", Type::String)),
                )
                .index("email")
                .index("age"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .optional("author", Type::link("users"))
                .field("readers", Type::list(Type::link("users")))
                .optional("price", Type::String)
                .optional("ref", Type::String)
                .field("meta", Type::String)
                .index("author"),
        )
}
```

:::

::: lang dart

A Dart program opens the copied file with the same declaration as annotated classes. The two have to agree, in version, collections, fields, types and indexes, or opening fails with `SCHEMA_MISMATCH`:

```dart
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'schema.g.dart';

@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  final String? zip;
}

@Collection('users')
class User {
  const User({
    required this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.score,
    this.avatar,
    required this.joined,
    required this.tags,
    this.address,
  });

  @PrimaryKey()
  final String id;
  final String name;
  @Index()
  final String? email;
  @Index()
  final int age;
  final double? score;
  final Uint8List? avatar;
  final int joined;
  final List<String> tags;
  final Address? address;
}

@Collection('posts')
class Post {
  const Post({
    required this.slug,
    this.author,
    required this.readers,
    this.price,
    this.ref,
    required this.meta,
  });

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
  final List<Link<User>> readers;
  final String? price;
  final String? ref;
  final String meta;
}

const app = Schema(1, [userSchema, postSchema]);
```

:::

## Copy the objects

The script reads the Realm file through the `realm` package and writes the DaruDB file through the Node.js package, whatever language the application is in: a DaruDB file is the same file in every language.

::: lang rust

Realm has no Rust SDK. Run the script below once with the Node.js package, then open the file it writes from Rust with `OpenOptions::new().schema(schema()).open("app.darudb")`.

:::

::: lang dart

Realm's Flutter SDK reads the file from Dart too, through the app's own Realm model classes, and the copy can be written the same way in Dart, a class at a time. The script below needs neither: run it once with the Node.js package, then open the file it writes from Dart with `Database.open('app.darudb', schema: app)`.

:::

```js
import Realm from 'realm';
import { Database } from 'darudb';

const realm = await Realm.open({ path: 'app.realm', readOnly: true });
const db = Database.open('app.darudb', { schema: app });

/** Copies `objects` into collection `name`, a thousand to a transaction. */
function copy(objects, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for (const object of objects) {
    batch.push(convert(object));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

copy(realm.objects('User'), 'users', (user) => ({
  id: user._id.toHexString(),
  name: user.name,
  email: user.email,
  age: user.age,
  score: user.score,
  avatar: user.avatar === null ? null : new Uint8Array(user.avatar),
  joined: user.joined.getTime(),
  tags: [...user.tags],
  address: user.address === null ? null : { city: user.address.city, zip: user.address.zip }
}));

copy(realm.objects('Post'), 'posts', (post) => ({
  slug: post.slug,
  author: post.author === null ? null : post.author._id.toHexString(),
  readers: [...post.readers].map((user) => user._id.toHexString()),
  price: post.price === null ? null : post.price.toString(),
  ref: post.ref === null ? null : post.ref.toString(),
  meta: JSON.stringify(post.meta)
}));

// Every deferred commit is durable once this returns.
db.close();
realm.close();

// The `realm` package keeps Node.js running after `close`.
process.exit(0);
```

- **Read-only, with the file's own schema.** Opened with `readOnly` and no schema, the file is read as it is and stays as it was. An encrypted file also needs its `encryptionKey`.
- **Classes, not embedded objects.** `realm.objects` takes a class; an embedded object is copied with its parent, as `address` is here.
- **One class at a time.** A link holds the key whatever has been copied so far, so the classes can go in any order.

## Check the copy

Compare `realm.objects('User').length` with <LangCode rust="len" node="count" dart="count" /> on `users`, for every class, and run the [integrity check](../guide/tools.md#check-a-file) on the new file.

## Queries

The query language is close to the Realm Query Language that `filtered` takes:

| Realm Query Language                     | DaruDB query language                     |
| ---------------------------------------- | ----------------------------------------- |
| `age >= $0`                              | `age >= $0`                               |
| `email == nil`                           | `email IS NULL`                           |
| `age BETWEEN {18, 30}`                   | `age BETWEEN 18 AND 30`                   |
| `age IN {18, 30}`                        | `age IN [18, 30]`                         |
| `name BEGINSWITH 'A'`                    | `name STARTSWITH "A"`                     |
| `name CONTAINS 'li'`, `ENDSWITH`         | `name CONTAINS "li"`, `ENDSWITH`          |
| `ANY tags == 'red'`                      | `tags == "red"`, or `tags CONTAINS "red"` |
| `author.name == 'Alice'`                 | `author.name == "Alice"`                  |
| `TRUEPREDICATE SORT(age DESC) LIMIT(10)` | `SORT BY age DESC LIMIT 10`               |

- There is no case-insensitive comparison, the `[c]` of `CONTAINS[c]`: keep a lowercased copy of the field and query that. `LIKE` with wildcards has no counterpart either.
- Aggregates such as `@count`, `@sum` and `@avg` inside a query have no counterpart; `count` counts the objects a query finds.
- A backlink becomes a query on the link field: the posts of a user are `author == $0` on `posts`, with the user's key.

[Queries](../guide/queries.md) has the whole language, and the builder.

## What changes in the application

- **Results are plain objects.** A query returns objects copied out of the file, not live objects tied to it: they do not change when the data does, and they stay usable after the transaction and the database are closed. A change is written back with `put` or `update`.
- **No change notifications.** Nothing calls the application when data changes. After a write, read again what the screen shows.
- **Transactions** are <LangCode rust="begin_write and commit" node="db.write" dart="db.write" /> instead of `realm.write`, and reads happen in a read transaction too. See [Transactions](../guide/transactions.md).
- **Schema versions** keep working the same way: raise the schema's version, and give a migration for what the engine does not do by itself. See [Migrations](../guide/migrations.md).
- **Encryption** takes a 32-byte key or a password instead of Realm's 64-byte key. The new file has a key of its own; see [Encryption](../guide/encryption.md).
- **Sync** to a server has no counterpart: DaruDB keeps data in a local file only.
