---
title: BackupOptions
order: 15
---

# BackupOptions

`BackupOptions` is what `Database.backup` takes besides the path: a key or a password that encrypts the copy under a new data key.

```ts
interface BackupOptions
```

[`backup` and `backupAsync`](../../api/node/database.md#backup) take it last, and every field is optional. Without a key or a password, a copy of an encrypted file keeps its data key, so the same key or password opens it. With one, the copy is encrypted under a new random data key, which the key or password wraps, and opens only with it. Changing a file's key or password wraps its data key again and leaves it as it was, so a backup under a new key is the way to leave behind a data key that may have been exposed: back up, then put the copy in the old file's place. A plain database's copy is encrypted the same way.

```ts
const report = await db.backupAsync('rekeyed.darudb', { password: 'a new password' });
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `key` | `Uint8Array` | A 32-byte key for the copy, which wraps its new data key |
| `password` | `string \| Uint8Array` | A password for the copy, hashed with Argon2id into the key that wraps its new data key |
| `passwordHashing` | [`PasswordHashing`](./password-hashing.md) | How much work hashing the copy's password takes. 19456 KiB, 2 iterations and 1 lane by default |

- A key that is not 32 bytes, an empty password, or a `key` and a `password` together fails with `INVALID_ARGUMENT`, before anything is written.
- The package copies the key or password when the call is made, so a buffer can be wiped with `fill(0)` as soon as `backup` returns, or as soon as `backupAsync` does, before its promise settles.
