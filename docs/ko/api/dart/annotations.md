---
title: 어노테이션
order: 2
counterpart: /api/rust/derive
---

# 어노테이션

어노테이션은 Dart 클래스를 컬렉션의 객체나 내장 객체로 만들고, `darudb_generator`는 그 클래스를 저장하는 코드를 씁니다.

```yaml
dependencies:
  darudb: ^1.1.0

dev_dependencies:
  build_runner: ^2.10.0
  darudb_generator: ^1.1.0
```

어노테이션을 붙인 클래스가 있는 라이브러리는 자기 이름을 딴 파트를 선언하고, `dart run build_runner build`가 그 파트를 씁니다. 생성기는 어노테이션만 읽으며, 실행 중인 프로그램이 생성기를 부르는 일은 없습니다.

```dart
import 'dart:typed_data';

import 'package:darudb/darudb.dart';

part 'models.g.dart';

@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  @Name('postcode')
  final String? zip;
}

@Collection('users')
class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.tags = const [],
    this.avatar,
    this.address,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  @Index()
  final List<String> tags;
  final Uint8List? avatar;
  final Address? address;
}

@Collection('posts')
class Post {
  const Post({required this.slug, required this.author, this.readers = const []});

  @PrimaryKey()
  final String slug;
  @Index()
  final Link<User> author;
  final List<Link<User>> readers;
}

final db = Database.open('app.darudb', schema: const Schema(1, [userSchema, postSchema]));
```

## 클래스

어노테이션을 붙인 클래스는 값입니다. 생성된 코드는 생성자로 객체를 만들고, 필드로 값을 읽습니다.

- **필드는 모두 `final`이고**, 클래스에는 타입 매개변수가 없습니다.
- **이름 없는 생성자가 모든 필드를 받습니다.** 이름 있는 매개변수든 위치 매개변수든 필드와 이름이 같아야 합니다. 저장하는 것은 클래스가 직접 선언한 인스턴스 필드뿐이고, getter와 static 필드, 상속한 필드는 저장하지 않습니다.
- **필드의 타입이 파일에서의 타입입니다.** `bool`, `int`, `double`, `String`, `Uint8List`, 이들이나 링크의 `List`, 컬렉션을 가리키는 [Link](../../types/dart/link.md), `@Embedded()`를 붙인 클래스를 쓸 수 있습니다. 목록에는 null이 들어가지 않습니다. 전체 목록은 [필드 타입](../../types/dart/field-types.md)에 있습니다.
- **null이 될 수 있으면 선택 필드입니다.** 그런 필드는 null일 수 있고, 레코드에 없으면 null이 됩니다. null이 될 수 없는 필드는 필수입니다.
- **매개변수의 기본값이 필드의 기본값입니다.** 필드가 생기기 전에 쓴 레코드는 이 값을 읽습니다. 기본값은 파일에 저장할 수 있는 상수, 즉 `bool`, `int`, `double`, `String`이나 이들의 목록입니다. 기본값이 있는 필드는 필수이므로 null이 될 수 없습니다.

이 중 하나라도 어기면 생성기는 클래스와 필드 이름을 담은 메시지를 내고 멈춥니다.

## 어노테이션 목록

### Collection

```dart
const Collection([String? name]);
```

클래스를 `name`이라는 컬렉션의 객체로 만듭니다. `name`을 빼면 컬렉션 이름은 클래스 이름 그대로입니다. `@PrimaryKey()`를 붙인 필드가 없으면 자동 증가 키를 쓰는 컬렉션이 되고, 클래스에는 다른 어노테이션 없이 `final int? id` 필드가 있어야 합니다. 이 필드는 아직 넣지 않은 객체에서 `null`이고, `null`인 채로 넣으면 엔진이 다음 번호를 줍니다.

### Embedded

```dart
const Embedded();
```

클래스를 내장 객체로 만듭니다. 내장 객체는 다른 객체의 필드에 담깁니다. 키가 없고, 그 필드에는 인덱스를 둘 수 없습니다.

### PrimaryKey

```dart
const PrimaryKey();
```

필드를 컬렉션의 기본 키로 만듭니다. `int`, `String`, `Uint8List` 중 하나이고, 필수이며 기본값이 없어야 합니다. 기본 키는 컬렉션마다 하나까지입니다.

### Index

```dart
const Index();
```

필드에 인덱스를 둡니다. 그 필드에 조건을 건 쿼리는 찾는 객체만 읽고, 그 필드 하나로만 정렬한 쿼리는 정렬 순서대로 읽습니다. 목록 필드의 인덱스에는 원소마다 항목이 생깁니다.

### Unique

```dart
const Unique();
```

필드에 인덱스를 두되, 값이 같은 객체 둘을 `DUPLICATE_KEY`로 거부합니다. null은 몇 개가 있어도 됩니다.

### Name

```dart
const Name(String name);
```

파일 안의 필드 이름을 Dart 필드 이름 대신 `name`으로 정합니다. 쿼리 빌더는 Dart 필드 이름으로, 쿼리 언어는 파일 안의 이름으로 필드를 가리킵니다.

## 생성기가 쓰는 코드

`@Collection()`을 붙인 클래스 `User`에는 이것들을 씁니다.

| 생성되는 것 | 내용 |
| --- | --- |
| `userSchema` | 컬렉션의 [CollectionSchema](../../types/dart/collection-schema.md). [Schema](./schema.md)에 넣고 `txn.collection`에 넘기는 상수입니다 |
| `UserQuery` | 컬렉션의 [QueryBuilder](./query-builder.md). 필드마다 [필드 객체](./fields.md)가 있습니다 |
| `UserLink` | `User`를 가리키는 링크의 필드 객체. `User`의 필드가 있어서, 쿼리가 링크를 거쳐 읽을 수 있습니다 |
| `UserCopyWith` | `copyWith`가 있는 확장. 받은 필드만 바꾼 사본을 만듭니다 |

`@Embedded()`를 붙인 클래스 `Address`에는 [EmbeddedSchema](../../types/dart/collection-schema.md#embeddedschema)인 `addressSchema`, 내장된 `Address`의 필드 객체인 `AddressFields`, 그리고 `AddressCopyWith`를 씁니다.

`copyWith`는 받지 않은 필드를 모두 그대로 두므로 필드를 null로 만들 수 없습니다. 그럴 때는 객체를 새로 만들거나 `update`에서 `set(null)`을 씁니다.
