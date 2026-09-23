---
title: Introduction
order: 1
---

# Introduction

DaruDB is an embedded database that keeps an application's data in one local file, with one engine written in Rust that programs in Rust, Node.js and Dart all use.

An embedded database runs inside your program rather than beside it. There is no server to install, start or connect to: your program opens a file, reads and writes objects in it, and closes it. That makes it the kind of database a desktop application, a mobile app, a command-line tool or a small server keeps its own data in.

## What it is designed for

Five requirements shape every decision, in this order of priority.

1. **Performance.** Reads and writes faster than the embedded SQL engines applications usually reach for, measured at the same durability settings.
1. **Encryption.** The whole file encrypted and authenticated, with keys derived and stored properly.
1. **Stability.** A file that survives a crash or a power cut, damage that is detected and can be repaired, and several processes using one file at the same time.
1. **Compatibility.** Old and new releases of Windows, macOS, Linux, iOS and Android. The file format carries its version, and every new format version and every new version of your schema comes with a migration from the one before.
1. **Ease of use.** A schema that is easy to declare and queries that are easy to write, with results that are plain objects rather than handles tied to the open database.

These are goals. Each one becomes a claim when the benchmark or the test suite that proves it is in the repository.

## How it is built

The engine is written once, in Rust. The Rust crate is the engine itself, and the Node.js and Dart packages are thin bindings over it that add no behaviour of their own. Every rule about the file lives in the engine, which is what makes a file written from one language read the same from another, and what gives every error the same `code` in every language.

Inside the engine, each layer only uses the layers below it:

| Layer | What it does |
| --- | --- |
| Objects, schema, queries | Collections, indexes, migrations, and the queries run against them |
| Transactions and locks | Snapshots for readers, one writer at a time, coordinated between processes through file locks |
| Copy-on-write B+tree | Stores records and indexes without ever overwriting a committed page |
| Pages | Reads and writes fixed-size pages, with a checksum on each and, when enabled, encryption |
| File format | What every byte of the file means, with the format version in the header |

The file is read and written at explicit offsets and never mapped into memory. Mapping would give faster reads in some cases, but it cannot be made safe when another process changes the file, and it does not combine with page encryption. The performance goal is to be met without it.

## Where it stands

The storage kernel works, for one process at a time:

- The Rust crate stores named trees of byte keys and byte values in read and write transactions. A commit is durable when it returns, and a file opened after a crash or a power cut holds the last commit that returned. A deferred commit trades that for speed: it returns without waiting for the disk, survives a crash of the process, and reaches the disk within a second by default.
- Every page is verified against the check its parent recorded before it is used, so a damaged page is reported rather than read.
- The crash suites back this up: thousands of simulated power cuts, each keeping an arbitrary part of the writes in flight, and hundreds of real processes killed in the middle of a commit.
- The Node.js package opens and closes a database through that engine, and passes its errors on with the same codes. Transactions reach it once the engine's API has settled.

The work ahead, in order:

1. **Storage kernel**: measuring the kernel, then batching its disk access and keeping its free space in memory between transactions.
1. **Encryption**: page encryption and key management, with the same test suite passing with encryption on and off.
1. **Several processes**: the file lock protocol, tested by fuzzing concurrent processes that are killed at random.
1. **Objects and queries**: schemas, indexes, queries and migrations, with benchmarks.
1. **Bindings and release**: the Dart package and prebuilt binaries for every supported platform.
1. **Tools**: integrity check, salvage, backup and compaction.
