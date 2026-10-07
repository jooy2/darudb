---
title: PasswordHashing
order: 5
---

# PasswordHashing

`PasswordHashing`은 비밀번호를 키로 해시하는 비용을 Argon2id가 세는 대로 메모리, 반복 횟수, 병렬도로 나타냅니다.

```python
@dataclasses.dataclass(frozen=True)
class PasswordHashing:
    memory_kib: int = 19456
    iterations: int = 2
    parallelism: int = 1
```

[`Database.open`](../../api/python/database.md#open)과 `backup`의 `password_hashing` 옵션이 이 값을 받습니다. 메모리와 반복을 늘리면 공격자가 비밀번호를 맞히는 데 시간이 더 들지만, 데이터베이스를 여는 데도 그만큼 더 걸립니다. 기본값은 요즘 컴퓨터에서 수십 밀리초가 걸리고, 모바일 앱 확장의 메모리 한도 안에 들어갑니다. 필드마다 기본값이 있으므로 `PasswordHashing(memory_kib=65536)`은 메모리만 바꿉니다.

```python
import darudb
from darudb import PasswordHashing

db = darudb.Database.open(
    "secret.darudb",
    password="correct horse battery staple",
    password_hashing=PasswordHashing(memory_kib=65536, iterations=3),
)
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `memory_kib` | `int` | KiB 단위 메모리. `parallelism`의 레인 하나에 8부터 1GiB(1048576)까지이고, 기본값은 19456입니다 |
| `iterations` | `int` | 메모리를 훑는 횟수. 1부터 1024까지이고, 기본값은 2입니다 |
| `parallelism` | `int` | 레인 수. 1부터 64까지이고, 기본값은 1입니다 |

`-1`, `1.5`, `True`처럼 0 이상의 정수가 아닌 필드는 `PasswordHashing`을 만들 때 `INVALID_ARGUMENT`로 실패합니다. 범위를 벗어난 값은 옵션을 쓸 때 `INVALID_ARGUMENT`로 실패합니다. 옵션은 주기만 하면 비밀번호가 없어도 씁니다.

## 적용되는 때

- **`password`로 새 데이터베이스를 만들 때.** 파일은 키와 함께 이 비용을 기록합니다.
- **`set_password`와 `set_password_async`.** 새 비밀번호는 이 프로세스가 파일을 열 때 이 옵션으로 준 비용으로 해시합니다. 파일을 여러 번 열었다면 처음 연 핸들의 비용이고, 파일은 그때부터 이 비용을 기록합니다. 옵션을 주지 않았다면 파일에 전에 어떤 비용이 기록돼 있었든 기본 비용을 씁니다.
- **새 `password`로 백업할 때.** 사본에는 백업에 준 `password_hashing`의 비용이 기록됩니다.
- **파일을 열 때는 적용되지 않습니다.** 암호화한 파일은 파일에 기록된 비용으로 엽니다. 그래서 애플리케이션의 새 릴리스에서 비용을 올려도 예전에 만든 파일은 그대로 열립니다. [`Database.salvage`](../../api/python/database.md#salvage)도 마찬가지여서 비용을 받지 않습니다.

비밀번호에서 얻은 키가 파일을 어떻게 지키는지는 [암호화](../../guide/encryption.md)에 있습니다.
