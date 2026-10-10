---
title: Encryption
order: 9
---

# Encryption

A database created with a key or a password is encrypted and authenticated, every page of it, and opens only with the same key or password.

## Encrypt a database

::: lang rust

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    println!("{}", db.is_encrypted()); // true
    db.set_password("a new password")?;
    db.close()
}
```

`OpenOptions::key` takes a 32-byte key instead of a password, and `Database::set_key` changes it.

:::

::: lang node

```ts
const db = Database.open('secret.darudb', {
  schema: app,
  password: 'correct horse battery staple'
});

console.log(db.isEncrypted); // true
db.setPassword('a new password'); // or `await db.setPasswordAsync(...)`
db.close();

Database.open('secret.darudb', { schema: app }); // throws KEY_REQUIRED
```

- `key` is a `Uint8Array` of 32 bytes, and `setKey` changes it. Give a `key` or a `password`, not both.
- A `password` is a string or a `Uint8Array`.
- The package copies the key or the password when `open` or `openAsync` is called, and wipes its copy once the engine has its own. A `Uint8Array` you pass can be wiped with `fill(0)` as soon as the call returns; a string cannot be wiped, and stays in memory until the garbage collector reclaims it.
- `setKey` and `setPassword` commit, so they follow the rules of a write: their `Async` twins wait their turn after this process's other writes on the file, and the synchronous forms are refused while an asynchronous write holds it.

:::

::: lang dart

```dart
final db = Database.open(
  'secret.darudb',
  schema: const Schema(1, [userSchema]),
  password: 'correct horse battery staple',
);

print(db.isEncrypted); // true
db.setPassword('a new password'); // or `await db.setPasswordAsync(...)`
db.close();

Database.open('secret.darudb', schema: const Schema(1, [userSchema])); // throws KEY_REQUIRED
```

- `key` is a `Uint8List` of 32 bytes, and `setKey` changes it. Give a `key` or a `password`, not both.
- The package copies the key or the password into native memory when `open` or `openAsync` is called, and wipes its copies once the engine has its own. A `Uint8List` you pass can be wiped with `fillRange` as soon as the call returns; a `String` cannot be wiped, and stays in memory until the garbage collector reclaims it.
- `setKey` and `setPassword` commit, so they follow the rules of a write: their `Async` twins wait their turn after this isolate's other asynchronous writes on the file, and the synchronous forms are refused while an asynchronous write holds it.

:::

::: lang python

```python
import darudb

db = darudb.Database.open(
    "secret.darudb",
    schema=darudb.Schema(1, [User]),
    password="correct horse battery staple",
)

print(db.is_encrypted)  # True
db.set_password("a new password")  # or `await db.set_password_async(...)`
db.close()

darudb.Database.open("secret.darudb", schema=darudb.Schema(1, [User]))  # raises KEY_REQUIRED
```

- `key` is 32 bytes, as `bytes`, `bytearray` or `memoryview`, and `set_key` changes it. Give a `key` or a `password`, not both.
- A `password` is a `str` or bytes.
- The native module copies the key or the password into a buffer of its own, which is wiped once the engine has its copy. A `bytearray` you pass can be wiped with `key[:] = bytes(len(key))` once the call has returned, or once an `_async` twin has been awaited; `bytes` and `str` cannot be wiped, and stay in memory until Python frees them.
- `set_key` and `set_password` commit, so they follow the rules of a write: their `_async` twins wait their turn after this event loop's other asynchronous writes on the file, and the synchronous forms are refused on the loop's thread while an asynchronous write holds it.

:::

Everything inside a page is encrypted: keys, values, collection and tree names included. Every page is authenticated, and so is the header's record of each commit, so a changed byte is reported as `CORRUPTED` rather than read. A file opened without its key fails with `KEY_REQUIRED`, and with the wrong one with `WRONG_KEY`.

## Keys and passwords

- **A key** is 32 random bytes, such as one kept in the operating system's keystore, as [below](#keep-the-key-in-the-operating-system-s-keystore).
- **A password** is turned into a key with Argon2id, which takes tens of milliseconds at the default cost of 19 MiB, 2 iterations and 1 lane. <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" dart="passwordHashing" python="password_hashing" /> raises or lowers that cost for a new file and for a password change. A file records the cost it was made with, so opening it takes that cost whatever the option says.
- **Changing the key or the password** re-encrypts nothing, and once it returns, the old one no longer opens the file.
- A plain database stays plain, and an encrypted one cannot be opened without its key. Keep the key, or the password, where it cannot be lost: without it, the data cannot be read.

<Diagram name="encryption" alt="A password goes through Argon2id, or a 32-byte key is used as it is, to make the key-encryption key, which is never stored. It wraps the data key, which is random and stored wrapped in every commit record, and the data key seals every page with XAES-256-GCM or XChaCha20-Poly1305 and a fresh nonce. A new password or key wraps the same data key again, so no page is encrypted again." />

[Encryption](../engine/encryption.md) in the Engine section explains how pages are encrypted and how the key is kept in the file.

## Keep the key in the operating system's keystore

A key the application makes once and keeps in the operating system's keystore encrypts the file without a password to ask the user for. Make 32 random bytes the first time, store them, and open the database with them from then on. DaruDB does not reach the keystore itself, so the code below uses what each platform offers, and each keystore has its own rules about which apps and users may read an entry. Store the key before the first commit: a key that is lost loses the data.

::: lang rust

```rust
use darudb::OpenOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut key = [0u8; 32];

    getrandom::fill(&mut key)?;
    // Keep `key` in the keystore here, and read it from there next time.

    let db = OpenOptions::new().key(key).open("secret.darudb")?;

    db.close()?;

    Ok(())
}
```

The `keyring` crate reaches the Keychain on macOS and iOS, the Credential Manager on Windows and the Secret Service on Linux. A server keeps the key with its other secrets, such as an environment variable its deployment fills from a secret manager.

:::

::: lang node

In Electron's main process, `safeStorage` encrypts a value with a key the operating system keeps: the Keychain on macOS, DPAPI on Windows, and the desktop's secret store on Linux. The database key, encrypted that way, can sit in a file beside the database.

```ts
import { randomBytes } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { app as electronApp, safeStorage } from 'electron';

// After the `ready` event.
function databaseKey(): Uint8Array {
  const file = join(electronApp.getPath('userData'), 'database.key');

  if (existsSync(file)) {
    return Buffer.from(safeStorage.decryptString(readFileSync(file)), 'base64');
  }

  const key = randomBytes(32);

  writeFileSync(file, safeStorage.encryptString(key.toString('base64')));

  return key;
}

const db = Database.open(join(electronApp.getPath('userData'), 'app.darudb'), {
  schema: app,
  key: databaseKey()
});
```

- Check `safeStorage.isEncryptionAvailable()` first. On Linux, when the desktop has no secret store, `safeStorage.getSelectedStorageBackend()` reports `basic_text`, and what it encrypts is protected by nothing the user holds.
- On a server, keep the key with the service's other secrets, such as an environment variable its deployment fills from a secret manager: `Buffer.from(process.env.DATABASE_KEY, 'base64')`.

:::

::: lang dart

In a Flutter app, the `flutter_secure_storage` package keeps a value in the Keychain on iOS and macOS, and encrypted with a key from the Android Keystore on Android.

```dart
import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

Future<Uint8List> databaseKey() async {
  const storage = FlutterSecureStorage();
  final stored = await storage.read(key: 'database-key');

  if (stored != null) {
    return base64Decode(stored);
  }

  final random = Random.secure();
  final key = Uint8List.fromList([for (var i = 0; i < 32; i++) random.nextInt(256)]);

  await storage.write(key: 'database-key', value: base64Encode(key));

  return key;
}

final db = Database.open(
  'secret.darudb',
  schema: const Schema(1, [userSchema]),
  key: await databaseKey(),
);
```

A Dart server or command-line tool keeps the key with its other secrets, such as an environment variable its deployment fills from a secret manager.

:::

::: lang python

The `keyring` package reaches the Keychain on macOS, the Credential Manager on Windows and the Secret Service on Linux. It stores text, so the key is kept in Base64.

```python
import base64
import secrets

import darudb
import keyring


def database_key() -> bytes:
    stored = keyring.get_password("my-app", "database-key")

    if stored is not None:
        return base64.b64decode(stored)

    key = secrets.token_bytes(32)
    keyring.set_password("my-app", "database-key", base64.b64encode(key).decode())

    return key


db = darudb.Database.open(
    "secret.darudb",
    schema=darudb.Schema(1, [User]),
    key=database_key(),
)
```

A server keeps the key with its other secrets, such as an environment variable its deployment fills from a secret manager: `base64.b64decode(os.environ["DATABASE_KEY"])`.

:::

## Tools on an encrypted file

A backup of an encrypted file is encrypted with the same key and opens with the same key or password, unless it is given a key or a password of its own, which encrypts it under a new data key: the way to change the data key itself, which changing the key or the password does not ([Back up under a new key](./tools.md#back-up-under-a-new-key)). Salvage takes the key or the password of the damaged file, which then opens the new file too. See [Tools](./tools.md).
