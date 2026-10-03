---
title: DaruException
order: 14
counterpart: /types/rust/error
---

# DaruException

`DaruException`은 패키지가 데이터베이스의 모든 실패에 던지는 예외로, 메시지와 함께 엔진의 오류 코드 가운데 하나를 담습니다.

```dart
final class DaruException implements Exception {
  const DaruException(this.code, this.message);

  final String code;
  final String message;
}
```

코드마다 언제 생기고 어떻게 대처하는지는 [오류](../../guide/errors.md)에 있습니다.

```dart
try {
  Database.open('app.darudb', create: false);
} on DaruException catch (error) {
  if (error.code != 'NOT_FOUND') {
    rethrow;
  }

  // 경로에 아무것도 없습니다.
}
```

## 속성

### code

```dart
final String code;
```

`DUPLICATE_KEY`처럼 `SCREAMING_SNAKE_CASE`로 쓴 실패 코드입니다. 모든 언어에서 같은 문자열이고, 한 번 릴리스한 코드는 이름을 바꾸지 않으므로 프로그램이 믿고 써도 됩니다.

### message

```dart
final String message;
```

무엇이 잘못됐는지 사람이 읽으라고 쓴 설명입니다. 메시지는 릴리스마다 문구가 바뀔 수 있으니, 메시지가 아니라 `code`로 비교하세요.

## 오류가 생기는 곳

- **엔진.** 엔진 안에서 난 실패는 엔진의 코드와 메시지를 그대로 담고 옵니다. 호출이 부른 isolate에서 돌았든 네이티브 라이브러리의 스레드에서 돌았든 같습니다. 버그로만 생길 수 있는 네이티브 라이브러리의 패닉은 `INTERNAL`로 옵니다.
- **패키지의 검사.** 패키지는 받은 값이 엔진에 닿기 전에 먼저 검사하고, 실패하면 엔진의 코드를 씁니다. 옵션이나 객체, 키가 맞지 않으면 `INVALID_ARGUMENT`, 쿼리나 매개변수가 맞지 않으면 `INVALID_QUERY`, 파일에서 읽은 레코드를 해석할 수 없으면 `CORRUPTED`입니다.
- **직접 넘긴 함수.** 트랜잭션 함수나 마이그레이션 함수가 던진 오류는 트랜잭션을 커밋하지 않고 끝낸 뒤 `read`, `write`, `open`과 그 `Async` 짝에서 던진 그대로 나옵니다.

## CLOSED

- `close`나 `closeAsync`로 닫은 뒤에는 [Database](../../api/dart/database.md)의 멤버 가운데 `path`, `isOpen`, `close`, `closeAsync`를 뺀 모든 것이 `CLOSED`를 던지고, 비동기 메서드는 이 오류로 실패합니다. 다시 닫으면 아무 일도 일어나지 않습니다.
- 함수가 반환한 뒤에 트랜잭션이나 거기서 얻은 컬렉션을 쓰면 `CLOSED`를 던지고, `Future` API에서는 이 오류로 실패합니다. 트랜잭션 밖에서 쓸 것은 컬렉션이 아니라 읽은 객체로 남겨 두세요. 객체는 트랜잭션이 끝난 뒤에도 쓸 수 있는 값입니다.

## Future API

`Future`를 돌려주는 메서드는 예외를 던지지 않습니다. 인자를 거부할 때를 포함해 실패하면, 동기 버전이 던지는 것과 같은 `DaruException`으로 `Future`가 완료됩니다. 비동기 트랜잭션에서 거부된 호출은 자기 `Future`만 실패시키고 아무것도 바꾸지 않으며, 그 뒤에 부른 호출은 그대로 실행됩니다. 트랜잭션 함수가 그 오류를 밖으로 흘려보내면 트랜잭션은 취소되고 `writeAsync`가 같은 오류로 실패합니다.

```dart
try {
  await db.writeAsync((txn) async {
    await txn.collection(userSchema).insert(const User(name: 'Alice', email: 'alice@example.com'));
  });
} on DaruException catch (error) {
  if (error.code != 'DUPLICATE_KEY') {
    rethrow;
  }

  // 이미 쓰는 email입니다. 커밋된 것은 없습니다.
}
```
