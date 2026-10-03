---
title: How the engine is built
order: 1
---

# How the engine is built

DaruDB is one engine written in Rust, and this page explains how it is put together: its layers from the file up, what a binding hands it, how it reads and caches the file, and the threads it starts.

## One engine, thin bindings

The engine is the Rust crate `darudb`. The Node.js package is a binding over that same engine, and a Dart package is planned the same way. A binding converts values between its language and the engine, and decides nothing about the file. Checking objects against the schema, keeping indexes in step, parsing, planning and running queries, migrating a schema, and giving every failure its error code all happen in the engine. That is why a file written from one language reads the same from every other, and why an error carries the same `code` in each.

### What the Rust crate is

The crate `darudb` is the engine itself, not a private part of the bindings. A Rust program depends on it and calls it directly, with nothing in between, and it offers three things:

- **The object layer**, which every language has: schemas, collections of typed objects, indexes, queries and migrations. [Collections and objects](../guide/objects.md) shows it.
- **The storage kernel**, which only Rust has: the named trees of byte keys and byte values that the object layer is built on. [Storage kernel](./storage-kernel.md) shows it.
- **The calls bindings are built on**: opening a database with a migration that stops between its steps, objects as records, and queries as IR. [Building a binding](./bindings.md) describes them.

## The layers

Each layer uses only the layers below it, so that a layer can be read, tested and replaced on its own. From the bottom up:

| Layer | Module | What it owns |
| --- | --- | --- |
| File format | `format` | What every byte of the file means: the header, pages, nodes, pointers and commit records, and the object layer's encodings of keys, records, schemas and queries. Pure functions over bytes, with no I/O. |
| Encryption | `crypto` | Encrypting and decrypting pages, wrapping the data key, turning a password into a key, and authenticating commit records. Also without I/O. |
| Operating system | `sys` | The system calls the Rust standard library lacks: byte-range locks, a rename that never replaces a file, and what identifies a file on Windows. The only module with `unsafe` code. |
| Storage | `storage` | How bytes reach the disk: positional reads and writes, the pager that verifies every page and encrypts and decrypts it, the page cache, and creating a new file. |
| B+tree | `btree` | Copy-on-write B+trees over those pages: lookups, walks in either direction, changes, and encoding the changed nodes when a transaction commits. |
| Free space | `space` | Which pages a write transaction may use, which it gives back, and reclaiming pages no reader can reach any more. |
| Locks | `lock` | Coordination between processes through the operating system's byte-range locks. |
| Instance | `instance` | The one shared state of each open file in a process: the file handle, the last commit this process knows of, the writer gate, the registry of snapshots, the page cache and the background threads. |
| Transactions | `txn` | Read and write transactions, the commit, and recovery. |
| Objects and queries | `schema`, `query` | Schemas and migrations, objects written with their indexes in step, and queries: the IR, the builder, the query language, choosing an index and running the query. |
| Tools | `tools` | The integrity check, backup, compaction and salvage. |
| Public API | `database`, `options`, `error` | `Database`, `OpenOptions` and `Error`, which the crate exports together with the public types of the layers below. |

Everything the engine stores is in the storage kernel's trees, objects included. A collection is one tree from each object's encoded primary key to its record, and each index is a tree of its own. Their names begin with a NUL byte, which the kernel keeps for the engine, so no tree a program creates can collide with them. A commit of objects is therefore a commit of trees, with the same durability, recovery and locking. [File format](./file-format.md) describes the layout.

## What a binding hands the engine

Each call from another language into Rust has to convert its arguments and its result, and in practice that cost, more than the engine's own work, often decides how fast a binding is. So a binding does not pass objects field by field. It hands the engine byte buffers in formats the engine defines:

- **Records.** An object crosses as a record: its fields by number, each a tag and a value. A batch of objects to write goes in one buffer, and the objects a query finds come back in one buffer.
- **Query IR.** A query built with a binding's query builder crosses as one buffer that holds its filter, sort, offset and limit. Text in the query language is parsed by the engine into the same IR, so every language shares one parser.
- **The schema.** A declared schema crosses once, when the database is opened, in the format the file stores a schema in.

The engine checks every buffer it is given before anything is stored: a record against the schema, a query against the collection it names. A binding cannot write what the schema does not allow. [Building a binding](./bindings.md) describes the formats and the calls that take them.

## Reading and writing the file

The engine reads and writes the file at explicit offsets, with `pread` and `pwrite` on Unix-like systems and with reads and writes at an offset on Windows. A commit writes each run of consecutive pages with one call, `pwritev` where the system has it. The file is never mapped into memory.

Mapping the file would let a read use a page where it lies, without copying it. It was rejected for two reasons:

- **Another process can change the file.** Several processes share one file, and any of them may commit, which writes pages, or shorten the file, at any moment. Bytes in a mapping can change under a reference that Rust's rules say cannot change, so safe code cannot hold one. A positional read is a copy that nothing else changes, and it is verified against its check before it is used.
- **Encryption.** A page of an encrypted file has to be decrypted before it is read. A mapping would hold ciphertext, and the plaintext would need a copy of its own anyway.

The cost is the copy that a mapping would save, so the performance goal has to be met without it. The page cache is how the engine makes that copy rare.

## The page cache

Each process keeps the pages it reads in a cache, one for each open file, shared by every handle to that file in the process. A page is verified against its check, and decrypted in an encrypted file, once, when it is read. Reading it again from the cache costs neither a read nor a check.

- **Keyed by page number and check.** A reader always knows the check it expects, from the pointer that led it to the page, so a cached copy of an older version of the page never matches. Nothing has to be invalidated when another process commits: entries that no longer match anything age out.
- **Sized in bytes.** 32 MiB for each file by default, and never fewer than 16 pages. It fills only as pages are read, and lets the pages it took in first go first. In Rust, `OpenOptions::cache_size` sets the size; in Node.js, the `cacheSize` option. The handle that opens a file first in a process decides it for every handle.
- **Tree nodes only.** A node in the cache also keeps a few bytes of each of its keys beside the page, in memory only, so that a search reads a whole key only where two of those agree; that memory counts against the size. A large value kept in pages of its own is not cached, and is read from the file each time.
- **Commits fill it.** The nodes a commit writes go into the cache, so the next transaction finds them there.

## Background threads

The engine starts two kinds of thread, each only while it has work to do. Their names tell them apart in a debugger or a profiler.

- **`darudb-sync`**, one for each open file with deferred commits waiting for the disk. The deferred commit that opens an unsynced window starts it, and it makes the window durable, as `sync` would, once the window's time limit runs out. It also watches a commit that another process left unsynced, and makes it durable if it is still waiting a time limit later, since that process may have died. The thread ends with the window. It holds the file only weakly, so it never keeps a database open.
- **`darudb-keeper`**, one for each process. When the last read transaction on a commit ends, the process keeps that snapshot's lock for 20 milliseconds, so that a read transaction begun right after can join it cheaply. This thread releases the kept locks, and ends after ten seconds with nothing to do.

If the system refuses to start either thread, nothing is lost. The next deferred commit after the time limit is made durable itself, and a kept lock goes when the process reads a newer commit, begins a write transaction, or closes the file. [Commits and recovery](./commits-and-recovery.md) and [Locking](./locking.md) say more about each.
