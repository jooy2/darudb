---
title: Comparison
order: 2
aside: false
pageClass: compare-page
---

# Comparing embedded databases

How DaruDB's features compare with those of the embedded databases applications use most, feature by feature, for the four languages DaruDB ships for.

Each cell marks a feature as supported, partly supported or not supported, and a note says what the mark alone would leave out. A database without a package for the language chosen in the sidebar is faded, and the checkbox above the table hides it.

<CompareTable />

## What each column describes

Each column is the database as an application installs it today:

- **SQLite** is the library itself and the bindings each language uses most. Encryption comes from extensions, which the cells name.
- **Realm** is the open-source SDKs and Realm Core. Their vendor deprecated them in September 2024 and ended support at the end of September 2025.
- **ObjectBox** is its current release, whose native core is closed source and free to use.
- **Isar** is version 3, the last stable one, as the original package and its community fork `isar_community` publish it.
- **Hive** is version 2, as the original package and its community edition `hive_ce` publish it.
- **LMDB** is the C library, versions 0.9 and 1.0, with the bindings others maintain.
- **RocksDB** and **redb** are their current releases, with the bindings others maintain.

## Speed

This page compares features only. Comparing speed needs benchmarks run the same way for every database, at the same durability settings, and those numbers will be a page of their own.

## Sources

Every cell was checked in October 2026 against the project's own documentation, its source and its package registry pages. Features change from one release to the next, so where a cell and a project's documentation disagree, the documentation is right, and an [issue](https://github.com/jooy2/darudb/issues) saying so is welcome.

To move an application's data into DaruDB from one of these databases, see the [migration guides](./migration/index.md).
