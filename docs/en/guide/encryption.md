---
title: Encryption
order: 9
---

# Encryption

A database created with a key or a password is encrypted and authenticated, every page of it, and opens only with the same key or password.

## How a file is encrypted

When an encrypted database is created, the engine makes a data key of 32 random bytes, and seals every page it writes from then on with that key. The key or password you give never encrypts a page itself: it wraps the data key, and the file stores the data key only in that wrapped form.

- **Pages.** Each page is encrypted with XAES-256-GCM or XChaCha20-Poly1305, both authenticated ciphers, under a 24-byte nonce drawn fresh every time the page is written. The machine that creates the file picks the cipher by what its processor does fast, and the file records it; any machine opens a file of either kind.
- **Checks.** The cipher's 16-byte tag is the page's check, kept in the page and in the pointer that leads to it. A page that was changed, or an older copy of it put back, fails with `CORRUPTED` rather than being read. The header's record of each commit carries a MAC as well, under a key derived from the data key.
- **The data key.** Your 32-byte key, or the Argon2id hash of your password, wraps the data key, and every commit record stores the wrapped data key. Changing the key or the password wraps the same data key again, so no page is encrypted again.
- **What is hidden, and what is not.** Keys, values, collection names and tree names are all inside pages, and encrypted. The header page is plain, so the page size, the cipher, the transaction ids and the size of the file can be read without the key.
- **In memory.** The engine decrypts a page as it reads it from the file and keeps the plaintext in its page cache, so a read the cache answers costs what it costs in a plain file.

<Diagram name="encryption" alt="A password goes through Argon2id, or a 32-byte key is used as it is, to make the key-encryption key, which is never stored. It wraps the data key, which is random and stored wrapped in every commit record, and the data key seals every page with XAES-256-GCM or XChaCha20-Poly1305 and a fresh nonce. A new password or key wraps the same data key again, so no page is encrypted again." />

[Encryption](../engine/encryption.md) in the Engine section has the details: why these two ciphers, how the wrapped data key is stored and found, and why the commit records need a MAC.

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

A file opened without its key fails with `KEY_REQUIRED`, and with the wrong one with `WRONG_KEY`. A plain file opened with a key or a password fails with `INVALID_ARGUMENT`.

## Keys and passwords

- **A key** is 32 random bytes. It opens the file at once, which suits an application that keeps the key for its user, in the operating system's keystore as [below](#keep-the-key-in-the-operating-system-s-keystore), and a server that keeps it with its other secrets.
- **A password** is turned into a key with Argon2id, which takes tens of milliseconds at the default cost of 19 MiB, 2 iterations and 1 lane, every time the file is opened. It suits a file that a person unlocks by typing. <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" dart="passwordHashing" python="password_hashing" /> raises or lowers that cost for a new file and for a password change. A file records the cost it was made with, so opening it takes that cost whatever the option says.
- A file made with one can change to the other: <LangCode rust="Database::set_key" node="setKey" dart="setKey" python="set_key" /> gives a file made with a password a key instead, and <LangCode rust="Database::set_password" node="setPassword" dart="setPassword" python="set_password" /> does the reverse.

## Make a key

A key has to be random: 32 bytes from a secure random generator, never bytes worked out from a name, a date or a password. An application makes its key in code, as the [next section](#keep-the-key-in-the-operating-system-s-keystore) shows. For a test, or for a key a server keeps with its secrets, the button below makes one.

<KeyGenerator />

The key is made in this browser, by the Web Crypto API's `crypto.getRandomValues`, and is not sent anywhere. The two forms hold the same 32 bytes: hex in 64 characters, and Base64 in 44. A server that keeps the key in an environment variable, here `DATABASE_KEY` in Base64, decodes it before opening the database:

::: lang rust

```rust
use base64::Engine as _;

fn database_key() -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let text = std::env::var("DATABASE_KEY")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(text)?;

    Ok(bytes.try_into().map_err(|_| "DATABASE_KEY is not 32 bytes")?)
}
```

The standard library has no Base64 decoder, so this uses the `base64` crate.

:::

::: lang node

```ts
const db = Database.open('app.darudb', {
  schema: app,
  key: Buffer.from(process.env.DATABASE_KEY ?? '', 'base64')
});
```

A key that does not decode to 32 bytes fails with `INVALID_ARGUMENT`.

:::

::: lang dart

```dart
import 'dart:convert';
import 'dart:io';

final db = Database.open(
  'app.darudb',
  schema: const Schema(1, [userSchema]),
  key: base64Decode(Platform.environment['DATABASE_KEY']!),
);
```

A key that does not decode to 32 bytes fails with `INVALID_ARGUMENT`.

:::

::: lang python

```python
import base64
import os

db = darudb.Database.open(
    "app.darudb",
    schema=darudb.Schema(1, [User]),
    key=base64.b64decode(os.environ["DATABASE_KEY"]),
)
```

A key that does not decode to 32 bytes fails with `INVALID_ARGUMENT`.

:::

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

## Encrypt an existing database

A plain database cannot be encrypted in place. A backup given a key or a password writes an encrypted copy of it, which then takes the plain file's place.

::: lang rust

```rust
use darudb::{BackupOptions, Database};

fn encrypt(key: [u8; 32]) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open("app.darudb")?;

    db.backup_with("app.encrypted.darudb", BackupOptions::new().key(key))?;
    db.close()?;
    std::fs::rename("app.encrypted.darudb", "app.darudb")?;

    Ok(())
}
```

:::

::: lang node

```ts
import { renameSync } from 'node:fs';

const db = Database.open('app.darudb', { schema: app });

db.backup('app.encrypted.darudb', { key }); // or `await db.backupAsync(...)`
db.close();
renameSync('app.encrypted.darudb', 'app.darudb');
```

:::

::: lang dart

```dart
import 'dart:io';

final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

db.backup('app.encrypted.darudb', key: key); // or `await db.backupAsync(...)`
db.close();
File('app.encrypted.darudb').renameSync('app.darudb');
```

:::

::: lang python

```python
import os

db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))
db.backup("app.encrypted.darudb", key=key)  # or `await db.backup_async(...)`
db.close()
os.replace("app.encrypted.darudb", "app.darudb")
```

:::

- The copy holds the commit that was published when the backup began. Stop writing, and close every handle to the plain file in every process, before the copy takes its place: a write made meanwhile is not in the copy, and a process that keeps the old file open goes on using it.
- Replacing the plain file does not wipe it from the disk. The file system frees its blocks without overwriting them, and copies made before, in backups or synced folders, stay plain.

## Change the key or the password

<LangCode rust="Database::set_key" node="setKey" dart="setKey" python="set_key" /> and <LangCode rust="Database::set_password" node="setPassword" dart="setPassword" python="set_password" /> wrap the data key again under the new key or password, as [Encrypt a database](#encrypt-a-database) shows. No page is encrypted again, so the call takes the same time whatever the size of the file: three sync commits, one that writes the newly wrapped data key and two that overwrite the old one in the header's other slots. Once it returns, the old key or password no longer opens the file. A new password is hashed at the cost of the options the handle was opened with.

What the change leaves as it was:

- **Copies made before it.** A backup, or a copy in the operating system's backups or a synced folder, holds the data key wrapped as it was, and still opens with the old key or password.
- **The data key.** Anyone who has the old password and a copy of the file from before the change can unwrap the data key from that copy, and the data key still decrypts the file as it is now. When a password or a key may have leaked, change the data key too: back up under a new key, as the previous section does, and put the copy in the file's place ([Back up under a new key](./tools.md#back-up-under-a-new-key)).

## Decrypt a database

No call takes the encryption off a file, and the backup of an encrypted file is always encrypted, under its own data key or a new one. A plain database is made by copying the objects into a new database created without a key, with the same schema, which then takes the encrypted file's place.

::: lang rust

```rust
use darudb::OpenOptions;

fn decrypt(key: [u8; 32]) -> Result<(), darudb::Error> {
    let schema = schema();
    let encrypted = OpenOptions::new().schema(schema.clone()).key(key).open("secret.darudb")?;
    let plain = OpenOptions::new().schema(schema.clone()).open("plain.darudb")?;
    let read = encrypted.begin_read()?;

    for collection in schema.collections() {
        let mut txn = plain.begin_write()?;
        let mut copy = txn.collection(collection.name())?;

        for object in read.collection(collection.name())?.iter()? {
            copy.put(object?)?;
        }

        txn.commit()?;
    }

    drop(read);
    plain.close()?;
    encrypted.close()
}
```

`schema()` declares the application's schema, as in [Collections and objects](./objects.md#declare-a-schema). A tool that does not know the schema opens the encrypted file without one and takes it from `Database::stored_schema`.

:::

::: lang node

```ts
import { Database } from 'darudb';

const encrypted = Database.open('secret.darudb', { schema: app, key });
const plain = Database.open('plain.darudb', { schema: app });

for (const name of ['teams', 'users'] as const) {
  const objects = encrypted.read((txn) => txn.collection(name).find());

  plain.write((txn) => {
    txn.collection(name).putMany(objects);
  });
}

plain.close();
encrypted.close();
```

The list names every collection of `app`, the schema of [Collections and objects](./objects.md#declare-a-schema).

:::

::: lang dart

```dart
void copy<T, Q extends QueryBuilder<T>, K extends Object>(
  Database from,
  Database to,
  CollectionSchema<T, Q, K> collection,
) {
  final objects = from.read((txn) => txn.collection(collection).find());

  to.write((txn) => txn.collection(collection).putMany(objects));
}

const schema = Schema(1, [teamSchema, userSchema]);
final encrypted = Database.open('secret.darudb', schema: schema, key: key);
final plain = Database.open('plain.darudb', schema: schema);

copy(encrypted, plain, teamSchema);
copy(encrypted, plain, userSchema);

plain.close();
encrypted.close();
```

:::

::: lang python

```python
import darudb

schema = darudb.Schema(1, [Team, User])

with (
    darudb.Database.open("secret.darudb", schema=schema, key=key) as encrypted,
    darudb.Database.open("plain.darudb", schema=schema) as plain,
):
    for cls in (Team, User):
        with encrypted.read() as txn:
            objects = txn.collection(cls).find()

        with plain.write() as txn:
            txn.collection(cls).put_many(objects)
```

:::

- Each collection is copied in one write transaction, so all of its objects are in memory at once. Copy a very large collection in parts, by ranges of its primary key, and commit after each part.
- Primary keys are copied as they are, so links still lead to the same objects. In a collection keyed by an auto-increment, the copy's next number is one past the highest key copied, so a number the encrypted file gave an object that was later deleted can be given again.
- Run the [integrity check](./tools.md#check-a-file) on the copy, and close every handle to both files before the copy takes the encrypted file's place. From then on, anyone who can read the file can read the data.

## What to consider

- **A lost key loses the data.** No call or tool opens a file whose key and password are both gone, and there is no other way in. Store the key before the first commit, somewhere that has backups of its own, such as the operating system's keystore or a secret manager.
- **A key does not belong in the application's code.** Every copy of the application would share it, and anyone with a copy could read it out of the program. An application on users' devices makes a key on each device and keeps it in the keystore, as [above](#keep-the-key-in-the-operating-system-s-keystore).
- **A password has to be hard to guess.** Argon2id makes each guess cost the memory and time of an open, which slows guessing down but does not stop it: a short or common password still falls. Raising the cost slows every open as much as every guess.
- **Every process pays for the hash.** Each open of a password file takes the memory of its cost, 19 MiB by default, in every process that opens it, and a mobile app extension may be allowed little more than that for its whole process.
- **Pages cost a little more to read and write.** Every page read from the disk is decrypted and checked, and every page written is encrypted. A file keeps the cipher the machine that created it chose, so it is slower on a processor that does not suit that cipher.
- **Every handle presents the key.** Each process that opens the file, and each further handle in one process, gives the key or the password. One that does not fails with `KEY_REQUIRED`, even while another handle has the file open.
- **The data is plain inside the process.** While the database is open, the data key and the pages read are in the process's memory, where a debugger, a crash dump or the system's swap file can reach them. Pass the key as bytes, and wipe your copy once the call returns; a string cannot be wiped.
- **The size and the commits show.** The plain header shows the file's size and its transaction ids, so how much data there is and how often it changes can be seen without the key. Putting back an older copy of the whole file cannot be detected from inside it either. An application that has to notice keeps the newest transaction id somewhere else, which the Rust API reads with `ReadTransaction::commit_id`.

## Tools on an encrypted file

A backup of an encrypted file is encrypted with the same data key and opens with the same key or password, unless it is given a key or a password of its own, which encrypts it under a new data key ([Change the key or the password](#change-the-key-or-the-password)). Salvage takes the key or the password of the damaged file, which then opens the new file too. See [Tools](./tools.md).
