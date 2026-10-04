---
title: BackupOptions
order: 19
counterpart: /types/node/backup-options
---

# BackupOptions

`BackupOptions`는 [`Database::backup_with`](./database.md#backup-with)가 사본을 쓸 때 쓰는 옵션입니다. 사본을 새 데이터 키로 암호화할 키나 비밀번호와, 그 비밀번호를 해시하는 비용을 정합니다.

```rust
#[derive(Debug, Clone)]
pub struct BackupOptions
```

키나 비밀번호가 없으면 사본은 [`backup`](./database.md#backup)이 쓰는 것과 같습니다. 암호화한 파일의 사본은 데이터 키를 그대로 두므로 같은 키나 비밀번호로 열립니다. 키나 비밀번호를 주면 사본은 무작위로 만든 새 데이터 키로 암호화되고, 그 키나 비밀번호가 새 데이터 키를 감쌉니다. 사본은 그것으로만 열립니다. 파일의 키나 비밀번호를 바꾸면 데이터 키를 다시 감쌀 뿐 데이터 키 자체는 그대로이므로, 노출됐을 수 있는 데이터 키를 버리려면 새 키로 백업한 뒤 사본을 원래 파일 자리에 두면 됩니다. 평문 데이터베이스의 사본도 같은 방식으로 암호화됩니다.

[`OpenOptions`](./open-options.md)처럼 빌더입니다. 메서드마다 `&mut self`를 받아 `&mut Self`를 돌려줍니다. `BackupOptions`는 `Default`도 구현하며, `new`와 같습니다.

```rust
use darudb::{BackupOptions, OpenOptions};

fn main() -> darudb::Result<()> {
    let db = OpenOptions::new().password("old password").open("app.darudb")?;

    db.backup_with("new.darudb", BackupOptions::new().password("new password"))?;
    db.close()
}
```

## 연관 함수

### new

```rust
pub fn new() -> Self
```

파일에 데이터 키가 있으면 그대로 두는 옵션입니다.

## 메서드

### key

```rust
pub fn key(&mut self, key: [u8; 32]) -> &mut Self
```

사본을 새 데이터 키로 암호화하고, `key`가 그 데이터 키를 감쌉니다. `key`와 [`password`](#password)는 서로를 대신하며 마지막에 준 것이 쓰입니다.

### password

```rust
pub fn password(&mut self, password: impl AsRef<[u8]>) -> &mut Self
```

사본을 새 데이터 키로 암호화하고, `password`에서 Argon2id로 만든 키가 그 데이터 키를 감쌉니다. 빈 비밀번호는 아무것도 쓰기 전에 `backup_with`를 `INVALID_ARGUMENT`로 실패시킵니다.

### password_hashing

```rust
pub fn password_hashing(&mut self, memory_kib: u32, iterations: u32, parallelism: u32) -> &mut Self
```

사본의 비밀번호를 해시하는 데 드는 일의 양입니다. [`OpenOptions::password_hashing`](./open-options.md#password-hashing)과 같으며 기본값은 19456 KiB, 반복 2회, 병렬 1입니다. 사본에 비용이 기록되므로 열 때 그 비용이 듭니다.
