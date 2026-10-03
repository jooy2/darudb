---
title: 필드 타입
order: 5
counterpart: /types/rust/field-type
---

# 필드 타입

필드의 Dart 타입이 파일에 담길 값과 null 허용 여부, 그리고 쿼리에서 그 필드에 닿는 필드 객체를 정합니다.

| Dart 타입 | 파일에서의 종류 | 필드 객체 |
| --- | --- | --- |
| `bool` | `BoolKind` | `BoolField` |
| `int` | 64비트 `IntKind` | `IntField` |
| `double` | 64비트 `FloatKind` | `FloatField` |
| `String` | `StringKind` | `StringField` |
| `Uint8List` | `BytesKind` | `BytesField` |
| 위 타입의 `List<E>` | `ListKind` | `ListField<E>`. `List<String>`은 `StringListField` |
| `Link<User>` | `LinkKind('users')` | `UserLink` |
| `List<Link<User>>` | `ListKind(LinkKind('users'))` | `ListField<Object>` |
| `@Embedded()` 클래스 `Address` | `ObjectKind(addressSchema)` | `AddressFields` |

```dart
@Collection('users')
class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.tags = const [],
    this.team,
    this.address,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  final List<String> tags;
  final Link<Team>? team;
  final Address? address;
}
```

## 필수, 선택, 기본값

- **필수**: `String name`처럼 null이 될 수 없는 타입의 필드입니다. 모든 객체에 있습니다.
- **선택**: `String? email`처럼 null이 될 수 있는 필드입니다. null일 수 있고, 필드가 생기기 전에 쓴 레코드는 null로 읽습니다.
- **기본값**: `this.age = 0`처럼 null이 될 수 없고 생성자 매개변수에 기본값이 있는 필드입니다. 필수이며, 필드가 생기기 전에 쓴 레코드는 기본값으로 읽습니다. 기본값은 파일에 저장할 수 있는 상수, 즉 `bool`, `int`, `double`, `String`이나 이들의 목록입니다.

null이 될 수 있으면서 기본값이 있는 필드와, null이 들어가는 목록은 생성기가 거부합니다.

## 값

- **정수**는 64비트입니다. 패키지가 도는 모든 플랫폼에서 Dart의 `int`가 64비트이기 때문입니다.
- **문자열**은 UTF-8로 저장하고 바이트로 비교합니다.
- **바이트**는 따로 만든 `Uint8List`로 돌아오므로, 프로그램이 바꿔도 데이터베이스는 바뀌지 않습니다.
- **목록**은 따로 만든 늘릴 수 있는 리스트로 돌아옵니다.
- **내장 객체**는 그 클래스의 생성자로 만든 인스턴스로 돌아옵니다.
- **링크**는 [Link](./link.md) 값으로 돌아오며, `key`는 대상 컬렉션의 키 타입입니다.
