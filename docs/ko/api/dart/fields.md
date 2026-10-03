---
title: 필드
order: 11
counterpart: [/api/rust/filter, /api/node/conditions]
---

# 필드

필드 객체는 쿼리에서 필드 하나를 가리키며, 그 메서드로 필터를 이루는 조건과 `update`가 할 변경을 만듭니다.

```dart
abstract base class Field {
  final List<String> path;
}
```

`darudb_generator`가 쓴 쿼리 빌더에는 클래스의 필드마다 getter가 있고, getter는 필드 타입에 맞는 필드 객체를 돌려줍니다. 프로그램은 `q.age`처럼 함수가 받은 빌더로만 필드 객체에 닿습니다.

| 필드의 Dart 타입 | 필드 객체 |
| --- | --- |
| `bool` | `BoolField` |
| `int` | `IntField` |
| `double` | `FloatField` |
| `String` | `StringField` |
| `Uint8List` | `BytesField` |
| `List<String>` | `StringListField` |
| 그 밖의 `List` | `ListField<E>`. `E`는 원소 타입이고, 링크 목록이면 `Object`입니다 |
| `Link<User>` | `User`의 필드가 있는 `LinkField`인 `UserLink` |
| `@Embedded()` 클래스 `Address` | `Address`의 필드가 있는 `EmbeddedField<Address>`인 `AddressFields` |

내장 객체나 링크가 가리키는 객체의 필드는 그것을 담은 필드를 거쳐 닿습니다. `q.address.city`처럼 쓰고, `q.team.city`처럼 쓰면 링크가 가리키는 객체를 검사합니다. 가리키는 객체가 없으면 null로 읽습니다. 목록과 null에 건 조건의 뜻은 [쿼리](../../guide/queries.md)에서 설명합니다.

```dart
final found = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q.where((q.age.atLeast(18) & q.age.lessThan(30)) | q.email.isNull()),
  ),
);
```

## Condition

```dart
final class Condition {
  Condition operator &(Condition other);
  Condition operator |(Condition other);
  Condition operator ~();
}
```

필드 객체의 메서드가 만드는 필터 조건입니다. `a & b`는 둘 다 참일 때, `a | b`는 둘 중 하나가 참일 때, `~a`는 `a`가 거짓일 때 참입니다. Dart에서는 `~`가 가장 강하게 묶이고 `&`가 `|`보다 강하게 묶이므로, `a | b & ~c`는 `a | (b & (~c))`입니다. 다르게 묶으려면 괄호를 씁니다. 조건은 값이므로 여러 쿼리에 함께 넣어도 됩니다.

필터는 24단계까지만 중첩할 수 있고, 넘으면 쿼리가 `INVALID_QUERY`로 실패합니다. `&`가 이어진 것이나 `|`가 이어진 것은 한 단계로 치므로, `~`와 번갈아 묶은 것만 단계를 늘립니다.

## Change

```dart
final class Change
```

[`update`](./write-collection.md#update)에 넘길 필드 하나의 새 값이며, 필드 객체의 `set`이 만듭니다. `set`은 객체 자신의 필드만 가리킵니다. 내장 객체나 링크를 거쳐 닿는 필드면 `INVALID_ARGUMENT`를 던집니다.

## 모든 필드의 메서드

### isNull

```dart
Condition isNull();
```

필드가 null일 때 참입니다. 빈 목록은 null이 아닙니다.

### isNotNull

```dart
Condition isNotNull();
```

필드가 null이 아닐 때 참입니다.

## 값 필드의 메서드

`BoolField`, `IntField`, `FloatField`, `StringField`, `BytesField`는 필드 타입 `V`의 값을 받습니다.

### equals

```dart
Condition equals(V value);
```

필드가 `value`와 같을 때 참입니다.

### notEquals

```dart
Condition notEquals(V value);
```

필드가 `value`와 다르거나 null일 때 참입니다.

### isIn

```dart
Condition isIn(List<V> values);
```

필드가 `values` 중 하나와 같을 때 참입니다.

### set

```dart
Change set(V? value);
```

필드를 `value`로 바꾸는 변경입니다. null을 주면 선택 필드는 null이 되고, 기본값이 있는 필드는 기본값이 됩니다.

## 순서가 있는 필드의 메서드

`IntField`, `FloatField`, `StringField`, `BytesField`에는 순서가 있습니다. 숫자는 값으로, 문자열과 바이트는 바이트 순서로 비교합니다.

### lessThan

```dart
Condition lessThan(V value);
```

필드가 `value`보다 작을 때 참입니다.

### atMost

```dart
Condition atMost(V value);
```

필드가 `value` 이하일 때 참입니다.

### greaterThan

```dart
Condition greaterThan(V value);
```

필드가 `value`보다 클 때 참입니다.

### atLeast

```dart
Condition atLeast(V value);
```

필드가 `value` 이상일 때 참입니다.

### between

```dart
Condition between(V low, V high);
```

필드가 `low` 이상 `high` 이하일 때 참입니다.

## 문자열 필드의 메서드

### contains

```dart
Condition contains(String text);
```

필드에 `text`가 들어 있을 때 참입니다.

### startsWith

```dart
Condition startsWith(String text);
```

필드가 `text`로 시작할 때 참입니다.

### endsWith

```dart
Condition endsWith(String text);
```

필드가 `text`로 끝날 때 참입니다.

## 목록 필드의 메서드

목록에 건 조건은 원소 하나라도 맞으면 참입니다. `ListField<E>`와 `StringListField`에 있습니다.

### contains

```dart
Condition contains(E value);
```

목록에 `value`와 같은 원소가 있을 때 참입니다. 링크 목록에서는 `value`가 가리키는 객체의 키입니다.

### containsAny

```dart
Condition containsAny(List<E> values);
```

원소 하나라도 `values` 중 하나와 같을 때 참입니다.

### anyStartsWith

```dart
Condition anyStartsWith(String text);
```

`StringListField`에만 있습니다. 원소 하나라도 `text`로 시작할 때 참입니다.

### anyEndsWith

```dart
Condition anyEndsWith(String text);
```

`StringListField`에만 있습니다. 원소 하나라도 `text`로 끝날 때 참입니다.

### set

```dart
Change set(List<E>? values);
```

목록을 `values`로 바꾸거나 null로 만드는 변경입니다.

## 내장 객체 필드의 메서드

`EmbeddedField<E>`는 클래스 `E`의 내장 객체를 담는 필드이고, 하위 클래스에는 그 클래스의 필드가 있어서 조건이 그것을 거쳐 읽을 수 있습니다.

### set

```dart
Change set(E? value);
```

내장 객체를 `value`로 통째로 바꾸거나 null로 만드는 변경입니다. 기본값이 있는 필수 필드에 null을 주면 기본값이 됩니다. 객체의 필드는 파일이 기록해 둔 자리에 쓰므로, 클래스가 파일과 다른 순서로 필드를 선언해도 필드마다 제자리에 들어갑니다.

## 링크 필드의 메서드

`LinkField`는 링크에 담긴 키를 대상 컬렉션의 키 타입으로 비교합니다. 하위 클래스에는 대상 컬렉션의 필드도 있어서, 조건이 링크를 거쳐 읽을 수 있습니다.

### equals

```dart
Condition equals(Object key);
```

링크에 `key`가 담겨 있을 때 참입니다.

### notEquals

```dart
Condition notEquals(Object key);
```

링크에 `key`가 담겨 있지 않거나 null일 때 참입니다.

### isIn

```dart
Condition isIn(List<Object> keys);
```

링크에 `keys` 중 하나가 담겨 있을 때 참입니다.

### set

```dart
Change set(Link<Object?>? link);
```

링크를 바꾸거나 null로 만드는 변경입니다.
