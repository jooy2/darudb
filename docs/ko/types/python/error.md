---
title: DaruError
order: 1
---

# DaruError

`DaruError`는 패키지가 데이터베이스의 모든 실패에 일으키는 단 하나의 예외로, 메시지와 함께 엔진의 오류 코드 가운데 하나를 담습니다.

```python
class DaruError(Exception):
    code: str
    message: str

    def __init__(self, code: str, message: str) -> None: ...
```

평범한 `Exception`의 하위 클래스이고, `darudb.DaruError`로 가져옵니다. `str`로 바꾸면 `CODE: message` 꼴이 되고, `args`는 `(code, message)`입니다. 코드마다 언제 생기고 어떻게 대처하는지는 [오류](../../guide/errors.md)에 있습니다.

```python
import darudb

try:
    darudb.Database.open("app.darudb", create=False)
except darudb.DaruError as error:
    if error.code != "NOT_FOUND":
        raise

    # 경로에 아무것도 없습니다.
```

## 속성

### code

```python
code: str
```

`DUPLICATE_KEY`처럼 `SCREAMING_SNAKE_CASE`로 쓴 실패 코드입니다. 모든 언어에서 같은 문자열이고, 한 번 릴리스한 코드는 이름을 바꾸지 않으므로 프로그램이 믿고 써도 됩니다.

### message

```python
message: str
```

무엇이 잘못됐는지 사람이 읽으라고 쓴 설명입니다. 메시지는 릴리스마다 문구가 바뀔 수 있으니, 메시지가 아니라 `code`로 비교하세요.

## 오류가 생기는 곳

- **엔진.** 엔진 안에서 난 실패는 엔진의 코드와 메시지를 그대로 담고 옵니다. 호출이 부른 스레드에서 돌았든 패키지의 스레드 풀에서 돌았든 같습니다.
- **패키지의 검사.** 패키지는 받은 값이 엔진에 닿기 전에 먼저 검사하고, 실패하면 엔진의 코드를 씁니다. 옵션이나 클래스, 객체, 키가 맞지 않으면 `INVALID_ARGUMENT`, 쿼리나 매개변수가 맞지 않으면 `INVALID_QUERY`입니다. 엔진이 담을 수 없는 Python 값도 `TypeError`가 아니라 `INVALID_ARGUMENT`로 실패하므로, `except` 하나로 모든 거부를 잡을 수 있습니다.
- **직접 넘긴 함수.** 트랜잭션 블록이나 마이그레이션 함수가 일으킨 오류는 트랜잭션을 커밋하지 않고 끝낸 뒤 `with` 문이나 `open`에서 일으킨 그대로 나옵니다.

Python 자체가 일으키는, `DaruError`가 아닌 오류도 몇 가지 있습니다. [조건](../../api/python/conditions.md)을 진릿값으로 쓰거나 `Database()`를 부르면 `TypeError`가 납니다. 데코레이터를 붙인 클래스는 데이터 클래스이므로 데이터 클래스의 오류도 그대로 납니다. 필수 필드를 빼고 생성자를 부르면 `TypeError`가, 필드에 값을 대입하면 `dataclasses.FrozenInstanceError`가 나는 것이 그 예입니다.

## CLOSED

- `close`나 `close_async`로 닫은 뒤에는 [Database](../../api/python/database.md)에서 파일에 접근하는 멤버가 비동기 멤버까지 모두 `CLOSED`를 일으킵니다. `path`, `schema`, `schema_version`, `is_open`은 그대로 읽을 수 있고, 다시 닫으면 아무 일도 일어나지 않습니다.
- 블록이 끝난 뒤에 트랜잭션이나 거기서 얻은 컬렉션을 쓰면 `CLOSED`를 일으킵니다. 트랜잭션 밖에서 쓸 것은 컬렉션이 아니라 읽은 객체로 남겨 두세요. 객체는 트랜잭션이 끝난 뒤에도 남는 클래스의 인스턴스입니다.

## 비동기 호출

패키지의 코루틴은 인자를 거부할 때든 엔진이 실패할 때든 await한 자리에서 `DaruError`를 일으킵니다. 비동기 트랜잭션에서 거부된 작업은 아무것도 바꾸지 않고, 그 뒤의 작업은 그대로 실행됩니다. 블록이 오류를 밖으로 흘려보내면 트랜잭션은 취소되고, 오류는 `async with` 문 밖으로 나갑니다.

```python
async def main() -> None:
    try:
        async with db.write_async() as txn:
            await txn.collection(User).insert(User(name="Alice", email="alice@example.com"))
    except darudb.DaruError as error:
        if error.code != "DUPLICATE_KEY":
            raise

        # 이미 쓰는 email입니다. 커밋된 것은 없습니다.
```
