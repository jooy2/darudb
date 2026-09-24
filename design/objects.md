# Objects and queries

Status: accepted.

The object layer of phase 4: collections of typed objects, a schema that says what they hold, indexes, queries, and migrations from one schema version to the next. It is built on the storage kernel's named trees of byte keys and byte values ([File format](file-format.md)), inside the kernel's transactions, and changes nothing below it: a commit of objects is a commit of trees, with the durability, recovery and locking the other documents give it.

The decisions this document builds on are recorded in `CLAUDE.md`, under "API and query model": two query forms compiled to one IR, migrations by schema version, a primary key that is a declared field or an auto-increment, and a Rust schema declared with a builder at run time.

## Goals

- An application declares its data once, as a schema, and reads and writes typed objects rather than bytes.
- A query is answered from an index when one fits, and otherwise by a scan, and gives the same result either way.
- A query result is a set of plain objects, copies that outlive the transaction and the database.
- Every language binding reaches the same engine through the same two byte formats, one for objects and one for queries, so every binding behaves the same way.
- A file written with one schema version opens with the next, and a file written by a newer application is refused rather than misread.

## Data model

A database holds **collections**. A collection holds **objects** of one shape, each identified by its **primary key**. An object is a set of **fields**, each with a name and a value of the field's type.

| Type        | Values                                                                       |
| ----------- | ---------------------------------------------------------------------------- |
| `bool`      | `false`, `true`                                                              |
| `int`       | Signed 64-bit integers                                                       |
| `float`     | 64-bit IEEE 754 numbers                                                      |
| `string`    | UTF-8 text                                                                   |
| `bytes`     | Any bytes                                                                    |
| `link(C)`   | The primary key of an object in collection `C`: a to-one link                |
| `list(T)`   | A list of values of a scalar type or of `link(C)`: a to-many link, or a list |
| `object(E)` | An embedded object: fields of its own, stored inside the owning object       |

- **Optional fields.** A field is required unless it is declared optional. An optional field may be null, which is also what it holds when it is left out. A required field may have a **default**, which it holds when it is left out: the object is written with the default, so that changing the default later changes no object already written.
- **Strings compare by bytes.** Their order is the order of their UTF-8 bytes, which is the order of their code points. There is no collation and no case folding in v1.
- **Floats.** `-0.0` equals `0.0`, and every NaN equals every other and sorts after positive infinity, so that a float field has a total order an index can keep.
- **Links** hold the target's primary key and nothing else. A link to an object that does not exist, or no longer does, is allowed and reads as the key it holds; the engine does not cascade deletes in v1.
- **Backlinks** are not stored. The objects that link to an object are the answer to a query on the link field, which an index on that field makes cheap.
- **Embedded objects** have no primary key and no collection of their own. They are read and written with the object that holds them, and their fields can be queried through it.
- **Lists** hold values of one type and no nulls. Most comparisons on a list field hold when they hold for any element ([Queries](#queries)).

A date or a time has no type of its own in v1. A binding stores one as an `int`, which is the Unix time in the unit it names, and a later version may add a type for it.

## Primary keys

A collection's primary key is one of these, chosen when the collection is declared:

- **A declared field** of type `int`, `string` or `bytes`. It is required, and an object's key never changes: writing an object under another key is a new object.
- **An auto-increment**: when no field is named, the collection gets a required `int` field called `id`, and an object written without it gets the next number, from 1 up, never reused in that file, not even after the object is deleted. An object written with an `id` of its own keeps it, and the next number is raised past it if it is lower.

A string or bytes key is limited by the kernel's key length: its encoding ([Keys](#keys)) has to fit in `⌊(C − 196) / 4⌋` bytes, 957 with 4096-byte pages. An object whose key does not fit is refused with `INVALID_ARGUMENT`.

## Schemas

The application declares a **schema**: a version number from 1 up, and its collections, each with its fields, its primary key and its indexes. The engine stores the schema in the file the first time, and compares the declared one with the stored one each time the file is opened.

The stored schema adds numbers the declared one does not have, assigned by the engine and never reused within the file:

- **Collection ids**, which name a collection's trees, so that renaming a collection moves no data.
- **Field ids**, unique within a collection or an embedded object, which records hold in place of field names, so that renaming a field rewrites no record.
- **Index ids**, which name an index's tree.

A declared schema matches a stored one by names: a collection or a field keeps its id for as long as it keeps its name, and a migration that renames one says so ([Migrations](#migrations)). The stored schema lists collections, fields and indexes by id, so declaring the same ones in another order is the same schema.

## Storage

Every tree of the object layer has a name that begins with the byte `0x00`, which [File format](file-format.md#the-catalog) reserves for the engine, so that no tree an application creates through the kernel's API can collide with one. The kernel's `tree_names` does not list them.

| Tree      | Key                              | Value                                              |
| --------- | -------------------------------- | -------------------------------------------------- |
| `\0meta`  | `schema`                         | The [stored schema](#the-stored-schema)            |
| `\0meta`  | `next/` and a collection id      | The next auto-increment number, 8 bytes, LE        |
| `\0rec/n` | The object's encoded primary key | The object, as a [record](#records)                |
| `\0idx/n` | An [index](#indexes) entry       | Empty, or the encoded primary key for a unique one |

`n` is the collection id or the index id in decimal ASCII, since tree names are text, and so is the collection id after `next/`. Writing an object writes its record and every index entry it changes, in the same kernel write transaction, so a commit never shows an index that disagrees with its records.

## Keys

Keys order as unsigned bytes and nothing else ([File format](file-format.md#key-order)), so the object layer encodes each value so that its bytes order the way the value does. Each encoding is a tag byte followed by the value:

| Tag    | Value  | Bytes after the tag                                                                     |
| ------ | ------ | --------------------------------------------------------------------------------------- |
| `0x01` | null   | None                                                                                    |
| `0x02` | false  | None                                                                                    |
| `0x03` | true   | None                                                                                    |
| `0x04` | int    | 8 bytes, big-endian, of the value with its sign bit flipped                             |
| `0x05` | float  | 8 bytes, big-endian, of the bits, all flipped if the sign bit is set, else the sign bit |
| `0x06` | string | The bytes, each `0x00` written as `0x00 0xFF`, then `0x00 0x00`                         |
| `0x07` | bytes  | As for a string                                                                         |

- **Null sorts first**, before every value of its field.
- **Floats** are canonicalised before encoding: `-0.0` becomes `0.0` and every NaN becomes the quiet NaN `0x7FF8000000000000`.
- **Strings and bytes end in `0x00 0x00`**, and a `0x00` inside them becomes `0x00 0xFF`, so an encoding is never a prefix of another's continuation: `"a"` sorts before `"a\0"`, which sorts before `"ab"`. That is what lets encodings be concatenated.
- **Concatenation orders field by field.** The encoding of `(a, b)` sorts as `a`, then `b`, which is what index entries rely on.

## Records

An object's value in its collection's tree is a record. The same format carries objects between a binding and the engine, and holds embedded objects and the stored schema.

```text
record  = count field*          the fields present, ascending by field id
field   = id value
value   = tag payload
```

`count` and `id` are unsigned LEB128 varints, and a field whose value is null is left out. The payload depends on the tag:

| Tag    | Type         | Payload                             |
| ------ | ------------ | ----------------------------------- |
| `0x02` | `bool` false | None                                |
| `0x03` | `bool` true  | None                                |
| `0x04` | `int`        | Zigzag LEB128 varint                |
| `0x05` | `float`      | 8 bytes, little-endian              |
| `0x06` | `string`     | Length varint, then the UTF-8 bytes |
| `0x07` | `bytes`      | Length varint, then the bytes       |
| `0x08` | `list(T)`    | Count varint, then that many values |
| `0x09` | `object(E)`  | Length varint, then a record        |
| `0x0A` | `link(C)`    | A value: the target's primary key   |

- **Reading a record.** A field absent from a record, which only a record written before the field existed can be, holds its default, or null if it has none. A field id the schema no longer has belongs to a field that was removed, and is skipped. A record read from the file is untrusted input: a varint that runs out, a length past the end, a tag that does not match the field's type, or ids out of order make it `CORRUPTED`.
- **Writing a record.** The engine checks a record it is given against the schema, types, required fields and all, before it stores it, and refuses one that does not fit with `INVALID_ARGUMENT`. A record from a binding is checked like any other.
- **Why this format.** It is compact for the common case, small integers and short strings, and cheap to write in any language, which is what makes it the format bindings exchange with the engine. Field ids rather than names keep it short and make renaming free.

### The stored schema

The stored schema is a record too, of this shape, where every `object` is itself a record of the shape its row gives:

| Record     | Field | Value                                                                                                |
| ---------- | ----- | ---------------------------------------------------------------------------------------------------- |
| Schema     | 1     | `int`: the object layer's format, 1                                                                  |
|            | 2     | `int`: the schema version                                                                            |
|            | 3     | `list(object)`: the collections                                                                      |
|            | 4     | `int`: the next collection id                                                                        |
|            | 5     | `int`: the next index id                                                                             |
| Collection | 1     | `int`: its id                                                                                        |
|            | 2     | `string`: its name                                                                                   |
|            | 3     | `list(object)`: its fields                                                                           |
|            | 4     | `int`: the next field id                                                                             |
|            | 5     | `int`: the primary key's field id                                                                    |
|            | 6     | `bool`: whether the primary key is an auto-increment                                                 |
|            | 7     | `list(object)`: its indexes                                                                          |
| Field      | 1     | `int`: its id                                                                                        |
|            | 2     | `string`: its name                                                                                   |
|            | 3     | `object`: its type                                                                                   |
|            | 4     | `bool`: whether it is optional                                                                       |
|            | 5     | Its default, a value of its type, if it has one                                                      |
| Type       | 1     | `int`: the kind: 1 `bool`, 2 `int`, 3 `float`, 4 `string`, 5 `bytes`, 6 `link`, 7 `list`, 8 `object` |
|            | 2     | `int`: a link's target collection id                                                                 |
|            | 3     | `object`: a list's element type                                                                      |
|            | 4     | `list(object)`: an embedded object's fields                                                          |
|            | 5     | `int`: an embedded object's next field id                                                            |
| Index      | 1     | `int`: its id                                                                                        |
|            | 2     | `int`: its field's id                                                                                |
|            | 3     | `bool`: whether it is unique                                                                         |

A stored schema that does not decode, or whose ids repeat or point nowhere, is `CORRUPTED`. Field 1 lets a later layout change be told apart; a build refuses a format it does not know with `UNSUPPORTED_FORMAT_VERSION`.

## Indexes

An index is on one field of a collection, scalar or `link`, and v1 has two kinds:

- **Non-unique**: an entry per object, whose key is the encoded field value followed by the encoded primary key, and whose value is empty. Objects with one value sort together, in key order.
- **Unique**: an entry per object, whose key is the encoded field value and whose value is the encoded primary key. Writing an object whose value another object has already fails with `DUPLICATE_KEY`. Nulls are not unique: an object whose value is null has an entry whose key is the null encoding followed by its primary key, as in a non-unique index.

An index on a list field holds an entry for each distinct element. An index on a field inside an embedded object is not in v1.

An indexed value's entry has to fit in the kernel's key length, together with the primary key. A string or bytes value too long for that is refused with `INVALID_ARGUMENT` when the object is written.

## Writing objects

A write transaction offers these for a collection:

- **Insert** an object: fails with `DUPLICATE_KEY` if its key is taken.
- **Put** an object: inserts it, or replaces the object with its key.
- **Delete** the object with a key: returns whether there was one.

Each writes the record and adds, changes or removes the index entries whose values changed, checking unique indexes first. Every check that can refuse a write, the object against the schema, its key, the unique indexes and the length of every key it adds, happens before any tree changes, so a refused write leaves the transaction as it was and able to commit. Several objects in one call cost one crossing of the language boundary: the bindings' batch calls take a sequence of records.

## Queries

A query names a collection and holds, all optional:

- **A filter**: an expression that an object has to satisfy.
- **A sort**: fields, each ascending or descending. Null sorts first ascending and last descending. Without a sort, objects come in primary key order, and objects that sort equal come in primary key order too.
- **An offset and a limit** on the sorted result.

A query either returns the objects or counts them. A count is of the objects the query would return, after its offset and within its limit.

### The filter

| Expression                              | Holds when                                                            |
| --------------------------------------- | --------------------------------------------------------------------- |
| `path == v`, `!=`, `<`, `<=`, `>`, `>=` | The field compares so with `v`                                        |
| `path BETWEEN a AND b`                  | `a <= path <= b`                                                      |
| `path IN [v, ...]`                      | The field equals one of the values                                    |
| `path CONTAINS s`                       | A string field contains `s`, or a list field contains the element `s` |
| `path STARTSWITH s`, `ENDSWITH s`       | A string field starts or ends with `s`                                |
| `path IS NULL`, `IS NOT NULL`           | The field is null, or is not                                          |
| `a AND b`, `a OR b`, `NOT a`            | As in logic                                                           |

- **A path** names a field, and follows embedded objects and links with `.`: `address.city`, `author.name`. A path through a to-one link reads the linked object, and holds as null if there is none. A path has at most 32 names, since one through a link back into its own collection could otherwise go on without end.
- **A path to a list**, or through a to-many link, holds when it holds for any element: `tags == "red"` for an object with a red tag. An empty list has no element, so nothing holds for it but `IS NOT NULL`: `IS NULL` holds when the list itself is null. `CONTAINS` on a list compares elements for equality, and `STARTSWITH` and `ENDSWITH` on a list of strings hold when an element does.
- **A null field** makes every condition on it false except `IS NULL`, and `IS NOT NULL` true only for a field that is not null. `path == null` means `path IS NULL`, and `path != null` means `path IS NOT NULL`; `null` anywhere else is `INVALID_QUERY`.
- **A value has to fit the field's type.** An `int` compares with an `int` and a `float` with a `float`. An `int` compared with a `float` field is taken as the float it equals, when it is within 2^53 either side of zero, since a language with one type of number cannot tell `1` from `1.0`. Any other comparison across types, a `float` with an `int` field included, is `INVALID_QUERY`. So is sorting by a list, a link through a to-many link, or an embedded object.
- **A filter nests at most 24 levels deep.** `AND` inside `AND`, and `OR` inside `OR`, flatten into one level, so only `NOT` and alternating groups count. Deeper is `INVALID_QUERY`, which keeps every query inside the nesting a record allows.

### The IR

A query is a tree that every binding builds and the engine executes. It crosses the language boundary in one buffer, as a record of a fixed shape:

| Field | Name       | Value                                                              |
| ----- | ---------- | ------------------------------------------------------------------ |
| 1     | collection | `string`                                                           |
| 2     | filter     | `object`: an expression, below                                     |
| 3     | sort       | `list(object)`: each a path (`list(string)`) and `bool` descending |
| 4     | offset     | `int`                                                              |
| 5     | limit      | `int`                                                              |
| 6     | count      | `bool`: count the objects rather than return them                  |

An expression is an object: field 1 is its operator, field 2 the path it tests as a `list(string)`, field 3 its values as a list of the field's type, and field 4 its sub-expressions as a `list(object)`. A value may instead be a **parameter**: an `object` whose field 1 is the parameter's number as an `int`, which no value a query compares with is. A query with parameters is a **prepared** one: it is kept, and given values each time it runs, without being built, sent or parsed again. Giving it values makes the tests they complete what the same values in the text would, `== null` becoming `IS NULL` among them; running it with a parameter that has no value is `INVALID_QUERY`.

| Operator | Code | Uses               | Operator     | Code | Uses             |
| -------- | ---- | ------------------ | ------------ | ---- | ---------------- |
| `AND`    | 1    | sub-expressions    | `>=`         | 9    | path, one value  |
| `OR`     | 2    | sub-expressions    | `BETWEEN`    | 10   | path, two values |
| `NOT`    | 3    | one sub-expression | `IN`         | 11   | path, any values |
| `==`     | 4    | path, one value    | `CONTAINS`   | 12   | path, one value  |
| `!=`     | 5    | path, one value    | `STARTSWITH` | 13   | path, one value  |
| `<`      | 6    | path, one value    | `ENDSWITH`   | 14   | path, one value  |
| `<=`     | 7    | path, one value    | `IS NULL`    | 15   | path             |
| `>`      | 8    | path, one value    |              |      |                  |

`IS NOT NULL` is `NOT` over `IS NULL`. An `AND` inside an `AND`, and an `OR` inside an `OR`, flatten when the IR is read, as they do when a query is built. An IR that does not decode, uses an unknown operator, or leaves out what its operator uses, is `INVALID_QUERY`.

### The query language

The string form, parsed by the engine into the same IR. The collection is not part of it: the call that runs a query names it.

```text
query       = [ expr ] [ "SORT" "BY" sort { "," sort } ] [ "LIMIT" int ] [ "OFFSET" int ]
sort        = path [ "ASC" | "DESC" ]
expr        = and { "OR" and }
and         = not { "AND" not }
not         = "NOT" not | "(" expr ")" | condition
condition   = path compare value
            | path "BETWEEN" value "AND" value
            | path "IN" "[" [ value { "," value } ] "]"
            | path ( "CONTAINS" | "STARTSWITH" | "ENDSWITH" ) value
            | path "IS" [ "NOT" ] "NULL"
compare     = "==" | "!=" | "<" | "<=" | ">" | ">="
path        = name { "." name }
value       = int | float | string | "true" | "false" | "null" | "$" digits
```

- **Keywords** are case-insensitive: `AND`, `OR`, `NOT`, `BETWEEN`, `IN`, `CONTAINS`, `STARTSWITH`, `ENDSWITH`, `IS`, `NULL`, `TRUE`, `FALSE`, `SORT`, `BY`, `ASC`, `DESC`, `LIMIT` and `OFFSET`.
- **A name** is letters, digits and `_`, not starting with a digit, or any text in backticks. Where a path starts, a keyword is a keyword, so a field with a keyword's name goes in backticks there; after a `.`, every word is a name.
- **Strings** are in double quotes, with `\"`, `\\`, `\n`, `\t` and `\u{...}` escapes. An **int** has no point and a **float** has one or an exponent, and either may start with `-`.
- **Parameters** `$0`, `$1` and so on take the values passed with the call, in order and with their types. Text parsed without values keeps them as parameters, for a prepared query. A value that comes from outside the application belongs in a parameter, never in the text.
- **A query that does not parse**, or does not fit the schema, fails with `INVALID_QUERY`, naming the position, as a character counted from 1, and what was expected. Parentheses and `NOT` nest at most 48 levels in the text, twice the filter's own limit, so that parsing stops before the filter would be refused anyway.

## Running a query

The engine chooses how to find the objects, and the choice never changes the result: every query also has a plain answer, a scan of the collection with the filter applied to each object, and the tests compare the two ([Tests](#what-the-phase-4-tests-must-show)).

1. **Find an access path.** Among the conditions that every matching object has to meet, the terms of the filter's top-level `AND`, pick one on a field of the collection itself that an index or the primary key answers: an equality, an `IN`, a range from comparisons or `BETWEEN`, a `STARTSWITH`, which is a range on a string's encoding, or an `IS NULL` on an indexed field, which is an equality with null. An equality or a `CONTAINS` on a list field is an equality in its index. The order of preference is an equality on the primary key or a unique index, then an `IN` on them, then an equality or an `IN` on another index, then a range; the range terms on the chosen field narrow one range together, unless the field is a list, where each term may hold for another element. With none, walk the index of the one field the query sorts by, if it sorts by an indexed field alone, or else every record.
2. **Read the candidates** from the access path, in key order or in reverse. One value of a unique index, other than null, is one entry, which is looked up rather than walked.
3. **Apply the rest of the filter** to each candidate, reading linked objects where a path goes through a link.
4. **Sort.** When the access path already delivers the query's order, the objects stream in that order and the limit stops the reading early. That holds when the sort is on the path's field alone and the field is not a list, and when there is no sort and the path is the primary key or one value of an index. An index walked in reverse gives the objects of one value in descending key order, so they are held back and given in ascending key order; a value with more than 64 objects is instead walked forwards from its first entry, so that a limit stops the reading there too. Otherwise the matching objects are sorted in memory, keeping only the best `offset + limit` when there is a limit. Without a sort, the order is the primary key's, so a candidate whose key comes after the kept ones is dropped before its object is read, and when the index answers the whole filter, only the objects of the result are read, once the kept keys are known.
5. **Skip the offset and stop at the limit.**

When the access path answers the whole filter, the rest of the filter is empty. A count then adds up the entries of the path's ranges a leaf of the tree at a time, without reading them out or reading the objects, unless an object can have several entries there, as in the index of a list field, where it follows the same steps without reading the objects. A count without a filter reads only the number of records the collection's tree keeps.

Walking an index in reverse is the kernel's backward walk of a tree, the one its `range_backward` offers.

## Migrations

Opening a file with a declared schema compares it with the stored one:

| Stored schema                     | Outcome                                                             |
| --------------------------------- | ------------------------------------------------------------------- |
| None                              | The declared schema is stored, in one write transaction             |
| The same version, the same schema | Nothing to do                                                       |
| The same version, another schema  | `SCHEMA_MISMATCH`: the application changed it without a new version |
| A newer version                   | `SCHEMA_TOO_NEW`: a newer application wrote the file                |
| An older version                  | A migration, below                                                  |

The comparison happens again inside the write transaction that stores or migrates the schema, so two processes that open an old file at once migrate it once: the second finds the new schema stored and has nothing to do.

A migration from version `m` to version `n` runs in one write transaction, and the file is untouched if any part of it fails:

1. **Renames.** Each version step from `m + 1` to `n` may name collections and fields that it renames. The stored schema takes the new names, and the ids stay.
2. **The engine's own changes**, from the renamed stored schema to the declared one:
   - A new collection is created, empty.
   - A new field is added. A required one needs a default, or declaring the schema fails with `INVALID_ARGUMENT`. No record is rewritten: a record without the field reads its default. Changing the default of an existing field changes what such records read, so the indexes on that field are built again, and a required field keeps its default once it has one: taking it away is `INVALID_ARGUMENT`. A float default is stored canonical, so a default of NaN or `-0.0` is the same schema every time it is declared.
   - A removed field's id is retired. Records keep its value until they are next written, and the migration functions below can still read it by its old name.
   - A new index is built from the objects, which can fail with `DUPLICATE_KEY`, and a removed one is deleted.
   - A field whose type changed, or a removed collection, is not a change the engine makes by itself: the step has to name it, as a field it replaces or a collection it deletes. A replaced field is a removed field and a new field with the same name, so a required one needs a default too. A primary key cannot be replaced.
3. **The migration functions**, one per version step that has one, in version order. Each gets the write transaction under the declared schema, and can read, write and delete objects to move data across. The objects as the schema before the migration reads them stay readable too, by the names that schema gave collections and fields, with the values of removed and replaced fields: an object reads that way as it is at the time, and one a function has already written reads the fields the new schema dropped as a record without them does, as their defaults or null.
4. **The stored schema** becomes the declared one, with its version, and the collections the steps delete go, last, so that the functions can still read them.

Inside the one write transaction, the engine stores the new schema before it runs the functions, so that they write objects through the same checks as any other transaction. Nothing outside the transaction can tell the order apart.

A binding runs the migration functions in its own language. The engine does not call back into it: opening stops once the new schema is stored and its indexes are built, and hands the migration's write transaction to the binding, which asks for the version steps one at a time, runs its function for each in the same transaction, and then asks the engine to delete what the steps delete and commit. Dropping the migration instead leaves the file as it was.

### Several processes

A process that opened the file with one schema can meet another process's migration. Each time a transaction reaches a collection, it compares the stored schema's record with the one the handle opened with, which costs a lookup of a page that is almost always cached, and fails with `SCHEMA_MISMATCH` when they differ: the handle has to be opened again with the application's new schema. A read transaction compares with the commit it sees, so one that began before the migration goes on reading under the old schema. Opening while another process migrates waits for the writer lock, as any write does.

The same holds between handles in one process. Each handle keeps the schema it was opened with, so a handle opened with a new schema migrates the file under the others, which fail the same way.

## Errors

This document adds five error codes to the engine:

| Code               | When                                                                                                |
| ------------------ | --------------------------------------------------------------------------------------------------- |
| `SCHEMA_MISMATCH`  | The declared schema differs from the stored one at the same version, or another process migrated it |
| `SCHEMA_TOO_NEW`   | The file's schema version is newer than the declared one                                            |
| `DUPLICATE_KEY`    | An insert found its primary key taken, or a unique index found its value taken                      |
| `INVALID_QUERY`    | A query does not parse, or names a field or compares a value that does not fit the schema           |
| `MIGRATION_FAILED` | An application's migration function reported an error, whose message this one carries               |

An engine error inside a migration function keeps its own code: a function that meets `DUPLICATE_KEY` and returns it ends the migration with `DUPLICATE_KEY`.

## What the phase 4 tests must show

- **Keys order as values do.** For random values of every type, including the edges of each, encodings compare as the values do, and concatenations compare field by field.
- **Records survive a round trip**, and random bytes read as a record give `CORRUPTED` or a valid object, never a panic.
- **Indexes agree with records.** After random writes, and after every cut of the crash suite, every index entry names an object that has the entry's value, and every object has its entries.
- **Every query gives the scan's answer.** Random queries over random data, run through the planner and as a plain scan, return the same objects in the same order; the IR a builder makes and the IR the query language parses from the same query are the same.
- **Migrations are atomic.** A migration interrupted anywhere, by a failing function or a simulated power cut, leaves the file at the old schema with its data, and a completed one leaves every object readable under the new schema.
- **Several processes.** A process whose schema another process migrated fails with `SCHEMA_MISMATCH` rather than reading or writing under the old schema.

## Benchmarks

Phase 4 ends with benchmarks against established embedded databases at the same durability settings. This repository names no other database, so the comparison runs from outside it: `examples/` measures DaruDB's object layer on its own, the same workloads run elsewhere against the others, and the results are reported without the others' names here.

## Not in v1

Composite indexes, indexes inside embedded objects, collation and case-insensitive comparison, full-text search, aggregates other than count, joins beyond following links, a date and time type, and cascading deletes.
