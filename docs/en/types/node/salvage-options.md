---
title: SalvageOptions
order: 14
---

# SalvageOptions

`SalvageOptions` is what `Database.salvage` takes besides the two paths: the key or password of an encrypted file, and how long to wait for the file.

```ts
interface SalvageOptions
```

[`Database.salvage` and `salvageAsync`](../../api/node/database.md) take it last. Every field is optional. Salvage takes no schema, since the file's own comes with it, and no hashing cost, since an encrypted file records its own.

```ts
import { Database } from 'darudb';

const report = Database.salvage('app.darudb', 'rescued.darudb', {
  password: 'correct horse battery staple',
  busyTimeout: 30_000
});
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `key` | `Uint8Array` | The 32-byte key of an encrypted file, which opens the new file too |
| `password` | `string \| Uint8Array` | The password of an encrypted file, which opens the new file too |
| `busyTimeout` | `number` | How long, in milliseconds, to wait for other processes to close the file before failing with `BUSY`. 5000 by default |

- An encrypted file without a key or password fails with `KEY_REQUIRED`, and with another one with `WRONG_KEY`.
- A plain file with a key or password, a key that is not 32 bytes, or a `key` and a `password` together fails with `INVALID_ARGUMENT`.
- The package copies the key or password when the call is made, so a buffer can be wiped with `fill(0)` as soon as `salvage` returns, or as soon as `salvageAsync` does, before its promise settles.
