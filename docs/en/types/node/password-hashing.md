---
title: PasswordHashing
order: 3
group: database
pageClass: reference-page
---

# PasswordHashing

`PasswordHashing` is what hashing a password into a key costs, as Argon2id counts it: memory, iterations and parallelism.

```ts
interface PasswordHashing
```

The `passwordHashing` field of [OpenOptions](./open-options.md#passwordhashing) takes one, with all three fields. More memory and more iterations make guessing a password slower for an attacker, and opening the database slower for everyone. The default takes tens of milliseconds on a current computer and fits the memory limits of a mobile app extension.

```ts
import { Database } from 'darudb';

const db = Database.open('secret.darudb', {
  password: 'correct horse battery staple',
  passwordHashing: { memoryKib: 65536, iterations: 3, parallelism: 1 }
});
```

## Fields

| Field | Type | Description |
| --- | --- | --- |
| `memoryKib` | `number` | Memory, in KiB, from 8 per lane of `parallelism` up to 1 GiB (1048576). 19456 by default |
| `iterations` | `number` | Passes over the memory, from 1 to 1024. 2 by default |
| `parallelism` | `number` | Lanes, from 1 to 64. 1 by default |

Each field is a whole number. A value outside its range, or a `passwordHashing` without all three, fails with `INVALID_ARGUMENT`. The option is checked whenever it is given, even when no password is.

## When it applies

- **A new database created with a `password`.** The file records this cost beside the key.
- **`setPassword` and `setPasswordAsync`.** The new password is hashed at the cost this option gave when the process opened the file, the first handle's if it opened the file more than once, and the file records that cost from then on. Without the option, that is the default cost, whatever the file recorded before.
- **Never when a file is opened.** Opening an encrypted file takes the cost the file records, so raising the cost in a new release of an application still opens the files it made before. [`Database.salvage`](../../api/node/database.md) does the same, and takes no cost.

[Encryption](../../guide/encryption.md) explains how the key derived from a password protects the file.
