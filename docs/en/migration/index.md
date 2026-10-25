---
title: Migration
order: 1
---

# Migrating from another database

These guides move an application's data into DaruDB from the embedded database it uses now, one page for each.

To change the schema of a DaruDB file instead, see [Migrations](../guide/migrations.md) in the guide.

## How a move goes

Every guide follows the same four steps, and the old file is never changed:

1. **Declare a schema** that holds what the old database holds: a collection for each table or class, and a field for each column or property. Each guide has a table of what maps to what, and of what has no counterpart yet.
1. **Copy the data** with code that opens the old file read-only through its own library and writes objects into a new DaruDB file. Copying each record's key as the object's key keeps every reference between records valid as a link.
1. **Check the copy**: compare the number of records in each table or class with the number of objects in each collection, and run the [integrity check](../guide/tools.md#check-a-file) on the new file.
1. **Point the application at the new file**, and keep the old one until the application has run on the new one for a while.

## Where the copy runs

A server's data is in one place, so a script copies it once. An app's data is on each device, so the copy runs in the app itself, once, at the first launch of the version that moves, before the app opens the old file. The Dart guides show it so: the copy is written under another name, `app.darudb.part`, and renamed into place once it is closed, so a copy that a crash or a closed app cut short is started again at the next launch, and the old file is deleted only in a later version. Code in any other language is made safe to cut short the same way.

## Copying fast

- **Many objects to a transaction.** The copies write a thousand objects to each write transaction, which costs one commit for the thousand rather than one each.
- **Deferred commits, then close.** Each transaction commits without waiting for the disk, and closing the database at the end makes all of them durable at once. A copy that stops halfway, for any reason, leaves a file to delete and copy again, so nothing is lost by not waiting.
- **No order to follow.** A link may name an object that is not there yet, so collections can be copied in any order.

## Guides

The guides of the language chosen in the sidebar:

::: lang rust

- [SQLite](./sqlite.md): tables, rows and indexes, through `rusqlite`.
- [LMDB](./lmdb.md): named databases of bytes, through `heed`.
- [redb](./redb.md): typed tables and multimap tables.
- [sled](./sled.md): trees of bytes.

:::

::: lang node

- [SQLite](./sqlite.md): tables, rows and indexes, through `node:sqlite`.
- [Realm](./realm.md): object schemas, links and embedded objects, through `realm`.
- [LMDB](./lmdb.md): named databases, through `lmdb`.
- [LevelDB](./level.md): sublevels of JSON values, through `classic-level`.

:::

::: lang dart

- [SQLite](./sqlite.md): tables, rows and indexes, through `sqflite`, inside a Flutter app.
- [Realm](./realm.md): object schemas, links and embedded objects, through `realm`, inside a Flutter app.
- [Hive](./hive.md): boxes and type adapters, through `hive_ce`, inside a Flutter app.

:::

::: lang python

- [SQLite](./sqlite.md): tables, rows and indexes, through `sqlite3`.
- [LMDB](./lmdb.md): named databases of bytes, through `lmdb`.
- [TinyDB](./tinydb.md): tables of JSON documents.
- [shelve](./shelve.md): a shelf of pickled values.

:::
