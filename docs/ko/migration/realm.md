---
title: Realm
order: 3
---

# Realm에서 옮기기

Realm 파일의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 객체 스키마와 속성 타입이 컬렉션과 필드에 어떻게 대응하는지, 모든 객체를 복사하는 Node.js 스크립트, 그리고 라이브 객체를 쓰던 애플리케이션에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| Realm | DaruDB |
| --- | --- |
| 객체 스키마, 곧 클래스 | 컬렉션 |
| 객체 | 객체 |
| `primaryKey` | 기본 키. 타입은 정수, 문자열, 바이트 중 하나 |
| `indexed: true` | 인덱스 |
| `bool`, `int`, `float`나 `double`, `string`, `data` | 불리언, 정수, 실수, 문자열, 바이트 필드 |
| `date` | 에포크 기준 밀리초를 담는 정수 필드, 또는 ISO 8601 문자열 |
| `objectId` | 16진수 24자리 문자열, 또는 12바이트 |
| `uuid`, `decimal128` | 문자열 필드. DaruDB에는 두 타입이 없습니다 |
| 객체를 가리키는 링크 | 링크. 다른 객체의 기본 키를 담습니다 |
| 리스트 | 값이나 링크의 목록 |
| 세트 | 목록. 값이 겹치지 않게 하는 것은 애플리케이션의 몫입니다 |
| 딕셔너리 | 대응하는 것 없음. 키가 정해져 있으면 내장 객체로, 아니면 JSON 문자열로 |
| `mixed` | 대응하는 것 없음. 담는 타입마다 필드를 두거나 JSON 문자열로 |
| 내장 객체 | 내장 객체 |
| 내장 객체의 리스트 | 이번 버전에는 대응하는 것 없음. 따로 컬렉션을 두고 부모를 링크로 가리킵니다 |
| `linkingObjects`, 곧 백링크 | 저장하지 않습니다. 링크 필드로 쿼리하고, 그 필드에 인덱스를 두면 비용이 적습니다 |

- **링크에는 키가 필요합니다.** 링크는 가리키는 객체의 기본 키를 담으므로, 다른 객체가 가리키는 클래스에는 기본 키가 있어야 합니다. 키가 없는 클래스는 복사하기 전에 Realm 마이그레이션으로 키를 주거나, 객체마다 부모가 하나뿐이면 내장 객체로 옮기세요.
- **기본값은 파일에 없습니다.** Realm 파일에는 클래스와 속성은 기록되지만 기본값은 기록되지 않으므로, DaruDB의 기본값은 직접 선언합니다.
- **큰 정수.** JavaScript SDK는 `int`를 모두 number로 읽습니다. 그래서 2^53을 넘는 값은 스크립트가 받기 전에 이미 정밀도를 잃습니다.

## 스키마 선언하기

예제로 옮길 Realm 스키마는 내장 주소가 있는 사용자와, 사용자를 가리키는 글입니다.

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

`_id`는 `id`라는 문자열 키로, 날짜는 밀리초 숫자로, `decimal128`과 `uuid`는 문자열로, 딕셔너리는 JSON 문자열로 바꿉니다. 백링크 `posts`는 옮기지 않습니다. `author`에 인덱스를 두면 사용자의 글은 쿼리 한 번으로 찾을 수 있습니다.

복사는 `realm` 패키지를 쓰는 Node.js에서 하므로, DaruDB 스키마도 TypeScript로 선언합니다.

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

Rust 프로그램은 복사한 파일을 같은 선언을 Rust로 옮긴 스키마로 엽니다. 버전과 컬렉션, 필드, 타입, 인덱스가 모두 같아야 하며, 다르면 `SCHEMA_MISMATCH`로 열리지 않습니다.

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

Dart 프로그램은 복사한 파일을 같은 선언을 어노테이션을 붙인 클래스로 옮긴 스키마로 엽니다. 버전과 컬렉션, 필드, 타입, 인덱스가 모두 같아야 하며, 다르면 `SCHEMA_MISMATCH`로 열리지 않습니다.

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

## 객체 복사하기

스크립트는 `realm` 패키지로 Realm 파일을 읽고 Node.js 패키지로 DaruDB 파일을 씁니다. DaruDB 파일은 어느 언어에서나 같은 파일이므로, 애플리케이션이 어떤 언어로 되어 있든 이 방법으로 옮깁니다.

::: lang rust

Realm에는 Rust SDK가 없습니다. 아래 스크립트를 Node.js 패키지로 한 번 돌린 뒤, 스크립트가 쓴 파일을 Rust에서 `OpenOptions::new().schema(schema()).open("app.darudb")`로 여세요.

:::

::: lang dart

Realm의 Flutter SDK로 앱의 Realm 모델 클래스를 거쳐 Dart에서 파일을 읽고, 클래스 하나씩 복사해도 됩니다. 아래 스크립트를 쓰면 그럴 필요가 없습니다. Node.js 패키지로 한 번 돌린 뒤, 스크립트가 쓴 파일을 Dart에서 `Database.open('app.darudb', schema: app)`으로 여세요.

:::

```js
import Realm from 'realm';
import { Database } from 'darudb';

const realm = await Realm.open({ path: 'app.realm', readOnly: true });
const db = Database.open('app.darudb', { schema: app });

/** `objects`를 컬렉션 `name`에 트랜잭션마다 천 개씩 복사합니다. */
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

// 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close();
realm.close();

// `realm` 패키지는 `close` 뒤에도 Node.js를 끝내지 않습니다.
process.exit(0);
```

- **파일 자체의 스키마로, 읽기 전용으로 엽니다.** `readOnly`를 주고 스키마 없이 열면 파일에 있는 그대로 읽고, 파일은 그대로 남습니다. 암호화한 파일이라면 `encryptionKey`도 줍니다.
- **내장 객체가 아니라 클래스를 복사합니다.** `realm.objects`는 클래스를 받습니다. 내장 객체는 여기서 `address`처럼 부모와 함께 복사합니다.
- **클래스 순서는 상관없습니다.** 링크는 지금까지 무엇을 복사했든 키를 담으므로, 클래스를 어떤 순서로 옮겨도 됩니다.

## 복사본 확인하기

클래스마다 `realm.objects('User').length`를 `users` 컬렉션의 개수(<LangCode rust="len" node="count" dart="count" />)와 견주고, 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다.

## 쿼리

쿼리 언어는 `filtered`가 받는 Realm Query Language와 비슷합니다.

| Realm Query Language                     | DaruDB 쿼리 언어                            |
| ---------------------------------------- | ------------------------------------------- |
| `age >= $0`                              | `age >= $0`                                 |
| `email == nil`                           | `email IS NULL`                             |
| `age BETWEEN {18, 30}`                   | `age BETWEEN 18 AND 30`                     |
| `age IN {18, 30}`                        | `age IN [18, 30]`                           |
| `name BEGINSWITH 'A'`                    | `name STARTSWITH "A"`                       |
| `name CONTAINS 'li'`, `ENDSWITH`         | `name CONTAINS "li"`, `ENDSWITH`            |
| `ANY tags == 'red'`                      | `tags == "red"`, 또는 `tags CONTAINS "red"` |
| `author.name == 'Alice'`                 | `author.name == "Alice"`                    |
| `TRUEPREDICATE SORT(age DESC) LIMIT(10)` | `SORT BY age DESC LIMIT 10`                 |

- `CONTAINS[c]`의 `[c]`처럼 대소문자를 가리지 않는 비교는 없습니다. 필드를 소문자로 바꾼 사본을 두고 그 필드로 찾으세요. 와일드카드를 쓰는 `LIKE`에도 대응하는 것이 없습니다.
- 쿼리 안에서 쓰는 `@count`, `@sum`, `@avg` 같은 집계에는 대응하는 것이 없습니다. `count`는 쿼리가 찾은 객체의 수를 셉니다.
- 백링크는 링크 필드로 하는 쿼리가 됩니다. 사용자의 글은 `posts`에서 사용자의 키로 `author == $0`을 찾으면 됩니다.

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **결과는 평범한 객체입니다.** 쿼리는 파일에 묶인 라이브 객체가 아니라 파일에서 복사해 온 객체를 돌려줍니다. 데이터가 바뀌어도 객체는 바뀌지 않고, 트랜잭션과 데이터베이스를 닫은 뒤에도 그대로 쓸 수 있습니다. 바꾼 내용은 `put`이나 `update`로 다시 씁니다.
- **변경 알림이 없습니다.** 데이터가 바뀌어도 애플리케이션을 불러 주는 것이 없습니다. 쓰기가 끝나면 화면에 보이는 것을 다시 읽으세요.
- **트랜잭션**은 `realm.write` 대신 <LangCode rust="begin_write와 commit" node="db.write" dart="db.write" />를 쓰고, 읽기도 읽기 트랜잭션 안에서 합니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **스키마 버전**은 그대로 같은 방식입니다. 스키마 버전을 올리고, 엔진이 알아서 하지 않는 변경은 마이그레이션에 적습니다. [마이그레이션](../guide/migrations.md)을 보세요.
- **암호화**는 Realm의 64바이트 키 대신 32바이트 키나 비밀번호를 씁니다. 새 파일에는 새 키를 정합니다. [암호화](../guide/encryption.md)를 보세요.
- **서버와의 동기화**에는 대응하는 것이 없습니다. DaruDB는 데이터를 로컬 파일에만 둡니다.
