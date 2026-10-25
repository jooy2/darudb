---
title: Hive
order: 8
languages: [dart]
---

# Hive에서 옮기기

Flutter 앱이 Hive 박스에 담은 데이터를 DaruDB로 옮기는 방법을 다룹니다. 박스와 키, 타입 어댑터가 컬렉션과 객체, 필드에 어떻게 대응하는지, 앱 안에서 모든 항목을 복사하는 코드, 그리고 앱에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| Hive | DaruDB |
| --- | --- |
| 박스. 박스마다 `.hive` 파일 하나 | 컬렉션. 모든 컬렉션이 데이터베이스 파일 하나에 들어갑니다 |
| `add`가 주는 것 같은 `int` 키 | 정수인 자동 `id` |
| `put`이 받는 것 같은 `String` 키 | 문자열 기본 키 |
| `@HiveType`을 붙인 클래스와 생성된 어댑터 | `@Collection()`을 붙인 클래스. 코드는 `darudb_generator`가 만듭니다 |
| `@HiveField`를 붙인 필드 | 클래스의 필드. 저장하는 필드의 번호는 엔진이 매깁니다 |
| 다른 항목의 키를 담는 필드 | `Link`. 그 객체의 키를 담습니다 |
| `HiveList` | 링크 목록 |
| 다른 클래스 안에 든, 어댑터가 따로 있는 클래스 | 내장 객체. `@Embedded()`를 붙인 클래스 |
| `save`와 `delete`가 있는 `HiveObject` | 대응하는 것 없음. 쓰기 트랜잭션 안에서 `put`과 `delete`를 씁니다 |
| `LazyBox` | 컬렉션. 읽기는 모두 페이지 캐시를 거쳐 파일에서 합니다 |
| `box.watch`, `box.listenable()`에 건 `ValueListenableBuilder` | 대응하는 것 없음. 쓰기가 끝나면 다시 읽습니다 |
| `HiveAesCipher` | 키나 비밀번호로 하는 파일 전체 암호화 |
| `compact` | `compact` |

- **컬렉션마다 키는 한 종류입니다.** 박스에는 `int` 키와 `String` 키가 섞여도 되지만, 컬렉션의 기본 키는 타입이 하나입니다. 두 키가 섞인 박스는 문자열 키를 쓰는 컬렉션으로 옮기고, 정수 키는 문자열로 바꿉니다. 그런 키를 담던 필드도 모두 같은 방식으로 바꿉니다.
- **키를 그대로 옮깁니다.** 항목의 키를 객체의 키로 옮기면 항목 사이의 참조가 모두 링크로 살아남습니다. Hive가 매기는 키는 0부터 시작하며, 컬렉션은 옮겨 넣은 가장 큰 키 다음부터 번호를 이어서 매깁니다.
- **인덱스가 생깁니다.** Hive는 키로 항목을 찾고, 그 밖의 검색은 메모리에 있는 값을 훑어서 합니다. 앱이 항목을 찾을 때 쓰는 필드마다 인덱스를 선언하면, 엔진은 맞는 객체만 읽습니다.

## 스키마 선언하기

예제로 옮길 것은 Flutter 앱의 박스 둘입니다. `users`에는 앱이 `add`로 항목을 넣었고, `posts`에는 슬러그를 키로 `put`했습니다. 앱의 Hive 클래스는 이렇습니다.

```dart
import 'package:hive_ce/hive_ce.dart';

part 'hive_models.g.dart';

@HiveType(typeId: 0)
class User {
  User({required this.name, this.email, this.age = 0});

  @HiveField(0)
  final String name;
  @HiveField(1)
  final String? email;
  @HiveField(2)
  final int age;
}

@HiveType(typeId: 1)
class Post {
  Post({this.authorKey, required this.body, this.publishedAt, required this.tags});

  /// `users` 박스에 있는 사용자의 키.
  @HiveField(0)
  final int? authorKey;
  @HiveField(1)
  final String body;
  @HiveField(2)
  final String? publishedAt;
  @HiveField(3)
  final List<String> tags;
}
```

DaruDB 클래스는 이름이 같으므로 따로 `schema.dart` 파일에 둡니다. `users`는 Hive의 키를 자동 `id`로 그대로 쓰고, `authorKey`는 링크가 됩니다.

```dart
import 'package:darudb/darudb.dart';

part 'schema.g.dart';

@Collection('users')
class User {
  const User({this.id, required this.name, this.email, this.age = 0});

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
}

@Collection('posts')
class Post {
  const Post({
    required this.slug,
    this.author,
    required this.body,
    this.publishedAt,
    required this.tags,
  });

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User>? author;
  final String body;
  final String? publishedAt;
  @Index()
  final List<String> tags;
}

const app = Schema(1, [userSchema, postSchema]);
```

`darudb_generator`와 `hive_ce_generator`는 `build_runner build` 한 번에 함께 돕니다.

```yaml
dependencies:
  darudb: ^1.3.0
  hive_ce: ^2.11.0
  hive_ce_flutter: ^2.3.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.1.0
  hive_ce_generator: ^1.11.0
```

원래의 `hive` 패키지를 쓰는 앱은 이렇게 할 수 없습니다. `hive_generator`는 `darudb_generator`가 요구하는 것보다 오래된 `analyzer`와 `source_gen`에 묶여 있어서, 두 생성기는 한 패키지 안에서 의존성이 풀리지 않습니다. 이때는 [Realm 가이드](./realm.md#스키마-선언하기)처럼 DaruDB 클래스를 별도 패키지에 두고 `darudb_generator`를 그 패키지의 `dev_dependencies`에 넣은 뒤, 앱에서 그 패키지를 경로로 의존합니다.

## 항목 복사하기

박스는 기기마다 있으므로, 복사는 앱 안에서 합니다. 옮기는 버전이 처음 실행될 때, 앱이 박스를 열기 전에 한 번 합니다.

```dart
import 'dart:io';

import 'package:darudb/darudb.dart';
import 'package:hive_ce_flutter/hive_flutter.dart';

import 'hive_models.dart' as hive;
import 'schema.dart';

/// Hive 박스를 [path]의 DaruDB 파일로 한 번 복사합니다.
Future<void> moveFromHive(String path) async {
  final part = File('$path.part');

  if (File(path).existsSync()) {
    return;
  }

  // 충돌로 중간에 끊긴 복사는 처음부터 다시 합니다.
  if (part.existsSync()) {
    part.deleteSync();
  }

  await Hive.initFlutter();
  Hive.registerAdapter(hive.UserAdapter());
  Hive.registerAdapter(hive.PostAdapter());

  final users = await Hive.openBox<hive.User>('users');
  final posts = await Hive.openBox<hive.Post>('posts');
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
    users.toMap().entries,
    userSchema,
    (MapEntry<dynamic, hive.User> entry) => User(
      id: entry.key as int,
      name: entry.value.name,
      email: entry.value.email,
      age: entry.value.age,
    ),
  );

  await copy(
    posts.toMap().entries,
    postSchema,
    (MapEntry<dynamic, hive.Post> entry) => Post(
      slug: entry.key as String,
      author: entry.value.authorKey == null ? null : Link<User>(entry.value.authorKey!),
      body: entry.value.body,
      publishedAt: entry.value.publishedAt,
      tags: entry.value.tags,
    ),
  );

  // 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
  await db.closeAsync();
  await Hive.close();
  part.renameSync(path);
}
```

앱은 데이터베이스를 열기 전에 이 함수를 부릅니다. 새 파일은 시스템이 비우지 않는 폴더에 둡니다. `path_provider`의 `getApplicationSupportDirectory`가 주는 폴더가 그런 곳입니다.

```dart
final folder = await getApplicationSupportDirectory();
final path = '${folder.path}/app.darudb';

await moveFromHive(path);

final db = await Database.openAsync(path, schema: app);
```

- **다른 이름으로 복사하고 나서 제자리로 옮깁니다.** 복사본은 `app.darudb.part`에 쓰고, 닫은 뒤에 이름을 바꿉니다. 그래서 앱이 찾는 파일은 아예 없거나(이때는 다시 복사합니다) 완전한 파일, 둘 중 하나입니다. 박스는 바뀌지 않으므로 복사가 중간에 끊겨도 잃는 것이 없습니다.
- **UI를 막지 않습니다.** 엔진의 쓰기는 `Future` API를 거쳐 네이티브 라이브러리의 스레드에서 돌므로, 복사하는 동안 앱은 업데이트 중이라는 화면을 보여 줄 수 있습니다.
- **박스는 한동안 남겨 둡니다.** 앱이 새 파일로 문제없이 돈 뒤, 다음 버전에서 `Hive.deleteBoxFromDisk`로 지웁니다.

## 복사본 확인하기

박스를 지우기 전에 박스마다 `users.length`를 컬렉션의 `count`와 견주고, 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

앱이 박스의 값을 훑어서 하던 일은 쿼리가 됩니다. 맞는 인덱스가 있으면 엔진이 그 인덱스로 쿼리를 돌립니다.

| 앱이 하던 일 | DaruDB 컬렉션에서 |
| --- | --- |
| `box.get(key)` | `get(key)` |
| `box.values.where((user) => user.age >= 18)` | `q.where(q.age.atLeast(18))`를 준 `find` |
| `box.values.firstWhere((user) => user.email == email)` | `q.where(q.email.equals(email))`를 준 `findOne`. 고유 인덱스를 거칩니다 |
| `box.values.where((post) => post.tags.contains('news'))` | `posts`에서 `q.where(q.tags.contains('news'))`를 준 `find` |
| 값을 정렬한 뒤 `take(10)` | `q.sortBy(q.age, descending: true).limit(10)`을 준 `find` |
| `box.length` | `count()` |

빌더 전체와 쿼리 언어는 [쿼리](../guide/queries.md)에 있습니다.

## 앱에서 바뀌는 것

- **읽기는 메모리에 둔 박스가 아니라 파일에서**, 페이지 캐시를 거쳐 합니다. 키로 읽으면 파일의 트리에서 찾으므로, 앱이 모든 박스를 한꺼번에 메모리에 둘 필요가 없어집니다.
- **쓰기는 트랜잭션입니다.** `db.write`와 `db.writeAsync`는 함수가 반환되면 커밋하고 예외를 던지면 중단하므로, 변경 여럿이 함께 들어가거나 하나도 들어가지 않습니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **객체는 바꿀 수 없습니다.** 바꿀 때는 `copyWith`로 만든 객체를 `put`으로 다시 쓰거나, `update`로 필드를 정합니다.
- **리스너가 없습니다.** 데이터가 바뀌어도 앱을 불러 주는 것이 없습니다. 쓰기가 끝나면 화면에 보이는 것을 다시 읽으세요.
- **여러 아이솔레이트와 프로세스**가 한 파일을 함께 쓸 수 있습니다. 앱과 그 확장 프로그램이 그런 경우입니다. 쓰는 쪽은 한 번에 하나, 읽는 쪽은 몇이든 됩니다. [여러 프로세스](../guide/processes.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
