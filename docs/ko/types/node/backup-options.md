---
title: BackupOptions
order: 15
---

# BackupOptions

`BackupOptions`는 `Database.backup`이 경로와 함께 받는 옵션으로, 사본을 새 데이터 키로 암호화할 키나 비밀번호를 정합니다.

```ts
interface BackupOptions
```

[`backup`과 `backupAsync`](../../api/node/database.md#backup)가 마지막 인자로 받고, 필드는 모두 생략할 수 있습니다. 키나 비밀번호가 없으면 암호화한 파일의 사본은 데이터 키를 그대로 두므로 같은 키나 비밀번호로 열립니다. 키나 비밀번호를 주면 사본은 무작위로 만든 새 데이터 키로 암호화되고, 그 키나 비밀번호가 새 데이터 키를 감쌉니다. 사본은 그것으로만 열립니다. 파일의 키나 비밀번호를 바꾸면 데이터 키를 다시 감쌀 뿐 데이터 키 자체는 그대로이므로, 노출됐을 수 있는 데이터 키를 버리려면 새 키로 백업한 뒤 사본을 원래 파일 자리에 두면 됩니다. 평문 데이터베이스의 사본도 같은 방식으로 암호화됩니다.

```ts
const report = await db.backupAsync('rekeyed.darudb', { password: 'a new password' });
```

## 필드

| 필드 | 타입 | 설명 |
| --- | --- | --- |
| `key` | `Uint8Array` | 사본의 32바이트 키. 사본의 새 데이터 키를 감쌉니다 |
| `password` | `string \| Uint8Array` | 사본의 비밀번호. Argon2id로 해시해 사본의 새 데이터 키를 감싸는 키를 만듭니다 |
| `passwordHashing` | [`PasswordHashing`](./password-hashing.md) | 사본의 비밀번호를 해시하는 데 드는 일의 양. 기본값은 19456 KiB, 반복 2회, 병렬 1입니다 |

- 32바이트가 아닌 키, 빈 비밀번호, 함께 준 `key`와 `password`는 아무것도 쓰기 전에 `INVALID_ARGUMENT`로 실패합니다.
- 패키지는 호출하는 순간 키나 비밀번호를 복사합니다. 그래서 넘긴 버퍼는 `backup`이 반환하자마자 `fill(0)`으로 지워도 되고, `backupAsync`라면 promise가 끝나기 전, 호출이 반환되자마자 지워도 됩니다.
