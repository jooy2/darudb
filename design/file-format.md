# File format

Status: accepted. This document describes file format version 2.

A DaruDB database is one file, divided into pages of equal size. Page 0 is the header, which says what the file is and where its newest commits are. Every other page is a node of a B+tree or part of an overflow run that holds one large value. This document gives the layout of each. [Commits and recovery](commits-and-recovery.md) says how they change, and [Locking](locking.md) says how several processes share them.

## Conventions

- Integers are unsigned and little-endian, unless a field says otherwise. The exception is the keys of the engine's own trees, which are big-endian so that their byte order is their numeric order.
- Offsets are in bytes. An offset inside a page counts from the first byte of the page.
- A **reserved** field is written as zeros and not interpreted when read. Giving one a meaning needs a new format version.
- `P` is the page size in bytes, and `C` is the size of a page's content area, `P − 72`.

## Pages

The file is an array of pages of `P` bytes each. Page `n` occupies the bytes from `n × P` up to, but not including, `(n + 1) × P`.

- `P` is a power of two from 4096 to 65536. It is chosen when the file is created, recorded in the header, and never changes.
- Page numbers are 64-bit. Page 0 is the header. Because no pointer ever points at the header, page number 0 in a pointer means "no page".
- Each commit record states the file's **page count**. Pages from 1 to page count − 1 belong to that commit's view of the file; anything past them is not part of it.
- The file never reaches byte 2^62, where the lock bytes begin ([Locking](locking.md#the-lock-bytes)). That bounds the page count at 2^62 / `P`.

The page size is unrelated to the operating system's memory page size, which the engine never relies on because it never maps the file into memory.

**Why 4096 at the smallest.** The header needs 2048 bytes, and it has to be a single page. Smaller pages would also save nothing on current storage, whose physical sectors are 4096 bytes. The skeleton's format version 1 accepted 512; version 2 does not.

**The default page size is 4096 bytes** until the benchmarks of phase 4 settle it.

## Checks

A check is 16 bytes that prove a page is the one expected.

- In a **plain file**, the check of page `n` is the XXH3-128 hash, with seed 0, of the page number as 8 bytes followed by the first `P − 16` bytes of the page. Covering the page number means a page written to the wrong place fails its own check.

Wherever this document stores an XXH3-128 value, it stores the 128-bit hash as a little-endian integer, like every other integer here.

- In an **encrypted file**, the check is the page's authentication tag ([Encryption](#encryption)).

XXH3-128 is fast enough that verifying every page on every read costs little, and 128 bits make an accidental match negligible even across billions of pages. It is not a cryptographic hash, and it does not need to be: it only has to catch accidents. Deliberate tampering is what the encrypted file's tags are for. Which Rust implementation of XXH3 the engine uses is decided when it is written, under the dependency rules in `CONTRIBUTING.md`.

## Pointers

A pointer is 32 bytes:

| Offset | Size | Field                                             |
| ------ | ---- | ------------------------------------------------- |
| 0      | 8    | Page number                                       |
| 8      | 8    | Transaction id of the commit that wrote that page |
| 16     | 16   | Check of that page                                |

A null pointer is 32 zero bytes. The root of an empty tree is a null pointer.

Every page is verified against the pointer that led to it before anything in it is used: its check has to match the pointer's check, and the transaction id in its header has to match the pointer's transaction id. That catches a page that is damaged, a page that is stale (an older version left behind by a write that never reached the disk), and a page written to the wrong place.

The transaction id is what makes recovery cheap. After a crash, the engine checks only the pages written after the last durable commit, and the transaction id in each pointer tells it which children those are without reading the others ([Commits and recovery](commits-and-recovery.md#checking-a-commit)).

## The header page

Page 0 is never encrypted: its fields say how to read everything else, including how to decrypt it. It is laid out in 512-byte sectors, so that on a disk with 512-byte sectors a write to one slot cannot damage another even without promise 2 in [Design](README.md#what-the-engine-assumes-of-the-platform).

| Offset | Size       | Content                         |
| ------ | ---------- | ------------------------------- |
| 0      | 64         | [Static fields](#static-fields) |
| 64     | 1          | [Selector](#the-selector)       |
| 65     | 447        | Reserved                        |
| 512    | 512        | [Slot 0](#commit-slots)         |
| 1024   | 512        | Slot 1                          |
| 1536   | 512        | Slot 2                          |
| 2048   | `P − 2048` | Reserved                        |

### Static fields

Written once, when the file is created, and never again.

| Offset | Size | Field                                                                            |
| ------ | ---- | -------------------------------------------------------------------------------- |
| 0      | 8    | Magic: `89 44 61 72 75 44 42 0A` (`\x89DaruDB\n`)                                |
| 8      | 4    | Format version: 2                                                                |
| 12     | 4    | Page size `P`                                                                    |
| 16     | 16   | File id: 16 random bytes, generated when the file is created                     |
| 32     | 1    | Cipher: 0 for a plain file, 1 for XChaCha20-Poly1305 ([Encryption](#encryption)) |
| 33     | 15   | Reserved                                                                         |
| 48     | 16   | Static check: XXH3-128 of bytes 0 to 47                                          |

The magic's leading `0x89` is outside ASCII, so the file is never mistaken for text and a transfer that strips the eighth bit is detected. Its trailing newline catches a transfer that rewrites line endings. The first 16 bytes are laid out as in format version 1, so every version can at least tell a DaruDB file and its version apart.

### The selector

One byte, rewritten on every commit. Readers find the published commit through it, so it relies on promise 1: a one-byte write is atomic, and a reader or a recovery after a power cut sees either the old value or the new one.

| Bits | Meaning                                                                                               |
| ---- | ----------------------------------------------------------------------------------------------------- |
| 0–1  | The slot holding the published commit: 0, 1 or 2. The value 3 is invalid.                             |
| 2    | **Unsynced**: set when the published commit may not be durable yet, because it was a deferred commit. |
| 3–7  | Reserved. A reader that finds one set treats the header as damaged.                                   |

### Commit slots

Three slots, each holding one commit record or nothing. A slot whose first 8 bytes are zero is empty.

Why three: at any moment one slot holds the published commit and one holds the durable commit, and the writer needs a third place for the record it is writing, so that it never overwrites either. Without deferred commits the published and the durable commit are the same and two slots would do; the third is what lets deferred commits exist without ever writing over a record someone may still need ([Commits and recovery](commits-and-recovery.md#choosing-the-slot)).

A commit record is 512 bytes:

| Offset | Size | Field                                                                                      |
| ------ | ---- | ------------------------------------------------------------------------------------------ |
| 0      | 8    | Transaction id, 1 or more                                                                  |
| 8      | 8    | Durable transaction id: the newest commit known to be durable when this record was written |
| 16     | 8    | Page count                                                                                 |
| 24     | 8    | Next tree id: the id the next new tree will receive, 16 or more                            |
| 32     | 32   | Pointer to the root of the [catalog](#the-catalog)                                         |
| 64     | 32   | Pointer to the root of the [free tree](#the-free-tree)                                     |
| 96     | 32   | Pointer to the root of the [retained tree](#the-retained-tree)                             |
| 128    | 128  | [Key block](#the-key-block), all zeros in a plain file                                     |
| 256    | 240  | Reserved                                                                                   |
| 496    | 16   | Record check: XXH3-128 of one byte holding the slot number, followed by bytes 0 to 495     |

Mixing the slot number into the record check means a record copied or written into the wrong slot fails its check.

A record is **valid** when its check matches and its fields are consistent: the transaction id is at least 1, the durable transaction id is smaller than the transaction id, the page count is at least 1, the next tree id is at least 16, and every root pointer is either null or names a page below the page count, written by a transaction no newer than the record's own.

## The page envelope

Every page other than page 0 has the same frame:

| Offset   | Size | Content                                                       |
| -------- | ---- | ------------------------------------------------------------- |
| 0        | 24   | Prefix: the nonce in an encrypted file, zeros in a plain file |
| 24       | 32   | [Page header](#the-page-header)                               |
| 56       | `C`  | Content, laid out according to the page's kind                |
| `P − 16` | 16   | The page's [check](#checks)                                   |

In an encrypted file, the bytes from 24 up to `P − 16` are ciphertext: the page header and the content are both encrypted.

A plain page carries the 24-byte prefix it does not use. That keeps the content area the same size with and without encryption, so a tree built by the same operations has the same shape in both. That is what lets one test suite cover both modes and compare them. The cost is 24 bytes a page: 0.6% at 4096 bytes and 0.15% at 16384.

## The page header

| Offset | Size | Field                                                                               |
| ------ | ---- | ----------------------------------------------------------------------------------- |
| 24     | 1    | Kind: 1 for a leaf, 2 for a branch, 3 for an overflow page                          |
| 25     | 1    | Level: 0 for a leaf or an overflow page; for a branch, one more than its children's |
| 26     | 2    | Entry count                                                                         |
| 28     | 4    | Reserved                                                                            |
| 32     | 8    | Transaction id of the commit that wrote the page                                    |
| 40     | 8    | Id of the tree the page belongs to                                                  |
| 48     | 8    | For an overflow page, its position in its run, counting from 0. Otherwise reserved. |

Kind, level, tree id and transaction id are redundant with what the parent already knows. They are there so that a page can be understood on its own: the integrity check compares them with the parent's expectations, and salvage uses them to put pages back into trees when the parents are lost.

## Leaf pages

A leaf holds the tree's entries in ascending key order. Its content area starts with a **slot array**: one 2-byte offset per entry, in key order, each giving where in the page the entry starts. The entries themselves sit in the rest of the content area; a writer packs them against the end, leaving the free space in the middle.

An entry is:

| Size | Field                                                      |
| ---- | ---------------------------------------------------------- |
| 2    | Key length `K`                                             |
| 1    | Value kind: 0 for an inline value, 1 for an overflow value |
| 2    | Inline only: value length `V`                              |
| `K`  | Key                                                        |
| `V`  | Inline only: the value                                     |
| 44   | Overflow only: the [overflow reference](#overflow-runs)    |

A leaf holds at least one entry. An empty tree has no leaf at all: its root pointer is null.

## Branch pages

A branch with `k` keys has `k + 1` children. Its content area holds, in order:

1. `k + 1` child pointers of 32 bytes each.
1. A slot array of `k` 2-byte offsets, in key order.
1. The keys, each as a 2-byte length followed by the key, packed against the end of the page.

Child 0 holds the keys below key 0. Child `i`, for `i` from 1 to `k − 1`, holds the keys from key `i − 1` up to, but not including, key `i`. Child `k` holds the keys from key `k − 1` up. A branch has at least one key. Every child of a branch at level `L` is at level `L − 1`.

Leaves are not linked to their neighbours. In a copy-on-write tree, a link to a neighbour would mean rewriting the neighbour whenever a leaf is copied, so a range scan keeps the path from the root instead.

## Overflow runs

A value too large to store inline goes into an **overflow run**: `m` consecutive pages, each of kind 3, carrying the value's bytes in order. Page `i` of the run holds the value's bytes from `i × C` onwards, up to `C` of them. The leaf entry holds a 44-byte reference instead of the value:

| Offset | Size | Field                                                                       |
| ------ | ---- | --------------------------------------------------------------------------- |
| 0      | 8    | First page of the run                                                       |
| 8      | 8    | Transaction id of the commit that wrote the run                             |
| 16     | 4    | Number of pages `m`                                                         |
| 20     | 8    | Value length                                                                |
| 28     | 16   | Run check: XXH3-128 of the checks of the run's pages, concatenated in order |

The run check lets a 44-byte reference vouch for any number of pages. Each page is verified by its own check, and together the checks must reproduce the run check.

## Trees

Every tree has an id, which every one of its pages carries.

| Id    | Tree                              |
| ----- | --------------------------------- |
| 1     | The catalog                       |
| 2     | The free tree                     |
| 3     | The retained tree                 |
| 4–15  | Reserved for the engine           |
| 16 on | Trees created by the layers above |

A tree id is never reused, even after its tree is deleted, so a page's tree id is unambiguous across the whole history of the file. That matters to salvage.

### The catalog

The catalog maps tree names to tree descriptors. Names are byte strings chosen by the layer above the storage kernel; names that begin with the byte `0x00` are reserved for the engine. A descriptor is 56 bytes:

| Offset | Size | Field                                         |
| ------ | ---- | --------------------------------------------- |
| 0      | 8    | Tree id                                       |
| 8      | 32   | Pointer to the tree's root                    |
| 40     | 8    | Number of entries in the tree                 |
| 48     | 4    | Flags, for the layer above. Reserved for now. |
| 52     | 4    | Reserved                                      |

Keeping the entry count in the descriptor makes counting a whole tree free.

### The free tree

The pages no one can reach any more, which the writer may use. Each entry is a run of consecutive free pages: the key is the first page number, as a big-endian 64-bit integer, and the value is the run's length, as a little-endian 64-bit integer. Runs never overlap and never touch: two adjacent runs are merged into one.

### The retained tree

Pages that the published commit no longer reaches, but that an older commit may still need, grouped by the commit that stopped using them. The key is 12 bytes: the transaction id of that commit, big-endian, followed by a big-endian 32-bit sequence number that starts at 0. The value is a list of runs, each 12 bytes: the first page number (8 bytes) and the run length (4 bytes). A group with more runs than fit in one inline value continues under the next sequence number.

[Commits and recovery](commits-and-recovery.md#reclaiming-pages) says when a group moves from here to the free tree.

## Key order

Every tree orders its keys by comparing bytes as unsigned numbers, and a key that is a prefix of another sorts first. The engine knows no other order. A layer that stores typed keys encodes them so that their byte order is the order it wants; the object layer's key encoding comes with phase 4.

## Limits

| Limit                | Rule                                                      | At `P` = 4096 | 16384 | 65536 |
| -------------------- | --------------------------------------------------------- | ------------- | ----- | ----- |
| Content area `C`     | `P − 72`                                                  | 4024          | 16312 | 65464 |
| Longest key          | `⌊(C − 196) / 4⌋`                                         | 957           | 4029  | 16317 |
| Largest inline entry | `7 + K + V ≤ ⌊C / 4⌋`                                     | 1006          | 4078  | 16366 |
| Largest value        | 2^32 − 1 bytes, and memory permitting on 32-bit platforms |               |       |       |

The key limit guarantees that four entries always fit in a node, whatever their keys and whether their values are inline or not. So a node that overflows can always be split into two valid nodes, and a branch always has room for the key a split pushes up. A value whose entry would exceed the inline limit is stored in an overflow run.

## Encryption

The cipher is chosen when the file is created and recorded in the static fields; a file does not change between plain and encrypted except by being rewritten into a new file. ### Pages

Each page is encrypted with **XChaCha20-Poly1305** under the file's data key (DEK), which is 32 random bytes generated when the file is created.

- The nonce is 24 random bytes, drawn fresh every time the page is written, and stored in the page's prefix.
- The associated data is the page number, as 8 bytes, which binds the ciphertext to its place in the file.
- The tag is the page's check, stored at the end of the page and in the pointer that leads to it.

A random 192-bit nonce is safe for any number of writes under one key. AES-256-GCM has only 96 bits of nonce, which makes random nonces unsafe after about 2^32 page writes under one key, and a counter-based nonce is hard to keep unique across several writer processes and crashes. That is why it is not proposed. If it is wanted for hardware acceleration, it needs a nonce scheme of its own and a new cipher value.

Because the tag is stored in the parent, an attacker cannot replace a page with an older, validly encrypted version of the same page: its tag would not match the parent's. The run check of an overflow run is an XXH3-128 hash over tags that an attacker cannot forge, so it carries the same protection.

### The key block

The DEK is stored wrapped under a key-encryption key (KEK) in every commit record. Changing a password is therefore an ordinary commit that writes a new key block, and it never re-encrypts a page.

| Offset | Size | Field                                                         |
| ------ | ---- | ------------------------------------------------------------- |
| 0      | 1    | KDF: 0 when the caller supplies a 32-byte key, 1 for Argon2id |
| 1      | 3    | Reserved                                                      |
| 4      | 4    | Argon2id memory, in KiB                                       |
| 8      | 4    | Argon2id iterations                                           |
| 12     | 4    | Argon2id parallelism                                          |
| 16     | 16   | Salt                                                          |
| 32     | 24   | Wrapping nonce                                                |
| 56     | 32   | Wrapped DEK                                                   |
| 88     | 16   | Wrapping tag                                                  |
| 104    | 24   | Reserved                                                      |

The KEK is either the caller's 32-byte key or the Argon2id hash of the caller's password under the stored salt and parameters. The DEK is wrapped with XChaCha20-Poly1305 under the KEK, with the file id as associated data, so a wrapped key copied into another file does not unwrap there. A wrong password or key fails the wrapping tag, and nothing else is tried.

Argon2id is memory-hard, which is what makes guessing passwords on GPUs expensive; the parameters are stored per record so that they can be raised later without a format change. Their defaults are decided with the encryption phase.

The older records keep the old key block until their slots are overwritten, and until then the old password still opens the file. Changing the key is therefore three sync commits: the one that writes the new key block, and two empty ones that overwrite the other two slots. Rotating the DEK itself means rewriting every page, which is done by compacting into a new file.

### What stays visible

The header page is plain, so an encrypted file still shows its page size, its file id, its transaction ids, its page count (and so its size), and the page numbers of its tree roots. Everything inside a page is encrypted, including every key, every value and every tree name.

Replacing the whole file with an older copy of itself cannot be detected from inside the file. An application that needs to detect it has to keep the newest transaction id somewhere else.

## Reading a file

Anything read from the file is untrusted input. A reader checks everything below before using it, and reports a failure as the error named, never as a panic.

| Step                                                                               | On failure                   |
| ---------------------------------------------------------------------------------- | ---------------------------- |
| The file holds at least 64 bytes, and they start with the magic                    | `NOT_A_DATABASE`             |
| The format version is 2                                                            | `UNSUPPORTED_FORMAT_VERSION` |
| The static check matches                                                           | `CORRUPTED`                  |
| The page size is a power of two from 4096 to 65536, and the cipher is known        | `CORRUPTED`                  |
| The file holds at least one whole page                                             | `CORRUPTED`                  |
| The selector's reserved bits are clear and its slot number is not 3                | `CORRUPTED`                  |
| Each record used is valid ([Commit slots](#commit-slots))                          | `CORRUPTED`                  |
| Each page read lies below the page count and inside the file                       | `CORRUPTED`                  |
| Each page's check and transaction id match the pointer that led to it              | `CORRUPTED`                  |
| Each page's kind, level and tree id are what its parent expects                    | `CORRUPTED`                  |
| Every offset and length inside a page stays inside the page, and keys are in order | `CORRUPTED`                  |

Checks that span pages, such as whether every key in a child lies between its parent's separators, belong to the integrity check rather than to every read ([Commits and recovery](commits-and-recovery.md#checking-and-salvaging)).

## Format versions

| Version | Introduced by                                                                                               |
| ------- | ----------------------------------------------------------------------------------------------------------- |
| 1       | The skeleton: magic, format version and page size, with page sizes from 512 bytes. Nothing could be stored. |
| 2       | This document: commits, trees and encryption.                                                               |

A build that writes version 2 refuses a version 1 file with `UNSUPPORTED_FORMAT_VERSION` and offers no migration, since a version 1 file never held data.
