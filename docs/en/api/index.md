---
title: API
order: 1
---

# API

The classes and functions of the package for your language, each on a page of its own, listed here for the language chosen in the sidebar.

::: lang rust

The crate `darudb` exports everything from its root: `use darudb::{Database, OpenOptions};`. The types these calls take and return are in [Types](../types/index.md).

:::

::: lang node

The package `darudb` exports everything from its root: `import { Database, schema } from 'darudb';`. The types these calls take and return are in [Types](../types/index.md).

:::

::: lang dart

The package `darudb` exports everything from one library: `import 'package:darudb/darudb.dart';`. The generator `darudb_generator` writes a schema constant and a query builder for each annotated class. The types these calls take and return are in [Types](../types/index.md).

:::

::: lang python

The package `darudb` exports everything from its root: `import darudb`, or `from darudb import Database, Schema, collection`. The types these calls take and return are in [Types](../types/index.md).

:::

<PageList section="api" />
