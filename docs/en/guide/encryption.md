---
title: Encryption
order: 7
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

Everything inside a page is encrypted: keys, values, collection and tree names included. Every page is authenticated, and so is the header's record of each commit, so a changed byte is reported as `CORRUPTED` rather than read. A file opened without its key fails with `KEY_REQUIRED`, and with the wrong one with `WRONG_KEY`.

## Keys and passwords

- **A key** is 32 random bytes, such as one kept in the operating system's keystore.
- **A password** is turned into a key with Argon2id, which takes tens of milliseconds at the default cost of 19 MiB, 2 iterations and 1 lane. <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" dart="passwordHashing" /> raises or lowers that cost for a new file and for a password change. A file records the cost it was made with, so opening it takes that cost whatever the option says.
- **Changing the key or the password** re-encrypts nothing, and once it returns, the old one no longer opens the file.
- A plain database stays plain, and an encrypted one cannot be opened without its key. Keep the key, or the password, where it cannot be lost: without it, the data cannot be read.

[Encryption](../engine/encryption.md) in the Engine section explains how pages are encrypted and how the key is kept in the file.

## Tools on an encrypted file

A backup of an encrypted file is encrypted with the same key and opens with the same key or password. Salvage takes the key or the password of the damaged file, which then opens the new file too. See [Tools](./tools.md).
