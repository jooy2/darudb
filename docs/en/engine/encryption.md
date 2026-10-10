---
title: Encryption
order: 6
---

# Encryption

This page explains how DaruDB encrypts and authenticates a database file: the page cipher and how it is chosen, the data key and how a key or a password wraps it, the MAC on commit records, and what an encrypted file still shows.

The specification is the "Encryption" section of [design/file-format.md](https://github.com/jooy2/darudb/blob/main/design/file-format.md#encryption) in the repository. [Encryption](../guide/encryption.md) shows how to open, create and rekey an encrypted database.

## What is encrypted

A database created with a key or a password is encrypted for its whole life: every page but the header, and so every key, every value and every tree name. Every page is also authenticated, and so is every commit record in the header, so a changed byte is reported as `CORRUPTED` rather than read.

A plain database stays plain. It cannot be encrypted in place: its data has to be copied into a new database created with a key or a password, which is what a backup given a key or a password does, for the application to put in the plain file's place ([Back up under a new key](../guide/tools.md#back-up-under-a-new-key)).

## Pages

Each page is encrypted under the file's data key with an authenticated cipher:

- **The nonce** is 24 random bytes, drawn fresh every time the page is written, and stored in the first 24 bytes of the page, the field every page reserves.
- **The associated data** is the page number, which binds the ciphertext to its place in the file.
- **The tag** of 16 bytes is the page's check. It is stored at the end of the page and in the pointer that leads to the page.

Because the tag is stored in the parent too, someone who can write the file cannot put back an older, validly encrypted version of a page: its tag would not match the one its parent records. A large value's run check is a hash over its pages' tags, which carries the same protection to its pages.

### Which cipher

A file uses one of two ciphers, XAES-256-GCM or XChaCha20-Poly1305. Both take a 24-byte nonce and give a 16-byte tag, so a page has the same layout under either.

A new encrypted file gets XAES-256-GCM when the machine that creates it has AES and carry-less multiplication instructions, and XChaCha20-Poly1305 otherwise, because each is several times faster than the other on the processors it suits. On one Apple silicon machine, 4 KiB pages encrypt at about 3.0 GB/s with XAES-256-GCM and 0.7 GB/s with XChaCha20-Poly1305; on software implementations alone the order reverses, at 0.13 and 0.32 GB/s. Old 32-bit ARM phones and some small ARM boards lack the instructions, and there the choice keeps the file fast.

The application does not choose. The header records the cipher, a file keeps it for life, and any machine opens a file of either kind, only more slowly where the processor does not suit its cipher.

A random 192-bit nonce is safe for any number of page writes under one key. Plain AES-256-GCM has a 96-bit nonce, which makes random nonces unsafe after about 2^32 page writes under one key, and a counter is hard to keep unique across several writer processes and crashes. XAES-256-GCM solves that by deriving a fresh AES-256-GCM key from the first half of each 24-byte nonce.

## The data key and the key block

The pages are encrypted under a data key: 32 random bytes, generated when the file is created. The data key is stored wrapped, in a key block that every commit record carries.

- **The key that wraps it** is the caller's 32-byte key, or the Argon2id hash of the caller's password, under a salt and a cost stored in the key block.
- **It is wrapped with XChaCha20-Poly1305**, with the file's id as associated data, so a key block copied into another file does not unwrap there.
- **Opening** tries the key or password on the key block of each commit record whose check matches, newest first, and uses the first that unwraps. The records share one data key, so which one unwraps it does not matter. A wrong key or password fails every wrapping tag, and opening fails with `WRONG_KEY`. An encrypted file opened without a key or password fails with `KEY_REQUIRED`, and a plain file opened with one fails with `INVALID_ARGUMENT`.

### Changing the key or the password

Changing the key or the password writes a new key block and re-encrypts no page, so it takes the same time whatever the size of the file. It is a sync commit with the new key block, followed by empty sync commits until no slot holds the old one: three commits in all when nothing else commits in between. Every commit copies the key block of the one before it, which is how the new one reaches the other slots. Until then, the old key still opens the file; once the call returns, it no longer does. The calls are <LangCode rust="Database::set_key" node="setKey" dart="setKey" python="set_key" /> and <LangCode rust="Database::set_password" node="setPassword" dart="setPassword" python="set_password" />.

The data key itself stays the same for the life of the file. A backup and a salvage keep it too, since the new file takes the old one's key block, so the key or password that opens the file opens the copy. A backup given a key or a password of its own is the exception: its copy gets a new random data key, which that key or password wraps, and every page of the copy is written under it, as every page of a backup is written anew anyway.

## Passwords

A password becomes a key through Argon2id, which is memory-hard: that is what makes guessing passwords on graphics processors expensive. The default cost is 19 MiB of memory, 2 iterations and 1 lane, the lowest Argon2id setting in the OWASP password storage guidance, which takes tens of milliseconds on a current computer. The memory is what decides it, because a mobile app extension may be allowed little more than that for its whole process.

The cost is stored in each key block, so an application can ask for more when it creates a file or changes its password, with <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" dart="passwordHashing" python="password_hashing" />, without a change to the format. Opening a file always takes the cost it was made with. A key block that asks for more than 1 GiB of memory, 1024 iterations or 64 lanes is refused as `CORRUPTED` before anything is hashed, since the hash allocates its memory at once.

## Commit records

The header stays plain, and each page's tag sits in the clear in its parent as well as in the page. Without a key of its own, a commit record could therefore be assembled by anyone who can write the file, from pages that are already there: the catalog of one commit with the free tree of another, say. No key holder committed that state, and its trees would disagree about which pages are in use, so the next commit would overwrite pages still in use.

Each record of an encrypted file therefore carries a MAC: keyed BLAKE2b with a 16-byte output, under a key derived from the data key, over the file id, the slot number and the record's fields and key block. Recovery never adopts a record whose MAC fails, and a process that reads a record another process wrote checks its MAC as well. BLAKE2b is in the build for Argon2id already, and a MAC of its own keeps the record independent of the page cipher.

## What stays visible

The header page is plain, so an encrypted file still shows its page size, its file id, its cipher, its transaction ids, its page count, and so its size, and the page numbers of its tree roots. Everything inside a page is encrypted.

Replacing the whole file, or the header page alone, with an older copy of itself cannot be detected from inside the file, because the older records are genuine. An application that needs to detect that has to keep the newest transaction id somewhere else; in Rust, `ReadTransaction::commit_id` reads it.

## Keys in memory

The engine keeps keys in buffers that are wiped when they are dropped, and the cipher wipes its own copy. That narrows how long a key sits in memory, but it cannot rule out a copy the compiler or the operating system made. The Node.js package copies the key or password it is given into buffers of its own, and fills them with zeros once the engine has its copy, which it takes when the call is made. A password passed as a JavaScript string stays in memory until the garbage collector reclaims it, so pass a `Uint8Array` to be able to wipe it.

Keep the key where it cannot be lost, such as the operating system's keystore. Without it, the data cannot be read.
