---
title: Introduction
order: 1
---

# Introduction

DaruDB is an embedded database that keeps an application's data in one local file, with one engine written in Rust that programs in Rust, Node.js, Dart and Python all use.

An embedded database runs inside your program rather than beside it. There is no server to install, start or connect to: your program opens a file, reads and writes objects in it, and closes it. That makes it the kind of database a desktop application, a mobile app, a command-line tool or a small server keeps its own data in.

## Packages

| Package | Language | What it is |
| --- | --- | --- |
| Crate `darudb` | Rust | The engine itself, and a library a Rust program uses directly |
| npm package `darudb` | Node.js | A binding over the engine, with a typed TypeScript API, synchronous and asynchronous |
| Dart package `darudb` | Dart | A binding over the engine for Flutter apps and Dart programs, synchronous and with `Future`s, whose generator `darudb_generator` stores annotated classes |
| PyPI package `darudb` | Python | A binding over the engine for Python programs, synchronous and for `asyncio`, whose collections are dataclasses |

The Rust crate is not only the core the other packages are built on. A Rust program depends on it the way a Node.js program depends on the npm package, and gets the same collections, queries, migrations and tools. It also has two things the bindings do not: the [storage kernel](../engine/storage-kernel.md) of named byte trees under the objects, and the [calls bindings are built on](../engine/bindings.md).

Choose your language with the switch at the top of the sidebar. The examples on every page, and the API and Types sections, follow it, and the site remembers the choice.

## What it is designed for

Five requirements shape every decision, in this order of priority.

1. **Performance.** Reads and writes faster than the embedded SQL engines applications usually reach for, measured at the same durability settings.
1. **Encryption.** The whole file encrypted and authenticated, with keys derived and stored properly.
1. **Stability.** A file that survives a crash or a power cut, damage that is detected and can be repaired, and several processes using one file at the same time.
1. **Compatibility.** Old and new releases of Windows, macOS, Linux, iOS and Android. The file format carries its version, and every new format version and every new version of your schema comes with a migration from the one before.
1. **Ease of use.** A schema that is easy to declare and queries that are easy to write, with results that are plain objects rather than handles tied to the open database.

These are goals. Each one becomes a claim when the benchmark or the test suite that proves it is in the repository.

## How it is built

The engine is written once, in Rust, and the Node.js, Dart and Python packages are thin bindings over it that add no behaviour of their own. Every rule about the file lives in the engine, which is what makes a file written from one language read the same from another, and what gives every error the same `code` in every language. [How the engine is built](../engine/architecture.md) goes through its layers.

## Where it stands

- **Storage.** A copy-on-write tree of pages, each verified against the check its parent recorded before it is used, so a damaged page is reported rather than read. A commit is durable when it returns; a deferred commit returns before the disk has it and reaches it within a second by default.
- **Encryption.** A database created with a key or a password is encrypted and authenticated, every page of it, and changing the password re-encrypts nothing.
- **Several processes.** Processes share one file through the operating system's file locks alone: one writes at a time, readers never wait, and a process that dies leaves nothing behind.
- **Objects.** Schemas, collections, indexes, links and embedded objects, queries built in code or written as text, and migrations from one schema version to the next.
- **Tools.** An integrity check, online backup, compaction, and salvage of a damaged file.
- **Tests.** Thousands of simulated power cuts and hundreds of real processes killed in the middle of a commit, with encryption on and off, and several processes reading and writing one file while random ones are killed.
- **Packages.** The Rust crate, the Node.js package, the Dart package and the Python package have all of the above, and all four are released. The Node.js, Dart and Python packages ship the engine prebuilt for every platform they support, so installing them needs no Rust compiler.
