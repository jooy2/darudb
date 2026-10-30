---
title: Realm
order: 3
languages: [node, dart]
---

# Realm에서 옮기기

Realm 파일의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 객체 스키마와 속성 타입이 컬렉션과 필드에 어떻게 대응하는지, Realm의 SDK로 모든 객체를 읽어 복사하는 코드를 Node.js 스크립트와 Flutter 앱 양쪽으로 보이고, 라이브 객체를 쓰던 애플리케이션에서 무엇이 바뀌는지 설명합니다.

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
- **큰 정수.** Node.js에서는 JavaScript SDK가 `int`를 모두 number로 읽습니다. 그래서 2^53을 넘는 값은 스크립트가 받기 전에 이미 정밀도를 잃습니다. Dart SDK는 `int`로 읽으므로 값이 그대로 남습니다.

## 스키마 선언하기

예제로 옮길 Realm 스키마는 내장 주소가 있는 사용자와, 사용자를 가리키는 글입니다.

::: lang node

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

DaruDB 스키마는 TypeScript로 이렇게 선언합니다.

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

:::

::: lang dart

앱의 Realm 모델입니다. `realm_generator`가 여기서 `User`, `Post`, `Address`를 만들었습니다.

```dart
import 'dart:typed_data';

import 'package:realm/realm.dart';

part 'realm_models.realm.dart';

@RealmModel(ObjectType.embeddedObject)
class _Address {
  late String city;
  String? zip;
}

@RealmModel()
class _User {
  @PrimaryKey()
  @MapTo('_id')
  late ObjectId id;
  late String name;
  @Indexed()
  String? email;
  @Indexed()
  int age = 0;
  double? score;
  Uint8List? avatar;
  late DateTime joined;
  late List<String> tags;
  _Address? address;
  @Backlink(#author)
  late Iterable<_Post> posts;
}

@RealmModel()
class _Post {
  @PrimaryKey()
  late String slug;
  _User? author;
  late List<_User> readers;
  Decimal128? price;
  Uuid? ref;
  late Map<String, String> meta;
}
```

`id`는 문자열 키로, 날짜는 밀리초 숫자로, `Decimal128`과 `Uuid`는 문자열로, 맵은 JSON 문자열로 바꿉니다. 백링크 `posts`는 옮기지 않습니다. `author`에 인덱스를 두면 사용자의 글은 쿼리 한 번으로 찾을 수 있습니다.

`realm` 패키지는 `realm_generator`에 의존하고, 이 생성기는 `darudb_generator`가 요구하는 것보다 오래된 `analyzer`와 `source_gen`에 묶여 있습니다. 그래서 두 생성기는 한 패키지 안에서 의존성이 풀리지 않습니다. DaruDB 클래스는 앱 저장소 안의 별도 패키지 `packages/app_data`에 두고, `darudb_generator`는 그 패키지의 `dev_dependencies`에 넣습니다.

```yaml
name: app_data
publish_to: none

environment:
  sdk: ^3.10.0

dependencies:
  darudb: ^1.4.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.1.0
```

```dart
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'app_data.g.dart';

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

`packages/app_data`에서 `dart run build_runner build`를 돌리면 `app_data.g.dart`가 생깁니다. 앱은 `realm` 옆에 이 패키지를 경로로 의존하고, 자기 모델에는 계속 `realm_generator`를 돌립니다.

```yaml
dependencies:
  realm: ^20.2.0
  app_data:
    path: packages/app_data
```

나중 버전의 앱이 더는 Realm 파일을 열지 않게 되면 `realm`을 빼고, 클래스를 `darudb_generator`와 함께 앱으로 다시 옮겨도 됩니다.

:::

## 객체 복사하기

::: lang node

스크립트는 `realm` 패키지로 Realm 파일을 읽고 Node.js 패키지로 DaruDB 파일을 씁니다.

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
- **클래스는 하나씩 옮깁니다.** 링크는 지금까지 무엇을 복사했든 키를 담으므로, 클래스를 어떤 순서로 옮겨도 됩니다.

:::

::: lang dart

Flutter 앱의 Realm 파일은 기기마다 있으므로, 복사는 앱 안에서 합니다. 옮기는 버전이 처음 실행될 때 한 번, 앱의 Realm 모델로 읽어 복사합니다.

```dart
import 'dart:convert';
import 'dart:io';

import 'package:app_data/app_data.dart';
import 'package:darudb/darudb.dart';
import 'package:realm/realm.dart' as realm;

import 'realm_models.dart' as old;

/// [from]의 Realm 파일을 [path]의 DaruDB 파일로 한 번 복사합니다.
Future<void> moveFromRealm(String from, String path) async {
  final part = File('$path.part');

  if (File(path).existsSync()) {
    return;
  }

  // 충돌로 중간에 끊긴 복사는 처음부터 다시 합니다.
  if (part.existsSync()) {
    part.deleteSync();
  }

  final source = realm.Realm(
    realm.Configuration.local(
      [old.User.schema, old.Post.schema, old.Address.schema],
      path: from,
      isReadOnly: true,
    ),
  );
  final db = await Database.openAsync(part.path, schema: app);

  /// [objects]를 트랜잭션마다 천 개씩 컬렉션에 복사합니다.
  Future<void> copy<R, T>(
    Iterable<R> objects,
    CollectionSchema<T, QueryBuilder<T>, Object> collection,
    T Function(R object) convert,
  ) async {
    final batch = <T>[];

    Future<void> flush() async {
      await db.writeAsync(
        (txn) => txn.collection(collection).insertMany(batch),
        durability: Durability.deferred,
      );
      batch.clear();
    }

    for (final object in objects) {
      batch.add(convert(object));

      if (batch.length == 1000) {
        await flush();
      }
    }

    if (batch.isNotEmpty) {
      await flush();
    }
  }

  await copy(
    source.all<old.User>(),
    userSchema,
    (old.User user) => User(
      id: user.id.hexString,
      name: user.name,
      email: user.email,
      age: user.age,
      score: user.score,
      avatar: user.avatar,
      joined: user.joined.millisecondsSinceEpoch,
      tags: List.of(user.tags),
      address: user.address == null
          ? null
          : Address(city: user.address!.city, zip: user.address!.zip),
    ),
  );

  await copy(
    source.all<old.Post>(),
    postSchema,
    (old.Post post) => Post(
      slug: post.slug,
      author: post.author == null ? null : Link<User>(post.author!.id.hexString),
      readers: [for (final user in post.readers) Link<User>(user.id.hexString)],
      price: post.price?.toString(),
      ref: post.ref?.toString(),
      meta: jsonEncode(Map.of(post.meta)),
    ),
  );

  // 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
  await db.closeAsync();
  source.close();
  part.renameSync(path);
}
```

앱은 데이터베이스를 열기 전에 이 함수를 부릅니다. Realm 파일의 경로와 함께, 새 파일은 시스템이 비우지 않는 폴더에 둡니다. `path_provider`의 `getApplicationSupportDirectory`가 주는 폴더가 그런 곳입니다.

```dart
final folder = await getApplicationSupportDirectory();
final path = '${folder.path}/app.darudb';

await moveFromRealm(realm.Configuration.defaultRealmPath, path);

final db = await Database.openAsync(path, schema: app);
```

- **앱의 모델로, 읽기 전용으로 엽니다.** `isReadOnly`를 주고 열면 파일은 그대로 남습니다. 암호화한 파일이라면 설정에 `encryptionKey`도 줍니다.
- **다른 이름으로 복사하고 나서 제자리로 옮깁니다.** 복사본은 `app.darudb.part`에 쓰고, 닫은 뒤에 이름을 바꿉니다. 그래서 앱이 찾는 파일은 아예 없거나(이때는 다시 복사합니다) 완전한 파일, 둘 중 하나입니다.
- **UI를 막지 않습니다.** 엔진의 쓰기는 `Future` API를 거쳐 네이티브 라이브러리의 스레드에서 돌므로, 복사하는 동안 앱은 업데이트 중이라는 화면을 보여 줄 수 있습니다.
- **Realm 파일은 한동안 남겨 둡니다.** 앱이 새 파일로 문제없이 돈 뒤, 다음 버전에서 `realm.Realm.deleteRealm`으로 지웁니다.
- **접두사를 붙여 가져옵니다.** Realm 모델과 DaruDB 클래스의 이름이 같으므로, 복사 코드는 모델을 `old`라는 이름으로 가져옵니다.

:::

## 복사본 확인하기

클래스마다 Realm에서 센 객체 수(Node.js에서는 `objects('User').length`, Dart에서는 `all`의 `length`)를 컬렉션의 `count`와 견주고, 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다.

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

- `CONTAINS[c]`의 `[c]`는 조건 뒤의 `IGNORE CASE`가 됩니다. `name CONTAINS "li" IGNORE CASE`처럼 씁니다. 와일드카드를 쓰는 `LIKE`에는 대응하는 것이 없습니다.
- 필드의 집계 `@sum`, `@min`, `@max`, `@avg`는 쿼리와 함께 컬렉션의 `sum`, `min`, `max`, `avg`가 되고, `@count`는 `count`가 됩니다. `items.@sum.price > 10`처럼 필터 안에서 쓰는 집계에는 대응하는 것이 없습니다.
- 백링크는 링크 필드로 하는 쿼리가 됩니다. 사용자의 글은 `posts`에서 사용자의 키로 `author == $0`을 찾으면 됩니다. `@links.Post.author.year > 2020`처럼 백링크에 거는 필터는 `users`에서 `ANY posts.author (year > 2020)`이 됩니다.

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다.

## 애플리케이션에서 바뀌는 것

- **결과는 평범한 객체입니다.** 쿼리는 파일에 묶인 라이브 객체가 아니라 파일에서 복사해 온 객체를 돌려줍니다. 데이터가 바뀌어도 객체는 바뀌지 않고, 트랜잭션과 데이터베이스를 닫은 뒤에도 그대로 쓸 수 있습니다. 바꾼 내용은 `put`이나 `update`로 다시 씁니다.
- **변경 알림이 없습니다.** 데이터가 바뀌어도 애플리케이션을 불러 주는 것이 없습니다. 쓰기가 끝나면 화면에 보이는 것을 다시 읽으세요.
- **트랜잭션**은 `realm.write` 대신 <LangCode node="db.write" dart="db.write와 db.writeAsync" />를 쓰고, 읽기도 읽기 트랜잭션 안에서 합니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **스키마 버전**은 그대로 같은 방식입니다. 스키마 버전을 올리고, 엔진이 알아서 하지 않는 변경은 마이그레이션에 적습니다. [마이그레이션](../guide/migrations.md)을 보세요.
- **암호화**는 Realm의 64바이트 키 대신 32바이트 키나 비밀번호를 씁니다. 새 파일에는 새 키를 정합니다. [암호화](../guide/encryption.md)를 보세요.
- **서버와의 동기화**에는 대응하는 것이 없습니다. DaruDB는 데이터를 로컬 파일에만 둡니다.
- **속도**는 [성능 페이지](../performance.md)의 Node.js 표와 Dart 표에서 같은 객체와 작업으로 Realm과 나란히 쟀습니다.
