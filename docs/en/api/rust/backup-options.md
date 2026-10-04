---
title: BackupOptions
order: 19
counterpart: /types/node/backup-options
---

# BackupOptions

`BackupOptions` holds the options [`Database::backup_with`](./database.md#backup-with) writes a copy with: a key or a password that encrypts the copy under a new data key, and the cost of hashing that password.

```rust
#[derive(Debug, Clone)]
pub struct BackupOptions
```

Without a key or a password, the copy is what [`backup`](./database.md#backup) writes: a copy of an encrypted file keeps its data key, so the same key or password opens it. With one, the copy is encrypted under a new random data key, which the key or password wraps, and opens only with it. Changing a file's key or password wraps its data key again and leaves it as it was, so a backup under a new key is the way to leave behind a data key that may have been exposed: back up, then put the copy in the old file's place. A plain database's copy is encrypted the same way.

It is a builder like [`OpenOptions`](./open-options.md): each method takes `&mut self` and returns `&mut Self`. `BackupOptions` also implements `Default`, which is `new`.

```rust
use darudb::{BackupOptions, OpenOptions};

fn main() -> darudb::Result<()> {
    let db = OpenOptions::new().password("old password").open("app.darudb")?;

    db.backup_with("new.darudb", BackupOptions::new().password("new password"))?;
    db.close()
}
```

## Associated functions

### new

```rust
pub fn new() -> Self
```

Options that keep the file's data key, if it has one.

## Methods

### key

```rust
pub fn key(&mut self, key: [u8; 32]) -> &mut Self
```

Encrypts the copy under a new data key, which `key` wraps. `key` and [`password`](#password) replace each other; the last one given counts.

### password

```rust
pub fn password(&mut self, password: impl AsRef<[u8]>) -> &mut Self
```

Encrypts the copy under a new data key, which a key derived from `password` with Argon2id wraps. An empty password fails `backup_with` with `INVALID_ARGUMENT` before anything is written.

### password_hashing

```rust
pub fn password_hashing(&mut self, memory_kib: u32, iterations: u32, parallelism: u32) -> &mut Self
```

How much work hashing the copy's password takes, as [`OpenOptions::password_hashing`](./open-options.md#password-hashing) says: 19456 KiB, 2 iterations and 1 lane by default. The copy records the cost, so opening it takes that cost.
