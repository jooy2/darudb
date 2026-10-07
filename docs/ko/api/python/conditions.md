---
title: conditions
order: 11
counterpart: [/api/rust/filter, /api/dart/fields]
---

# conditions

`F`는 쿼리가 검사하는 객체의 필드를 가리키고, 필드를 비교하면 `Condition`이 생기며, 조건은 `&`, `|`, `~`로 다른 조건과 엮입니다.

```python
F: Final[_Fields]


class FieldRef:
    def __getattr__(self, name: str) -> FieldRef: ...
    def __getitem__(self, name: str) -> FieldRef: ...


class Condition:
    def __and__(self, other: Condition) -> Condition: ...
    def __or__(self, other: Condition) -> Condition: ...
    def __invert__(self) -> Condition: ...
    def query(self) -> Query: ...
```

`F.age`는 필드 `age`를 가리키는 `FieldRef`이고, `F.age >= 18`은 그 필드에 건 조건입니다. [`where`](./query.md#where)가 조건을 받고, `find`, `find_one`, `count`도 조건을 받아 그 조건 하나만 있는 쿼리로 칩니다. 조건은 특정 컬렉션에 묶이지 않습니다. 쿼리를 실행할 때 컬렉션의 스키마와 대조하며, 컬렉션에 없는 필드나 타입이 다른 값은 그때 `INVALID_QUERY`로 실패합니다.

```python
from darudb import F

young = (F.age >= 18) & (F.age < 30)

with db.read() as txn:
    found = txn.collection(User).find(young | F.email.is_null())
```

Python의 `&`, `|`, `~`는 비교 연산자보다 먼저 묶이므로, 이들로 잇는 비교는 `(F.age >= 18) & (F.age < 30)`처럼 하나하나 괄호로 감쌉니다.

- **경로.** 필드는 이름으로 가리키고, 내장 객체나 링크를 지나 닿을 수도 있습니다. `F.address.city`처럼 쓰고, `F.team.city`처럼 쓰면 링크가 가리키는 객체를 검사합니다. 가리키는 객체가 없으면 `None`으로 읽습니다. 경로에는 이름이 32개까지 들어갈 수 있습니다.
- **속성 이름.** 경로는 필드마다 Python 속성 이름으로 가리키고, 패키지는 내장 클래스와 링크된 컬렉션까지 따라가며 엔진에는 파일에 저장된 이름을 넘깁니다. 그래서 `field(name=...)`로 선언한 필드도 쿼리에서는 속성 이름으로 씁니다. 클래스의 속성이 아닌 이름은 그대로 엔진에 갑니다.
- **목록.** 목록에 건 조건은 원소 하나라도 맞으면 참입니다. 목록에 `contains`를 쓰면 그 원소가 있는지 봅니다. 빈 목록에는 원소가 없으므로 `is_not_null`만 참입니다.
- **None.** `None`인 필드에 건 조건은 `is_null`을 빼고 모두 거짓입니다. `F.email == None`은 `F.email.is_null()`과, `F.email != None`은 `F.email.is_not_null()`과 같습니다. 그 밖의 검사에 `None`을 쓰면 그 자리에서 `INVALID_QUERY`로 실패합니다. 내장 객체 자체는 `None`인지도 검사하지 않고, 조건은 언제나 그 필드 중 하나를 검사합니다.
- **타입.** 값은 필드의 타입과 맞아야 합니다. `int` 필드는 `int`와만 비교하고 `float`나 `bool`과는 비교하지 않으며, `float` 필드는 어떤 수와도 비교합니다. 링크는 대상 컬렉션의 키와 비교합니다. 값은 `bool`, `int`, `float`, `str` 하나이거나, `bytes`, `bytearray`, `memoryview`로 준 바이트, 또는 [param](./param.md)입니다. 목록이나 그 밖의 값은 쿼리를 실행할 때 `INVALID_QUERY`로 실패합니다.
- **중첩.** 필터는 24단계까지만 중첩할 수 있습니다. `&` 안의 `&`와 `|` 안의 `|`는 한 단계로 합쳐지므로, `~`와 번갈아 나오는 묶음만 단계로 셉니다.

## F

```python
F: Final[_Fields]
```

모든 필드 경로의 출발점입니다. `F`의 속성은 그 이름의 필드이고, 필드의 속성은 그 안의 필드입니다. `F.address.city`처럼 씁니다. `F["name"]`도 같은 일을 하며, 아래 메서드처럼 `FieldRef` 자체에 있는 속성과 이름이 같거나 밑줄 두 개로 시작하는 이름에 씁니다. `F.address["contains"]`는 `address`의 필드 `contains`입니다.

## FieldRef

```python
class FieldRef:
    def __getattr__(self, name: str) -> FieldRef: ...
    def __getitem__(self, name: str) -> FieldRef: ...
```

쿼리가 검사하는 객체의 필드를 경로로 나타내며, `F`가 만듭니다. 비교하면 `bool` 값 대신 조건이 나오므로, `FieldRef`는 해시할 수 없어서 `dict`의 키나 `set`의 원소가 될 수 없습니다. `repr`은 `F.address.city`처럼 경로입니다.

### 비교

```python
def __eq__(self, value: object) -> Condition: ...
def __ne__(self, value: object) -> Condition: ...
def __lt__(self, value: object) -> Condition: ...
def __le__(self, value: object) -> Condition: ...
def __gt__(self, value: object) -> Condition: ...
def __ge__(self, value: object) -> Condition: ...
```

`==`, `!=`, `<`, `<=`, `>`, `>=`로 값과 비교하면 조건이 생깁니다. 필드가 값과 같다, 다르다, 작다는 식의 조건입니다. 필드는 어느 쪽에 와도 되므로 `18 <= F.age`는 `F.age >= 18`과 같습니다. `18 <= F.age < 30` 같은 연쇄 비교는 Python이 두 비교를 `and`로 잇기 때문에 `TypeError`가 납니다. `(F.age >= 18) & (F.age < 30)`이나 `F.age.between(18, 29)`로 쓰세요.

### between

```python
def between(self, low: object, high: object) -> Condition: ...
```

필드가 `low` 이상 `high` 이하입니다.

### is_in

```python
def is_in(self, values: Iterable[object]) -> Condition: ...
```

필드가 `values` 중 하나와 같습니다. `values`는 반복할 수 있는 값이면 되고, 비어 있으면 아무것도 맞지 않습니다.

### contains

```python
def contains(self, value: object) -> Condition: ...
```

문자열 필드가 `value`를 포함하거나, 목록에 `value`라는 원소가 있습니다.

### startswith

```python
def startswith(self, value: object) -> Condition: ...
```

문자열 필드가 `value`로 시작합니다. 문자열 목록이면 원소 하나라도 `value`로 시작할 때 참입니다.

### endswith

```python
def endswith(self, value: object) -> Condition: ...
```

문자열 필드가 `value`로 끝납니다. 문자열 목록이면 원소 하나라도 `value`로 끝날 때 참입니다.

### is_null

```python
def is_null(self) -> Condition: ...
```

필드가 `None`입니다. 목록은 목록 자체가 `None`일 때만 `None`이고, 비어 있다고 `None`이 되지는 않습니다.

### is_not_null

```python
def is_not_null(self) -> Condition: ...
```

필드가 `None`이 아닙니다.

## Condition

```python
class Condition:
    def __and__(self, other: Condition) -> Condition: ...
    def __or__(self, other: Condition) -> Condition: ...
    def __invert__(self) -> Condition: ...
    def query(self) -> Query: ...
```

필드 하나에 건 검사이거나, 여러 검사를 엮은 것입니다. 한 번 만든 조건은 바뀌지 않으므로 여러 쿼리에 넣어도 됩니다.

조건에는 진릿값이 없습니다. `bool`로 바꾸면 `TypeError`가 나고, 진릿값을 묻는 `and`, `or`, `not`, `in`도 마찬가지입니다. 조건은 `&`, `|`, `~`로 엮으세요. 조건이 아닌 값과 `&`나 `|`로 엮어도 `TypeError`가 납니다.

### & (AND)

```python
def __and__(self, other: Condition) -> Condition: ...
```

두 조건이 모두 참입니다.

### | (OR)

```python
def __or__(self, other: Condition) -> Condition: ...
```

조건 중 하나 이상이 참입니다.

### ~ (NOT)

```python
def __invert__(self) -> Condition: ...
```

조건이 거짓입니다.

### query

```python
def query(self) -> Query: ...
```

이 조건 하나만 있는 [Query](./query.md)입니다. 처음 요청할 때 만들어 기억해 두며, `find`가 조건을 받으면 이 쿼리를 실행합니다. 기억해 둔 쿼리이므로 같은 조건을 다시 실행해도 다시 컴파일하지 않습니다.
