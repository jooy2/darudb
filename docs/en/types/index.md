---
title: Types
order: 1
pageClass: reference-page
---

# Types

The types the API takes and returns, each on a page of its own, listed here for the language chosen in the sidebar.

::: lang rust

Every type is exported from the crate's root, as `darudb::Value`. The calls that take and return them are in [API](../api/index.md).

:::

::: lang node

The types are exported from the package's root for TypeScript: `import type { OpenOptions } from 'darudb';`. The calls that take and return them are in [API](../api/index.md).

:::

::: lang dart

The types are exported from the package's library, `package:darudb/darudb.dart`. The calls that take and return them are in [API](../api/index.md).

:::

::: lang python

The types are exported from the package's root, as `darudb.Key` or `from darudb import CheckReport`, and the package ships type hints for them. The calls that take and return them are in [API](../api/index.md).

:::

<PageList section="types" grouped />

## Field types in every language

A field of a schema holds one of these. Each package declares it in its own way and reads it as one of its own types, and a file written from one language reads the same from the others.

| A field that holds | Rust | Node.js | Dart | Python |
| --- | --- | --- | --- | --- |
| A boolean | `Type::Bool`, `bool` | `t.bool()`, `boolean` | `bool` | `bool` |
| A 64-bit integer | `Type::Int`, `i64` | `t.int()`, `number`, or `t.bigint()`, `bigint` | `int` | `int` |
| A 64-bit float | `Type::Float`, `f64` | `t.float()`, `number` | `double` | `float` |
| Text | `Type::String`, `String` | `t.string()`, `string` | `String` | `str` |
| Bytes | `Type::Bytes`, `Vec<u8>` | `t.bytes()`, `Uint8Array` | `Uint8List` | `bytes` |
| A list | `Type::list(...)`, `Vec<T>` | `t.list(...)`, an array | `List<E>` | `list[E]` |
| A link to another collection | `Type::link(...)`, `Link<T>` | `t.link(...)`, the key | `Link<T>` | the key type, with `field(link=...)` |
| An embedded object | `Type::object(...)`, a struct with `#[derive(Embedded)]` | `t.object({...})`, an object | an `@Embedded()` class | an `@darudb.embedded` class |
| Nothing, when optional | `optional(...)`, `Option<T>` | `.optional()`, `null` | a nullable type, `null` | `X \| None`, `None` |

In Rust, the first of each pair declares the field in a [`Schema`](../api/rust/schema.md) and the second is its type in a struct that [`#[derive(Object)]`](../api/rust/derive.md) makes a collection. In Node.js, the first is the [`t`](../api/node/t.md) builder and the second what an object holds. Dart and Python declare a field by its type in a class. The field types page of each language has the details.
