---
title: 필드 타입
order: 3
counterpart: /types/rust/field-type
---

# 필드 타입

필드의 어노테이션이 파일에 담길 값과 필드가 받는 Python 값, 그리고 파일에서 읽은 객체의 그 필드에 들어갈 값을 정합니다.

| 어노테이션 | 파일에서 | 받는 값 | 읽을 때 |
| --- | --- | --- | --- |
| `bool` | 불리언 | `bool` | `bool` |
| `int` | 64비트 정수 | -2^63부터 2^63 - 1까지의 `int`. `bool`은 받지 않습니다 | `int` |
| `float` | 64비트 부동소수점 수 | `float`나 `int` | `float` |
| `str` | UTF-8 문자열 | `str` | `str` |
| `bytes` | 바이트 | `bytes`, `bytearray`, `memoryview` | `bytes` |
| 위 타입의 `list[E]` | 목록 | `str`과 바이트를 뺀, `E`를 담은 반복할 수 있는 값 | `list` |
| `field(link=...)`를 붙인 `int`, `str`, `bytes` | 링크 | 가리키는 객체의 키 | 키 |
| `field(link=...)`를 붙인 `list[int]`, `list[str]`, `list[bytes]` | 링크의 목록 | 키를 담은 반복할 수 있는 값 | 키의 `list` |
| `@embedded`를 붙인 클래스 | 내장 객체 | 그 클래스의 인스턴스 | 그 클래스의 인스턴스 |
| 위 타입에 `\| None`을 붙인 것 | 같은 타입의 선택 필드 | 같은 값이나 `None` | 같은 값이나 `None` |

필드와 타입이 다른 값은 객체를 쓸 때 `INVALID_ARGUMENT`로 실패합니다. 그 밖의 어노테이션은 [Schema](../../api/python/schema.md)를 만들 때 `INVALID_ARGUMENT`로 실패합니다. `dict`, 원소 타입이 없는 `list`, 두 타입의 유니언 타입, 목록의 목록이나 내장 객체의 목록, 원소가 `None`일 수 있는 목록이 그 예입니다.

```python
import darudb
from darudb import field


@darudb.embedded
class Address:
    city: str
    zip: str | None = None


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    rating: float = 0.0
    active: bool = True
    tags: list[str] = field(default_factory=list)
    avatar: bytes | None = None
    friends: list[int] = field(default_factory=list, link="users")
    address: Address | None = None
```

## 필수, 선택, 기본값

- **필수**: `name: str`처럼 `| None`이 아니고 기본값이 없는 필드입니다. 모든 객체에 있고, `None`을 넣으면 `INVALID_ARGUMENT`로 실패합니다.
- **선택**: `email: str | None = None`처럼 `X | None`이나 `Optional[X]`로 어노테이션한 필드입니다. `None`일 수 있고, 필드가 생기기 전에 쓴 레코드는 `None`으로 읽습니다.
- **기본값**: `age: int = 0`처럼 `| None`이 아니고 기본값이 있는 필드입니다. 필수이며, 필드가 생기기 전에 쓴 레코드는 기본값으로 읽습니다. 이 필드에 `None`을 쓰면 기본값이 저장됩니다.

기본값은 파일에 저장할 수 있는 상수여야 합니다. `bool`, `int`, `float`, `str`, `bytes`나 이들의 목록입니다. `float` 필드의 `int` 기본값은 `float`로 저장됩니다. 선택 필드의 기본값은 `None`뿐입니다.

## 값

- **정수**는 64비트입니다. Python은 `bool`을 `int`로 치지만, `int` 필드와 `float` 필드는 `bool`을 거부합니다.
- **부동소수점 수** 필드는 `int`도 받습니다. Python 코드는 정수를 `int`로 쓰는 일이 흔하기 때문이며, 다시 읽으면 `float`입니다.
- **문자열**은 UTF-8로 저장하고 바이트로 비교합니다.
- **바이트**는 Python이 바이트를 담는 세 타입 중 어느 것으로든 넘길 수 있고, 따로 만든 `bytes`로 돌아옵니다.
- **목록**은 `str`과 바이트를 뺀 반복할 수 있는 값이면 받습니다. `str`과 바이트를 받으면 글자나 숫자의 목록으로 읽히기 때문입니다. 목록은 새 `list`로 돌아오고, 목록에는 `None`이 들어가지 않습니다.
- **내장 객체**는 그 클래스나 하위 클래스의 인스턴스로 넘기고, `__init__` 없이 만든 그 클래스의 인스턴스로 돌아옵니다. `dict`는 거부합니다.
- **링크**에는 대상 컬렉션에 있는 객체의 키를 그 컬렉션의 키 타입으로 담고, 키로 돌아옵니다. 없는 객체를 가리키는 링크도 허용됩니다.
