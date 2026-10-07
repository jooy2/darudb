---
title: Database
order: 1
---

# Database

`Database`는 Python에서 연 DaruDB 파일을 나타내며, 프로그램은 이 객체로 트랜잭션을 실행하고 쿼리를 준비하고 파일 도구를 씁니다.

```python
class Database: ...
```

생성자는 없습니다. `Database.open`이나 `Database.open_async`가 만들어 돌려주고, `Database()`를 부르면 `TypeError`가 납니다. [스키마](./schema.md) 없이 연 데이터베이스에는 컬렉션이 없고, 스키마를 주고 열면 `txn.collection(User)`처럼 클래스로 컬렉션에 접근합니다.

`close`나 `close_async`로 닫기 전까지 쓸 수 있습니다. 닫은 뒤에도 `path`, `schema`, `schema_version`, `is_open`은 읽을 수 있지만, 파일에 접근하는 멤버는 모두 코드가 `CLOSED`인 [DaruError](../../types/python/error.md)를 일으킵니다. 프로세스가 이미 연 파일을 다시 열면 같은 데이터베이스의 핸들이 하나 더 생기고, 페이지 캐시도 함께 씁니다. 스키마는 핸들마다 열 때 받은 것을 계속 씁니다.

네이티브 모듈은 엔진이 일하는 동안 언제나 GIL을 놓으므로, 한 스레드가 디스크나 다른 쓰기를 기다려도 다른 스레드는 계속 돕니다. 데이터베이스는 여러 스레드에서 동시에 쓸 수 있고, 트랜잭션은 어느 스레드에서든 한 번에 호출 하나씩 쓸 수 있습니다. 파일에 접근하는 메서드에는 이름이 `_async`로 끝나는 `asyncio`용 짝이 있습니다. 짝 메서드는 엔진의 일을 패키지가 따로 두는 스레드 풀에서 하므로, 이벤트 루프가 디스크나 다른 프로세스의 쓰기를 기다리지 않습니다. 이 풀의 스레드는 많아야 `min(32, os.cpu_count() + 4)`개입니다. 두 방식이 한 파일을 어떻게 함께 쓰는지는 [비동기 API](../../guide/async.md)에서 설명합니다.

## 클래스 메서드

### open

```python
@classmethod
def open(
    cls,
    path: str | os.PathLike[str],
    *,
    schema: Schema | None = None,
    migrations: Sequence[Migration] = (),
    create: bool = True,
    page_size: int | None = None,
    busy_timeout: float | None = None,
    cache_size: int | None = None,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> Database: ...
```

`path`의 데이터베이스를 엽니다. 그 자리에 아무것도 없으면 새로 만들고, 스키마를 저장하거나 비교하거나 마이그레이션합니다.

| 옵션 | 설명 |
| --- | --- |
| `schema` | 파일에 담을 컬렉션을 나타내는 [Schema](./schema.md). 처음 열 때 저장하고, 그 뒤로는 비교하며, 더 낮은 버전을 담은 파일은 마이그레이션합니다 |
| `migrations` | [Migration](./migration.md) 단계 목록. 엔진이 알아서 하는 것 말고도 할 일이 있는 버전마다 하나씩 둡니다 |
| `create` | 없는 파일을 만들지 여부. `False`면 만들지 않고 `NOT_FOUND`로 실패합니다 |
| `page_size` | 새 파일의 페이지 크기. 4096부터 65536 사이의 2의 거듭제곱이며 기본값은 4096입니다. 이미 있는 파일은 자기 페이지 크기를 씁니다 |
| `busy_timeout` | 파일을 열 때와 쓸 때 다른 쓰기를 기다리다 `BUSY`로 실패하기까지의 시간. 단위는 초이며 기본값은 5입니다 |
| `cache_size` | 페이지 캐시가 쓸 메모리. 단위는 바이트이며 기본값은 32 MiB입니다 |
| `key` | 32바이트 키. 새 파일을 암호화하고, 암호화한 파일을 엽니다 |
| `password` | 비밀번호. Argon2id로 만든 키로 `key`와 같은 일을 합니다. 키와 비밀번호를 함께 주면 `INVALID_ARGUMENT`로 실패합니다 |
| `password_hashing` | 비밀번호에서 키를 만드는 비용. 새 파일과 `set_password`에 씁니다. [PasswordHashing](../../types/python/password-hashing.md)을 보세요 |

마이그레이션 함수는 이 호출 안에서 버전 단계마다 차례로 실행되며 [Migrating](./migrating.md)을 받습니다. 여기서는 평범한 함수여야 합니다. 코루틴 함수는 `INVALID_ARGUMENT`로 실패하고, 오류 메시지가 `open_async`를 쓰라고 알려 줍니다. 함수가 예외를 일으키면 파일은 예전 스키마와 데이터를 그대로 유지하고, `open`도 같은 예외를 일으킵니다.

- `NOT_FOUND`: `path`에 아무것도 없는데 `create`가 `False`입니다.
- `NOT_A_DATABASE`: DaruDB 데이터베이스가 아닌 파일입니다.
- `KEY_REQUIRED`, `WRONG_KEY`: 암호화된 파일인데 `key`도 `password`도 주지 않았거나, 준 것이 틀렸습니다.
- `SCHEMA_MISMATCH`: 파일에 버전은 같지만 내용이 다른 스키마가 있습니다. `SCHEMA_TOO_NEW`: 파일의 스키마 버전이 더 높습니다.
- `BUSY`: 다른 프로세스가 `busy_timeout`보다 오래 파일을 붙잡고 있거나, 되살리기가 파일을 쓰고 있습니다.
- `INVALID_ARGUMENT`: 쓸 수 없는 옵션이 있습니다. 4096부터 65536 사이의 2의 거듭제곱이 아닌 페이지 크기, 음수나 NaN인 `busy_timeout`, 32바이트가 아닌 키, 함께 준 키와 비밀번호, 빈 비밀번호, 이미 있는 평문 파일에 준 키나 비밀번호, 스키마 없이 준 마이그레이션, 같은 버전으로 가는 마이그레이션 둘, 엔진이 저장할 수 없는 스키마가 그 예입니다.

```python
import darudb


@darudb.collection("users")
class User:
    id: int | None = None
    name: str


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))
```

`bytearray`로 준 키나 비밀번호는 `open`이 반환되자마자 지워도 됩니다. `bytes`와 `str`은 지울 수 없어서, Python이 거둘 때까지 메모리에 남습니다.

### open_async

```python
@classmethod
async def open_async(
    cls,
    path: str | os.PathLike[str],
    *,
    schema: Schema | None = None,
    migrations: Sequence[Migration] = (),
    create: bool = True,
    page_size: int | None = None,
    busy_timeout: float | None = None,
    cache_size: int | None = None,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> Database: ...
```

패키지의 스레드 풀에서 실행하는 `open`입니다. 그래서 이벤트 루프는 파일이나 복구, 마이그레이션의 커밋을 기다리지 않습니다. 여기서는 마이그레이션의 `run`이 코루틴 함수여도 되고, 이 함수는 [AsyncMigrating](./migrating.md#asyncmigrating)을 받습니다. 평범한 함수도 됩니다. 한 단계는 함수가 반환하고 코루틴까지 끝난 뒤, 함수가 시작한 작업이 모두 끝나야 마무리됩니다. 실패하는 경우는 `open`과 같습니다. 키와 비밀번호는 풀에서 일이 실행될 때 읽으므로, `bytearray`는 `await`가 반환된 뒤에야 지워도 됩니다.

```python
db = await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User]))
```

## 정적 메서드

### salvage

```python
@staticmethod
def salvage(
    source: str | os.PathLike[str],
    target: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    busy_timeout: float | None = None,
) -> SalvageReport: ...
```

손상된 `source`의 데이터베이스에서 건질 수 있는 것을 `target`의 새 데이터베이스로 옮기고, 무엇을 건졌고 무엇을 건지지 못했는지 보고합니다. 파일을 여는 대신 페이지 단위로 읽으므로 열리지 않는 파일에도 쓸 수 있습니다. 파일에 기록된 가장 새 커밋에서 시작하고, 그 커밋에서 읽지 못한 부분은 같은 페이지의 옛 버전에서 가져옵니다. 인덱스는 모두 다시 만들므로 새 파일은 무결성 검사를 통과합니다. 암호화한 파일은 `key`나 `password`가 있어야 하고, 새 파일도 그것으로 열립니다. `busy_timeout`은 다른 프로세스가 파일을 닫기를 기다리는 시간입니다. 결과는 [SalvageReport](../../types/python/salvage-report.md)에, 언제 쓰는지는 [도구](../../guide/tools.md)에 있습니다.

- `BUSY`: 이 프로세스가 파일을 열고 있으면 곧바로 실패하고, 다른 프로세스가 열고 있으면 `busy_timeout`이 지난 뒤 실패합니다. 되살리는 동안 파일을 열어도 `BUSY`로 실패합니다.
- `INVALID_ARGUMENT`: `target`에 이미 무언가 있거나, 키와 비밀번호를 함께 주었습니다. 되살리기는 파일을 덮어쓰지 않습니다.
- `NOT_FOUND`: `source`에 아무것도 없습니다.
- `KEY_REQUIRED`: 암호화한 파일인데 키도 비밀번호도 주지 않았습니다.

### salvage_async

```python
@staticmethod
async def salvage_async(
    source: str | os.PathLike[str],
    target: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    busy_timeout: float | None = None,
) -> SalvageReport: ...
```

패키지의 스레드 풀에서 실행하는 `salvage`입니다.

## 속성

### path

```python
path: str
```

데이터베이스를 연 경로로, `open`에 준 경로를 `os.fspath`가 바꾼 문자열입니다. `close` 뒤에도 읽을 수 있습니다.

### schema

```python
schema: Schema | None
```

데이터베이스를 열 때 준 [Schema](./schema.md)이고, 주지 않았으면 `None`입니다. `close` 뒤에도 읽을 수 있습니다.

### is_open

```python
@property
def is_open(self) -> bool: ...
```

데이터베이스가 열려 있는지 나타냅니다. `close`나 `close_async`가 데이터베이스를 닫으면 `False`가 됩니다.

### page_size

```python
@property
def page_size(self) -> int: ...
```

파일의 페이지 크기이며 단위는 바이트입니다. 파일은 만들 때 정한 페이지 크기를 계속 씁니다.

### format_version

```python
@property
def format_version(self) -> int: ...
```

파일에 기록된 파일 형식 버전입니다. 이 빌드가 여는 파일이면 [FORMAT_VERSION](../../types/python/constants.md)과 같습니다.

### is_encrypted

```python
@property
def is_encrypted(self) -> bool: ...
```

파일이 암호화돼 있는지 나타냅니다.

### schema_version

```python
@property
def schema_version(self) -> int | None: ...
```

데이터베이스를 열 때 준 스키마의 버전이고, 스키마 없이 열었으면 `None`입니다. 파일이 열린 뒤에는 파일에 있는 버전과 같습니다. `schema`에서 가져오는 값이므로 `close` 뒤에도 읽을 수 있습니다.

## 메서드

### prepare

```python
def prepare(self, collection: type[T] | str, query: Query | Condition | str) -> Prepared[T]: ...
```

클래스나 이름으로 가리킨 `collection`에서 실행할 쿼리를 한 번 준비해 두고, 실행할 때마다 매개변수 값만 넘기게 합니다. 쿼리는 `$0`, `$1` 같은 매개변수를 쓴 쿼리 언어 문자열이거나, 값 자리에 [param](./param.md)을 넣어 만든 [Query](./query.md)나 [조건](./conditions.md)입니다. 준비한 쿼리인 [Prepared](../../types/python/prepared.md)는 데이터베이스나 트랜잭션에 묶이지 않으므로, 그 컬렉션이라면 동기든 비동기든 어느 트랜잭션에서나 실행할 수 있습니다.

스키마에 없는 컬렉션이거나 스키마 없이 연 데이터베이스이면 `INVALID_ARGUMENT`로, 해석할 수 없는 문자열이면 `INVALID_QUERY`로, 데이터베이스를 닫은 뒤라면 `CLOSED`로 실패합니다. 컬렉션에 없는 필드는 쿼리를 실행할 때 드러납니다.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))

with db.read() as txn:
    alice = txn.collection(User).find_one(by_email, "alice@example.com")
```

### read

```python
def read(self) -> _ReadScope: ...
```

읽기 트랜잭션을 컨텍스트 관리자로 돌려줍니다. `with db.read() as txn`은 [ReadTransaction](./read-transaction.md)을 주고, 이 트랜잭션은 블록이 끝날 때까지 커밋 하나를 봅니다. 시작할 때 쓰기를 기다리지 않습니다.

### write

```python
def write(self, *, durability: Durability = "sync") -> _WriteScope: ...
```

쓰기 트랜잭션을 컨텍스트 관리자로 돌려줍니다. `with db.write() as txn`은 [WriteTransaction](./write-transaction.md)을 주고, 블록이 끝나면 커밋하고 블록에서 예외가 나면 취소합니다. 기본으로는 커밋이 디스크에 기록된 뒤 반환하고, `durability="deferred"`를 주면 디스크를 기다리지 않습니다. 자세한 내용은 [Durability](../../types/python/durability.md)에 있습니다. 그 밖의 `durability`는 `write`를 부를 때 `INVALID_ARGUMENT`로 실패합니다.

- `BUSY`: 다른 스레드나 다른 프로세스의 쓰기가 `busy_timeout`보다 오래 파일을 붙잡고 있었습니다.
- `INVALID_ARGUMENT`: 이 스레드의 쓰기 블록이 이 핸들로든 다른 핸들로든 이미 파일을 쥐고 있거나, 이 스레드에서 도는 이벤트 루프의 비동기 쓰기가 파일을 쥐고 있습니다. 쓰기 트랜잭션은 겹칠 수 없고, 여기서 기다리면 이 쓰기가 끝나야 끝날 수 있는 쓰기를 기다리게 됩니다.
- `SYNC_FAILED`: 커밋의 디스크 동기화가 실패했습니다. 블록이 끝날 때 일어나며, 데이터베이스를 닫고 다시 열어야 합니다.

### read_async

```python
def read_async(self) -> AsyncReadScope: ...
```

`async with`로 쓰는 읽기 트랜잭션을 돌려줍니다. `async with db.read_async() as txn`은 [AsyncReadTransaction](./read-transaction.md#asyncreadtransaction)을 주고, 그 작업은 await로 기다리며 스레드 풀에서 실행됩니다. 읽기를 시작할 때는 쓰기를 기다리지 않으므로, 트랜잭션은 부른 스레드에서 시작합니다.

### write_async

```python
def write_async(self, *, durability: Durability = "sync") -> AsyncWriteScope: ...
```

`async with`로 쓰는 쓰기 트랜잭션을 돌려줍니다. `async with db.write_async() as txn`은 [AsyncWriteTransaction](./write-transaction.md#asyncwritetransaction)을 줍니다. 블록이 끝나면 블록에서 시작한 작업이 모두 끝난 뒤에 커밋하고, 블록에서 예외가 나면 취소합니다. 이 프로세스가 한 이벤트 루프에서 한 파일에 하는 비동기 쓰기는 차례를 지킵니다. 각 쓰기는 앞선 쓰기가 끝나기를 이벤트 루프에서 기다렸다가 그제야 풀의 스레드를 잡으므로, 기다리는 쓰기는 스레드를 잡지 않습니다.

이 스레드의 동기 쓰기 블록이 파일을 쥐고 있을 때와, 같은 파일에 하는 비동기 쓰기의 블록 안이나 그 블록 안에서 만든 태스크에서 await할 때는 `INVALID_ARGUMENT`로 실패합니다. 어느 쪽이든 자신을 감싼 쓰기를 기다리게 되기 때문입니다. 쓰기와 함께 차례를 기다리는 `sync_async`, `close_async`, `compact_async`, `set_key_async`, `set_password_async`도 그런 곳에서는 거부됩니다.

### check

```python
def check(self) -> CheckReport: ...
```

게시된 커밋을 빠짐없이 검사합니다. 모든 페이지가 검사값과 맞는지, 키가 순서대로 있는지, 항목 수가 맞는지, 모든 페이지가 사용 중이거나 비었거나 보류 중인 상태 중 정확히 하나인지, 모든 객체가 인덱스와 맞는지 봅니다. 예외를 일으키지 않고 찾은 문제를 모두 [CheckReport](../../types/python/check-report.md)에 담아 돌려주며, 다른 핸들과 프로세스가 쓰는 동안에도 읽습니다.

### check_async

```python
async def check_async(self) -> CheckReport: ...
```

스레드 풀에서 실행하는 `check`입니다. 쓰기를 기다리는 일이 없습니다.

### backup

```python
def backup(
    self,
    path: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> BackupReport: ...
```

게시된 커밋을 `path`의 새 파일로 복사합니다. 그동안 다른 핸들과 프로세스는 계속 쓸 수 있습니다. 사본에는 빈 공간이 없고, 페이지 크기는 원본과 같으며, 같은 키나 비밀번호로 열립니다. 파일을 덮어쓰지 않으므로 이미 있는 경로는 `INVALID_ARGUMENT`로 실패합니다. 결과는 [BackupReport](../../types/python/backup-report.md)에 있습니다.

32바이트 `key`나 `password`를 주면 사본은 무작위로 만든 새 데이터 키로 암호화되고, 그 키나 비밀번호로만 열립니다. 비밀번호를 해시하는 비용은 [`password_hashing`](../../types/python/password-hashing.md)이 정합니다. 파일의 키나 비밀번호를 바꾸면 데이터 키를 다시 감쌀 뿐 데이터 키 자체는 그대로입니다. 그래서 노출됐을 수 있는 데이터 키를 버리려면 새 키로 백업한 뒤 사본을 원래 파일 자리에 두면 됩니다. 평문 데이터베이스의 사본도 같은 방식으로 암호화됩니다. 32바이트가 아닌 키나 함께 준 키와 비밀번호는 아무것도 쓰기 전에 `INVALID_ARGUMENT`로 실패합니다. 백업을 만드는 방식은 [도구](../../guide/tools.md)에 있습니다.

### backup_async

```python
async def backup_async(
    self,
    path: str | os.PathLike[str],
    *,
    key: bytes | bytearray | memoryview | None = None,
    password: str | bytes | bytearray | None = None,
    password_hashing: PasswordHashing | None = None,
) -> BackupReport: ...
```

스레드 풀에서 실행하는 `backup`입니다. 키와 비밀번호는 `open_async`에서처럼 일이 실행될 때 읽습니다.

### compact

```python
def compact(self) -> CompactReport: ...
```

파일을 그 자리에서 줄입니다. 파일 끝쪽 페이지를 앞쪽 빈 페이지로 옮기고, 비게 된 끝을 파일 시스템에 돌려줍니다. 자체 쓰기 트랜잭션으로 일하므로 다른 쓰기처럼 쓰기를 기다립니다. 이 스레드가 같은 파일에 연 쓰기 블록 안에서는 어느 핸들로 부르든, 그리고 비동기 쓰기가 파일을 쥔 이벤트 루프의 스레드에서는 기다리지 않고 곧바로 `INVALID_ARGUMENT`로 거부됩니다. 어느 쪽이든 이 호출이 반환해야 끝날 수 있는 쓰기를 기다리게 되기 때문입니다. 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 옮기지 않습니다. 결과는 [CompactReport](../../types/python/compact-report.md)에 있습니다.

### compact_async

```python
async def compact_async(self) -> CompactReport: ...
```

스레드 풀에서 실행하는 `compact`입니다. 이 이벤트 루프가 그 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다.

### set_key

```python
def set_key(self, key: bytes | bytearray | memoryview) -> None: ...
```

암호화한 데이터베이스의 키를 32바이트 `key`로 바꿉니다. 페이지를 다시 암호화하지는 않으며, 반환한 뒤로는 옛 키나 비밀번호로 파일을 열 수 없습니다. 커밋을 하므로 쓰기를 기다리고, `compact`가 거부되는 곳에서는 똑같이 거부됩니다. 암호화하지 않은 데이터베이스이거나 키가 32바이트가 아니면 `INVALID_ARGUMENT`로 실패합니다. 키와 비밀번호는 [암호화](../../guide/encryption.md)에서 다룹니다.

### set_key_async

```python
async def set_key_async(self, key: bytes | bytearray | memoryview) -> None: ...
```

스레드 풀에서 실행하는 `set_key`입니다. 이 이벤트 루프가 그 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다. 키는 일이 실행될 때 읽으므로, `bytearray`는 `await`가 반환된 뒤에야 지워도 됩니다.

### set_password

```python
def set_password(self, password: str | bytes | bytearray) -> None: ...
```

암호화한 데이터베이스의 키를 `password`에서 Argon2id로 얻은 키로 바꿉니다. 해시 비용은 이 프로세스가 파일을 열 때 `password_hashing`으로 정한 값이고, 정하지 않았으면 기본값입니다. 거부되거나 실패하는 경우는 `set_key`와 같습니다.

### set_password_async

```python
async def set_password_async(self, password: str | bytes | bytearray) -> None: ...
```

스레드 풀에서 실행하는 `set_password`입니다. 이 이벤트 루프가 그 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다. 비밀번호는 `set_key_async`의 키처럼 일이 실행될 때 읽습니다.

### sync

```python
def sync(self) -> None: ...
```

어느 핸들이나 프로세스가 했든 지연 커밋을 포함한 모든 커밋을 디스크에 기록합니다. 쓰기를 기다릴 수 있으므로 `compact`가 거부되는 곳에서는 똑같이 거부됩니다.

### sync_async

```python
async def sync_async(self) -> None: ...
```

스레드 풀에서 실행하는 `sync`입니다. 이 이벤트 루프가 그 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다.

### close

```python
def close(self) -> None: ...
```

지연 커밋을 디스크에 기록하고 이 핸들을 닫습니다. 이미 닫은 데이터베이스를 닫으면 아무 일도 하지 않습니다. 쓰기를 기다릴 수 있으므로 `compact`가 거부되는 곳에서는 똑같이 거부되고, 거부되면 데이터베이스는 열린 채로 남습니다.

### close_async

```python
async def close_async(self) -> None: ...
```

스레드 풀에서 실행하는 `close`입니다. 이 이벤트 루프가 그 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다.

## 컨텍스트 관리자

```python
def __enter__(self) -> Self: ...
def __exit__(
    self,
    kind: type[BaseException] | None,
    error: BaseException | None,
    trace: TracebackType | None,
) -> None: ...
async def __aenter__(self) -> Self: ...
async def __aexit__(
    self,
    kind: type[BaseException] | None,
    error: BaseException | None,
    trace: TracebackType | None,
) -> None: ...
```

데이터베이스는 블록이 끝나면 블록에서 예외가 났든 안 났든 스스로 닫히고, 블록에서 난 예외는 그대로 밖으로 나갑니다. `with`는 `close`로, `async with`는 `close_async`로 닫습니다.

```python
with darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User])) as db:
    with db.write() as txn:
        txn.collection(User).insert(User(name="Alice"))


async def main() -> None:
    async with await darudb.Database.open_async("app.darudb", schema=darudb.Schema(1, [User])) as db:
        async with db.read_async() as txn:
            print(await txn.collection(User).count())
```
