---
title: Query
order: 10
---

# Query

`Query` says what to find in a collection, in what order, and how many.

```rust
#[derive(Debug, Clone, Default)]
pub struct Query
```

A query is built with methods from `Query::new`, or parsed from text with [`parse`](#parse) or [`prepare`](#prepare); both build the same query. It is not tied to a collection or a transaction, so one query can run many times. [`CollectionReader::query`](./collection-reader.md#query) and [`count`](./collection-reader.md#count) run it, and check it against the collection's schema then: a field that is not there, or a value of the wrong type, fails with `INVALID_QUERY`.

Without a sort, objects come in primary key order, and objects that sort equal come in primary key order too. Two queries are equal when they find the same objects in the same way, so a bound prepared query equals the query with the same values written in. `Query` is `Send` and `Sync`.

```rust
use darudb::{Database, Filter, Query};

fn adults(db: &Database) -> darudb::Result<()> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);

    for user in users.query(&query)? {
        println!("{:?}", user.get("name"));
    }

    Ok(())
}
```

A condition on the primary key or on an indexed field, joined to the rest of the filter with `and`, lets the engine read only the objects that meet it. A query sorted by an indexed field alone reads its objects in that order and stops at the limit. Otherwise the engine reads every object of the collection. [Queries](../../guide/queries.md) has the details.

## Associated functions

### new

```rust
pub fn new() -> Self
```

A query that finds every object of the collection, in primary key order. It is the same as `Query::default()`.

### parse

```rust
pub fn parse(text: &str, parameters: &[Value]) -> Result<Self>
```

Parses `text` in the query language, with `parameters` for `$0`, `$1` and on. A filter comes first, then `SORT BY`, `LIMIT` and `OFFSET`, each optional. Text that does not parse, or names a parameter that `parameters` does not have, fails with `INVALID_QUERY`, and the message names the character where it went wrong.

```rust
use darudb::Query;

fn main() -> darudb::Result<()> {
    let query = Query::parse(
        r#"age >= $0 AND (name STARTSWITH "A" OR tags CONTAINS "admin")
           SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;
    let _ = query;

    Ok(())
}
```

Each condition of [`Filter`](./filter.md) has a form in the text:

| Text                                     | Filter                                 |
| ---------------------------------------- | -------------------------------------- |
| `f == v`, `f != v`                       | `eq`, `ne`                             |
| `f < v`, `f <= v`, `f > v`, `f >= v`     | `lt`, `le`, `gt`, `ge`                 |
| `f BETWEEN v AND w`                      | `between`                              |
| `f IN [v, w]`                            | `is_in`                                |
| `f CONTAINS v`, `STARTSWITH`, `ENDSWITH` | `contains`, `starts_with`, `ends_with` |
| `f IS NULL`, `f IS NOT NULL`             | `is_null`, `is_not_null`               |
| `a AND b`, `a OR b`, `NOT a`, `(a)`      | `and`, `or`, `!`                       |

- **Precedence.** `NOT` binds tightest, then `AND`, then `OR`; parentheses group.
- **Sorting.** `SORT BY f DESC, g` sorts by several paths, each ascending unless `DESC` follows it. `LIMIT` and `OFFSET` take a number that is not negative.
- **Names.** A field is a word, a path joins names with `.`, and a name that is a keyword or not a plain word goes in backticks, such as `` `limit` ``. Keywords are case-insensitive.
- **Values.** Integers, numbers with a point or an exponent, which are floats, strings in double quotes, `true`, `false` and `null`. A string escapes `\"`, `\\`, `\n`, `\t` and `\u{...}`. Bytes have no literal and are given as a parameter.
- **Parameters.** `$0`, `$1` and on take the values passed with the text. A value that comes from outside the application belongs in a parameter, never in the text.

### prepare

```rust
pub fn prepare(text: &str) -> Result<Self>
```

Parses `text` as `parse` does, keeping `$0`, `$1` and on as parameters, for a query that runs many times with different values: [`bind`](#bind) gives it values without parsing it again. Running a prepared query that has not been bound fails with `INVALID_QUERY`.

```rust
use darudb::Query;

fn main() -> darudb::Result<()> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["alice@example.com".into()])?;
    let _ = query;

    Ok(())
}
```

## Methods

### filter

```rust
pub fn filter(mut self, filter: Filter) -> Self
```

Keeps only the objects that meet `filter`, and those of any filter given before: two calls are joined with `and`.

### sort_by

```rust
pub fn sort_by(mut self, field: &str) -> Self
```

Sorts by `field`, a name or a path, in ascending order, after any sort given before. Null sorts first, and strings compare by their bytes. A field that holds several values, such as a list, cannot be sorted by: the query fails with `INVALID_QUERY` when it runs.

### sort_by_desc

```rust
pub fn sort_by_desc(mut self, field: &str) -> Self
```

Sorts by `field` in descending order, after any sort given before. Null sorts last.

### offset

```rust
pub fn offset(mut self, count: u64) -> Self
```

Skips the first `count` objects of the result. A later call replaces an earlier one.

### limit

```rust
pub fn limit(mut self, count: u64) -> Self
```

Returns at most `count` objects. A later call replaces an earlier one.

### first

```rust
pub fn first(mut self) -> Self
```

Returns at most the first object: a limit of one, or of none if the query's own limit is zero. A lookup of one object stops reading there.

### bind

```rust
pub fn bind(&self, parameters: &[Value]) -> Result<Self>
```

This prepared query with `parameters` for its `$0`, `$1` and on. The bound query shares the prepared one's parsed form rather than copying it. Fewer values than the query names fail with `INVALID_QUERY`. A query without parameters, or one bound already, comes back as it is.

### bind_encoded

```rust
pub fn bind_encoded(&self, parameters: &[u8]) -> Result<Self>
```

`bind` with the parameters as a language binding sends them, in one buffer: a record whose field 0 is how many there are and field `n + 1` the value of parameter `n`, a null one left out. A Rust program uses `bind`. [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md#the-ir) specifies the encoding.
