---
title: Query language
order: 5
pageClass: reference-page
---

# Query language

The query language writes a whole query as one string, which the engine parses, so it means the same in every language. This page lists all of it, with an example of each part.

A query written as text and one made with the [query builder](./query-builder.md) become the same query inside the engine, so either can be used anywhere a query is taken, and the two can be mixed in one program. [Queries](./queries.md) covers what both have in common: how a condition treats null, lists and types, and how the engine uses indexes.

## At a glance

```text
age >= 18 AND city == "Seoul" SORT BY age DESC, name LIMIT 20 OFFSET 40
```

| Part      | Example                         | Without it                     |
| --------- | ------------------------------- | ------------------------------ |
| Filter    | `age >= 18 AND city == "Seoul"` | Every object of the collection |
| `SORT BY` | `SORT BY age DESC, name`        | Primary key order              |
| `LIMIT`   | `LIMIT 20`                      | Every object the filter finds  |
| `OFFSET`  | `OFFSET 40`                     | From the first object          |

Each part may be left out, but those that are there come in this order. An empty string is a query too, and finds every object. The collection is not part of the text: the call that runs the query names it.

## Run a query written as text

::: lang rust

`Query::parse` takes the text and the values of its parameters, and the query it returns goes to `query`, `query_one` or `count` like any other.

```rust
use darudb::{Database, Query};

fn find(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::parse("age >= $0 SORT BY age DESC LIMIT 10", &[18.into()])?;

    println!("{:?}", users.query(&query)?);
    Ok(())
}
```

:::

::: lang node

`find`, `findOne` and `count` take the text and an array of its parameters' values.

```ts
db.read((txn) => {
  const users = txn.collection('users');

  users.find('age >= $0 SORT BY age DESC LIMIT 10', [18]);
  users.findOne('email == $0', ['ada@example.com']);
  users.count('city == "Seoul"');
});
```

:::

::: lang dart

`findText`, `findOneText` and `countText` take the text and a list of its parameters' values. A raw string, `r'...'`, keeps Dart from reading `$0` as interpolation.

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  users.findText(r'age >= $0 SORT BY age DESC LIMIT 10', [18]);
  users.findOneText(r'email == $0', ['ada@example.com']);
  users.countText('city == "Seoul"');
});
```

:::

::: lang python

`find`, `find_one` and `count` take the text, and the values of its parameters after it.

```python
with db.read() as txn:
    users = txn.collection(User)

    users.find("age >= $0 SORT BY age DESC LIMIT 10", 18)
    users.find_one("email == $0", "ada@example.com")
    users.count('city == "Seoul"')
```

A text names fields as the file stores them, which is the Python name unless `field(name=...)` gave the field another.

:::

## Conditions

A condition tests one field, given by its name or by a [path](#fields-and-paths). It holds or it does not; a condition on a field that is null does not hold, except the test for null.

| Operator | Example | Finds the objects whose field |
| --- | --- | --- |
| `==` | `city == "Seoul"` | equals the value |
| `!=` | `city != "Seoul"` | is not null and differs from the value |
| `<`, `<=`, `>`, `>=` | `age >= 18` | is less than, at most, greater than, or at least the value |
| `BETWEEN` ... `AND` ... | `age BETWEEN 20 AND 29` | is between the two values, both included |
| `IN [` ... `]` | `city IN ["Busan", "Lisbon"]` | equals one of the values; `IN []` finds nothing |
| `CONTAINS` | `name CONTAINS "in"` | is a string holding the text, or a list holding the value |
| `STARTSWITH` | `name STARTSWITH "J"` | is a string that starts with the text |
| `ENDSWITH` | `email ENDSWITH "@example.com"` | is a string that ends with the text |
| `IS NULL` | `email IS NULL` | is null, which a field left out of an object is |
| `IS NOT NULL` | `email IS NOT NULL` | holds a value |
| `== null`, `!= null` | `email == null` | is null, or is not null: the same as `IS NULL` and `IS NOT NULL` |

- **Strings** compare by their bytes, so `"Z"` sorts before `"a"`, and `STARTSWITH`, `ENDSWITH` and `CONTAINS` match exactly, case included. `STARTSWITH` and `ENDSWITH` on a field that is neither a string nor a list of strings fail with `INVALID_QUERY`, and so does `CONTAINS` on one that is neither a string nor a list.
- **Lists.** A condition on a list holds when it holds for any element: `tags == "admin"` and `tags CONTAINS "admin"` both find objects with that element, and `tags STARTSWITH "ad"` finds those with an element that starts so. An empty list is not null, and holds no element.
- **Values have the field's type.** An integer field compares with an integer and never a float, a float field with a float or with an integer from -2^53 to 2^53, and a link with the linked collection's key. Any other value fails with `INVALID_QUERY`, as does an operator other than `==` and `!=` with `null`.

## Combine conditions

| Keyword     | Example                              | Holds when                      |
| ----------- | ------------------------------------ | ------------------------------- |
| `AND`       | `age >= 18 AND city == "Seoul"`      | both hold                       |
| `OR`        | `city == "Seoul" OR city == "Busan"` | either holds                    |
| `NOT`       | `NOT city == "Seoul"`                | the condition after it does not |
| `(` ... `)` | `(a == 1 OR b == 2) AND c == 3`      | grouped as the parentheses say  |

- **Precedence.** `NOT` binds tightest, then `AND`, then `OR`, so `a == 1 OR b == 2 AND c == 3` means `a == 1 OR (b == 2 AND c == 3)`.
- **`NOT` and null.** `NOT city == "Seoul"` also finds the objects whose city is null, since the condition it reverses does not hold for them. `city != "Seoul"` does not find them.
- **Depth.** Parentheses and `NOT` nest at most 48 levels in the text, and the filter they make at most 24 levels. An `AND` inside an `AND`, and an `OR` inside an `OR`, count as one level.

## Sort, limit and offset

| Clause | Example | What it does |
| --- | --- | --- |
| `SORT BY` path | `SORT BY name` | Sorts by the field, ascending |
| `SORT BY` path `ASC` | `SORT BY name ASC` | The same: `ASC` may be written or left out |
| `SORT BY` path `DESC` | `SORT BY age DESC` | Sorts by the field, descending |
| `SORT BY` paths | `SORT BY city, age DESC` | Sorts by `city`, and objects with the same city by `age`, descending |
| `LIMIT` number | `LIMIT 10` | Returns at most this many objects |
| `OFFSET` number | `OFFSET 20` | Skips this many objects first |

- **Order.** Objects that sort equal come in primary key order, and without `SORT BY` every object does. Null sorts first ascending and last descending.
- **Lists** cannot be sorted by, since an object may hold several values there; it fails with `INVALID_QUERY`.
- **Numbers.** `LIMIT` and `OFFSET` take a whole number that is not negative, written in the text: they take no parameter.
- **Pages.** `SORT BY name LIMIT 20 OFFSET 40` is the third page of twenty. A query with a limit and a sort the engine can read from an index stops reading at the limit.

## Fields and paths

A path names a field, or goes through embedded objects and links with `.`.

| Path | Example | Tests |
| --- | --- | --- |
| A field | `age > 30` | The object's own field |
| Embedded object | `address.city == "Lisbon"` | A field of the embedded object in `address` |
| Link | `team.name == "Core"` | A field of the object the link `team` points to |
| The link itself | `team == 3` | The linked object's primary key |
| A name in quotes | `` `limit` > 3 `` | A field whose name is a keyword, or not a plain word |

- **Names** are letters, digits and `_`, not starting with a digit. Any other name goes in backticks: `` `first name` ``, or a keyword such as `` `limit` ``.
- **Keywords after a dot** are names, so `team.limit` needs no backticks. A keyword only needs them where a path starts.
- **A link to an object that is not there** reads as null, so `team.name IS NULL` finds objects with no team as well as those whose team has no name.
- **Length.** A path has at most 32 names, and cannot end at an embedded object, since a condition tests one of its fields.

## Values

| Value     | Written as                                      | Examples                          |
| --------- | ----------------------------------------------- | --------------------------------- |
| Integer   | Digits, with `-` in front for a negative number | `42`, `-7`                        |
| Float     | Digits with a point, an exponent, or both       | `0.5`, `-1.25`, `6.02e23`, `1E-9` |
| String    | Double quotes                                   | `"Seoul"`, `"say \"hi\""`         |
| Boolean   | `true`, `false`                                 | `active == true`                  |
| Null      | `null`                                          | `email == null`                   |
| Parameter | `$` and the parameter's number                  | `$0`, `$1`                        |
| Bytes     | No literal: pass bytes as a parameter           | `hash == $0`                      |

A string escapes five things, and a backslash before anything else fails with `INVALID_QUERY`:

| Escape    | Means                                                |
| --------- | ---------------------------------------------------- |
| `\"`      | A double quote                                       |
| `\\`      | A backslash                                          |
| `\n`      | A line break                                         |
| `\t`      | A tab                                                |
| `\u{...}` | The character with that hexadecimal code: `\u{AC00}` |

An integer has to fit in 64 bits. `true`, `false` and `null` are case-insensitive, like every keyword.

## Parameters

`$0`, `$1` and on stand for the values passed with the text, in order, with their own types. A parameter may appear more than once, and a value can go anywhere a value is written: `age BETWEEN $0 AND $1`, `city IN [$0, $1]`.

- **Values from outside the program** belong in a parameter, never in the text. A parameter is a value whatever it holds, while text pasted into a query is read as part of the query.
- **A missing value**, a `$2` with two values given, fails with `INVALID_QUERY`.
- **Parsed once.** A query that runs often can be [prepared](./queries.md#prepare-a-query-that-runs-often), which parses it once and takes its values on each run. The Node.js, Dart and Python packages also keep up to 256 texts they have parsed, so a text run again with other values is not parsed again.

## Keywords

`AND`, `OR`, `NOT`, `BETWEEN`, `IN`, `CONTAINS`, `STARTSWITH`, `ENDSWITH`, `IS`, `NULL`, `TRUE`, `FALSE`, `SORT`, `BY`, `ASC`, `DESC`, `LIMIT` and `OFFSET`. They are case-insensitive, so `sort by age desc` is the same query, and a field with one of these names goes in backticks where a path starts.

## Errors

Text that does not parse fails with `INVALID_QUERY`, and the message says at which character, counted from 1, and what was expected there:

| Text                 | Message                                               |
| -------------------- | ----------------------------------------------------- |
| `age >=`             | at character 7: expected a value, found the end       |
| `age >= 18 LIMIT -1` | at character 17: a limit or an offset is not negative |
| `name == "Ada`       | at character 9: a string does not end                 |
| `desc == 1`          | at character 1: expected a field name, found `desc`   |

A query that parses but does not fit the collection, such as one naming a field it does not have or comparing a field with a value of another type, fails with `INVALID_QUERY` when it runs.

## Grammar

```text
query       = [ filter ] [ "SORT" "BY" sort { "," sort } ] [ "LIMIT" int ] [ "OFFSET" int ]
sort        = path [ "ASC" | "DESC" ]
filter      = and { "OR" and }
and         = not { "AND" not }
not         = "NOT" not | "(" filter ")" | condition
condition   = path compare value
            | path "BETWEEN" value "AND" value
            | path "IN" "[" [ value { "," value } ] "]"
            | path ( "CONTAINS" | "STARTSWITH" | "ENDSWITH" ) value
            | path "IS" [ "NOT" ] "NULL"
compare     = "==" | "!=" | "<" | "<=" | ">" | ">="
path        = name { "." name }
value       = int | float | string | "true" | "false" | "null" | "$" digits
```

## Examples

The examples below run against a collection of users with these fields: `name` and `city`, strings; `email`, an optional string with a unique index; `age`, an integer with an index; `tags`, a list of strings; `address`, an embedded object with `city` and `zip`; and `team`, a link to a collection of teams with a `name`.

| Query | Finds |
| --- | --- |
| `email == "ada@example.com"` | The one user with this email, looked up in the unique index |
| `age >= 18 AND age < 30` | Users from 18 to 29, read from the index on `age` |
| `age BETWEEN 18 AND 29` | The same users |
| `city IN ["Seoul", "Busan"]` | Users in either city |
| `(city == "Seoul" OR city == "Busan") AND age >= 18` | Adults in either city |
| `name STARTSWITH "Ma" SORT BY name` | Users whose name starts with `Ma`, by name |
| `email ENDSWITH "@example.com"` | Users with an address at that domain |
| `tags CONTAINS "admin"` | Users with the tag `admin` |
| `tags STARTSWITH "team-"` | Users with a tag that starts with `team-` |
| `email IS NULL` | Users without an email |
| `city != "Seoul"` | Users with a city other than Seoul |
| `NOT city == "Seoul"` | The same users, and those with no city |
| `address.city == "Lisbon"` | Users whose embedded address is in Lisbon |
| `team.name == "Core"` | Users whose linked team is named Core |
| `team == 3` | Users linked to the team whose key is 3 |
| `SORT BY age DESC, name LIMIT 10` | The ten oldest users, by name where their ages are equal |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | The third page of twenty users in Seoul |
| `age >= $0 AND city == $1` | Users of at least one age in one city, both given as values |
