---
title: Key
order: 2
---

# Key

`Key`는 Python이 주고받는 기본 키로, `int`, `str`, `bytes` 중 하나입니다.

```python
Key: TypeAlias = int | str | bytes
```

`get`, `update`, `delete`가 키를 받고, `insert`, `insert_many`, `put`, `put_many`는 쓴 객체의 키를 돌려주며, `previous_keys`도 키를 돌려줍니다. 링크 필드에는 가리키는 객체의 키가 들어갑니다. 컬렉션의 키가 셋 중 무엇인지는 [`field(primary_key=True)`](../../api/python/collection.md#기본-키)로 정한 키 필드가 정합니다.

| 키 필드           | 넘길 때                            | 돌려받을 때 |
| ----------------- | ---------------------------------- | ----------- |
| 없음, 엔진의 `id` | `int`                              | `int`       |
| `int` 필드        | `int`                              | `int`       |
| `str` 필드        | `str`                              | `str`       |
| `bytes` 필드      | `bytes`, `bytearray`, `memoryview` | `bytes`     |

- **정수**는 64비트로, -2^63부터 2^63 - 1까지입니다. 이 범위를 벗어나면 `INVALID_ARGUMENT`로 실패합니다. Python은 `bool`을 `int`로 치지만, `bool`은 키가 아닙니다.
- **문자열**은 `str`로 넘기고 `str`로 돌려받습니다.
- **바이트**는 Python이 바이트를 담는 세 타입 중 어느 것으로든 넘길 수 있고, `bytes`로 돌아옵니다. `Key`에는 `bytes`만 적혀 있으므로, 타입 검사기는 `bytearray`나 `memoryview`를 먼저 바꾸라고 요구합니다.

컬렉션의 키 필드와 타입이 다른 키는 `INVALID_ARGUMENT`로 실패하고, `None`이나 `float` 같은 값도 마찬가지입니다. 문자열 키와 바이트 키는 파일의 키에도 들어가야 합니다. 4096바이트 페이지에서는 인코딩한 길이가 957바이트까지이고, 인코딩하면 키보다 몇 바이트 길어집니다. 더 긴 키는 객체를 쓸 때 `INVALID_ARGUMENT`로 실패합니다.

```python
import darudb
from darudb import field


@darudb.collection("files")
class File:
    digest: bytes = field(primary_key=True)
    size: int


with darudb.Database.open("files.darudb", schema=darudb.Schema(1, [File])) as db:
    with db.write() as txn:
        files = txn.collection(File)
        key = files.insert(File(digest=b"\xca\xfe", size=2))  # b"\xca\xfe"

        files.get(key)
```
