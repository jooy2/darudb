---
title: File format
order: 3
---

# File format

This page summarises how a DaruDB database file is laid out: its pages, the header and its three commit slots, the pointers that verify every page, and how the format is versioned.

The specification, with every offset and field, is [design/file-format.md](https://github.com/jooy2/darudb/blob/main/design/file-format.md) in the repository. It describes format version 4, the one this build reads and writes.

## Pages

A database is one file, divided into pages of equal size.

- **The page size** is a power of two from 4096 to 65536 bytes, chosen when the file is created and recorded in its header. 4096 is the default. Larger pages made scans and counts faster in the benchmarks, but a lookup in a file larger than the page cache slower, since a miss reads and checks a whole page, and a small commit slower, since it writes whole pages. An application that mostly scans can choose more when it creates the file.
- **Page numbers** are 64-bit. Page 0 is the header, and every other page is a node of a B+tree or part of a large value.
- **The operating system's memory page size** plays no part. The engine never maps the file, so it never assumes one, which matters on Android, where memory pages are now 16 KB.
- **Each commit counts the file's pages.** Pages past that count are not part of the commit.
- **The file never reaches byte 2^62**, where the lock bytes begin ([Locking](./locking.md)).

## The header

Page 0 is never encrypted, because it says how to read everything else, the key included. Its first 2048 bytes are laid out in sectors of 512 bytes, so that on a disk with 512-byte sectors a write to one commit slot cannot damage another.

| Bytes | What they hold |
| --- | --- |
| 0 to 63 | The static fields: the magic bytes `\x89DaruDB\n`, the format version, the page size, a file id of 16 random bytes, the cipher, and a check of them all. Written once, when the file is created. |
| 64 | The selector |
| 512 to 2047 | Three commit slots of 512 bytes each |

The magic's first byte is outside ASCII, so the file is never mistaken for text and a transfer that strips the eighth bit is caught. Its last byte, a newline, catches a transfer that rewrites line endings.

### The selector

One byte, rewritten on every commit. Two bits name the slot that holds the published commit, the one a new reader sees. One bit, the unsynced bit, is set when that commit was deferred and may not be durable yet. A one-byte write is atomic, so a reader, or a recovery after a power cut, sees either the old selector or the new one, and readers find the published commit without taking a lock.

### Three commit slots

Each slot holds one commit record or nothing. A record holds the commit's transaction id, the transaction id of the newest commit known to be durable when it was written, the page count, the roots of the engine's three trees described below, the key block and a MAC in an encrypted file, and a check that mixes in the slot number, so that a record written into the wrong slot fails it.

Three is the fewest that works. At any moment one slot holds the published commit and one holds the durable commit, which differ while deferred commits wait for the disk, and the writer needs a third place for the record it is writing. So a new record never overwrites one that a reader or a recovery may still need. [Commits and recovery](./commits-and-recovery.md) says how the writer chooses.

## The page envelope

Every page other than page 0 has the same frame:

| Bytes | What they hold |
| --- | --- |
| 0 to 23 | The nonce of an encrypted page, and zeros in a plain file |
| 24 to 55 | The page header: the kind (leaf, branch or overflow), the level, the entry count, the transaction id of the commit that wrote the page, the id of its tree, and an overflow page's position in its run |
| 56 to the last 16 | The content |
| The last 16 | The page's check |

- **Every page reserves the nonce field**, encrypted or not. A tree built by the same operations then has the same shape with and without encryption, which lets one test suite cover both. The cost is 24 bytes a page, 0.6% at 4096 bytes.
- **The page header repeats what the parent knows**, so that a page can be understood on its own. The integrity check compares it with what the parent expects, and salvage uses it to put pages back into trees whose parents are lost.

## Pointers and checks

Every reference from one page to another, and from the header to a tree's root, is a pointer of 32 bytes: the page number (8 bytes), the transaction id of the commit that wrote the page (8), and the page's check (16).

- **The check** is the XXH3-128 hash of the page number and the page's bytes in a plain file, and the page's authentication tag in an encrypted one ([Encryption](./encryption.md)). Mixing in the page number means a page written to the wrong place fails its own check.
- **Every page is verified against the pointer that led to it** before anything in it is used: its check has to match, and so does the transaction id in its header. That catches a page damaged on the disk, a stale page left behind by a write that never reached it, and a page written to the wrong place. A page that fails is reported as `CORRUPTED`, never read.
- **The transaction id makes recovery cheap.** After a crash, recovery checks only the pages written since the last durable commit, and the ids in the pointers tell it which children those are without reading the others.

XXH3-128 is fast enough that checking every page on every read costs little, and 128 bits make an accidental match negligible across billions of pages. It is not a cryptographic hash and does not have to be: it catches accidents, and an encrypted file's tags are what stop deliberate changes.

## Trees

Every tree has an id, which each of its pages carries, and a tree id is never reused, so salvage can tell which tree a page belonged to across the file's whole history.

| Id | Tree |
| --- | --- |
| 1 | The catalog: each tree's name, with its id, its root and its entry count |
| 2 | The free tree: runs of pages nothing can reach any more, which the writer may use |
| 3 | The retained tree: pages that only an older commit still reaches, grouped by the commit that stopped using them, until no reader or recovery can need them |
| 4 to 15 | Reserved for the engine |
| 16 and up | Trees of the layers above: the storage kernel's trees, and the collections and indexes of the object layer |

- **Leaves** hold entries in key order, through an array of 2-byte offsets at the start of their content. **Branches** hold child pointers and the separator keys between them.
- **Leaves are not linked to their neighbours.** In a copy-on-write tree, a link would mean rewriting the neighbour every time a leaf is copied, so a walk keeps its path from the root instead.
- **A large value** goes into an overflow run of consecutive pages, and its entry holds a reference of 44 bytes with a check over the checks of the run's pages.
- **Keys order as unsigned bytes**, and nothing else. A key is at most a quarter of a page, less a few bytes, which guarantees that four entries always fit in a node, so a node that overflows can always be split.
- **The catalog keeps each tree's entry count**, so counting a whole tree costs nothing.

### Objects in trees

The object layer stores everything in trees whose names begin with the byte `0x00`, which the catalog reserves for the engine. `\0meta` holds the stored schema and the collections' auto-increment counters, each collection's objects are in a tree from the encoded primary key to the object's record, and each index is a tree of its own. Keys are encoded so that their bytes sort as their values do, and records hold field ids rather than names, so renaming a field rewrites nothing. The stored schema carries a format number of its own for these encodings. [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md) specifies them.

## Reading a file

Anything read from the file is treated as untrusted input. Before it is used, the engine checks the magic, the format version, the static check, the page size and the cipher, the selector's reserved bits, each commit record it uses, and each page: that it lies inside the file and below the page count, that its check, transaction id, kind, level and tree are what its pointer and parent expect, and that every offset and length inside it stays inside it. A damaged file produces an error, `CORRUPTED` for damage and `NOT_A_DATABASE` for a file that is not a DaruDB database at all, and never brings the program down.

Checks that span several pages, such as whether every key under a branch lies between its separators, belong to the integrity check rather than to every read ([Tools](../guide/tools.md)).

## Format versions

The header's format version names the layout of everything on disk, and any change to what is written changes it.

- **This build reads and writes format version 4.** A file in any other version, older or newer, is refused with `UNSUPPORTED_FORMAT_VERSION`, and the error names the version found. The first 16 bytes of the header have kept the same layout since version 1, so every build can at least tell a DaruDB file and its version apart.
- **Until the first release, there are no migrations.** Versions 1 to 3 never left development, so a file from an older build is refused, not upgraded.
- **From the first release on**, the plan is that opening a file in an older format upgrades it, unless an option turns that off for an application that may roll back to an older build, which then upgrades the file with an explicit call.

In Rust, `darudb::FORMAT_VERSION` is the version a build reads and writes ([constants](../types/rust/constants.md)), and in Node.js, the package exports it as `FORMAT_VERSION`.
