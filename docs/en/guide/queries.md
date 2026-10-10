---
title: Queries
order: 4
---

# Queries

A query says which objects of a collection to find, in what order, and how many, and can be built in code or written as text.

::: tip Every operator in one place

[Query language](./query-language.md) lists every part of the text form, and [Query builder](./query-builder.md) every part of the builder in your language, each with examples.

:::

## Build a query

::: lang rust

A `Query` holds a `Filter`, a sort, a limit and an offset. A collection's `query` returns the objects it finds, and `count` counts them.

```rust
use darudb::{Database, Filter, Query};

fn adults(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let query = Query::new()
        .filter(Filter::ge("age", 18).and(Filter::starts_with("name", "A")))
        .sort_by_desc("age")
        .limit(10);

    for user in users.query(&query)? {
        println!("{:?}", user.get("name"));
    }

    let adults = users.count(&Query::new().filter(Filter::ge("age", 18)))?;
    println!("{adults} adults");
    Ok(())
}
```

The conditions are `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `between`, `is_in`, `contains`, `starts_with`, `ends_with`, `is_null` and `is_not_null`, combined with `and`, `or` and `!`. `Filter::eq(field, Value::Null)` is `is_null`.

:::

::: lang node

`find`, `findOne` and `count` take a function that builds a query on the `Query` they are given. `findOne` stops at the first object.

```ts
const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);

db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null));
  users.find((q) => q.where('tags', 'contains', 'new').where('age', 'between', [18, 30]));
  users.find((q) => q.where('team.city', '==', 'Seoul'));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
  users.count((q) => q.where('name', 'startsWith', 'A'));
});
```

`where` takes a field, an operator and a value: `==`, `!=`, `<`, `<=`, `>`, `>=`, `between`, `in`, `contains`, `startsWith` and `endsWith`. Calling it again adds a condition with AND. For anything else, give `where` a function of the `conditions`, which has each of these as a method, `isNull` and `isNotNull`, and `and`, `or` and `not`.

In TypeScript, `where` accepts only the collection's fields, and a value of the field's type. A path through an embedded object or a link, such as `team.city`, is checked by the engine when the query runs.

:::

::: lang dart

`find`, `findOne` and `count` take a function that builds a query on the query builder the generator made for the class. Each field is an object whose methods make a condition, and conditions combine with `&` (both), `|` (either) and `~` (not). `findOne` stops at the first object.

```dart
final adults = db.read(
  (txn) => txn.collection(userSchema).find(
    (q) => q.where(q.age.atLeast(18)).sortBy(q.age, descending: true).limit(10),
  ),
);

db.read((txn) {
  final users = txn.collection(userSchema);

  users.find((q) => q.where(q.email.isNull()));
  users.find((q) => q.where(q.tags.contains('new') & q.age.between(18, 30)));
  users.find((q) => q.where(q.team.city.equals('Seoul')));
  users.find((q) => q.where(q.name.equals('Alice') | q.email.isNull()));
  users.count((q) => q.where(~q.name.startsWith('A')));
});
```

The methods are `equals`, `notEquals`, `lessThan`, `atMost`, `greaterThan`, `atLeast`, `between`, `isIn`, `contains`, `startsWith`, `endsWith`, `isNull` and `isNotNull`, each on the fields it fits: `startsWith` on a string, `contains` on a string or a list. Calling `where` again adds a condition with AND, and `sortBy` again sorts the objects the first leaves equal. A link's field, `q.team`, compares the key it holds and has the linked collection's fields; an embedded object's field has its fields.

The types check every condition: `q.age.atLeast('18')` does not compile.

:::

::: lang python

`find`, `find_one` and `count` take a condition or a `Query`. `F` names a field, as `F.age`, and comparing it makes a condition; conditions combine with `&` (both), `|` (either) and `~` (not). `where` makes a condition a `Query`, which adds a sort, an offset and a limit. `find_one` stops at the first object.

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    adults = users.find(where(F.age >= 18).sort_by(F.age, descending=True).limit(10))

    users.find(F.email.is_null())
    users.find(F.tags.contains("new") & F.age.between(18, 30))
    users.find(F.team.city == "Seoul")
    users.find((F.name == "Alice") | F.email.is_null())
    users.count(~F.name.startswith("A"))
```

The tests are `==`, `!=`, `<`, `<=`, `>` and `>=`, and the methods `between`, `is_in`, `contains`, `startswith`, `endswith`, `is_null` and `is_not_null`; `== None` and `!= None` test for null too. Calling `where` on a query again adds a condition with AND, and `sort_by` again sorts the objects the first leaves equal. A path goes through an embedded object or a link by attribute, `F.address.city`, and `F["name"]` names a field whose name is also a method's.

- **Parentheses.** `&` and `|` bind more tightly than `==` or `>=` in Python, so a comparison combined with another goes in parentheses. A condition has no truth value, so `and`, `or` and `not` raise `TypeError`.
- **Python names.** A path names a field by its Python attribute, and the package gives the engine the name the file stores, which `field(name=...)` may have changed.
- **Reuse.** Every method returns a new query, and a query is compiled once for each collection it runs on, so a query kept in a variable is not compiled again on its next run.
- **Checks.** `F` is not typed, so a field the collection does not have, or a value of another type, fails with `INVALID_QUERY` when the query runs.

:::

What a condition means is the same in every language:

- **A path** names a field, or goes through an embedded object or a link with `.`: `address.city`, or `author.name` to test the linked object. A link to an object that is not there reads as null.
- **Lists.** A condition on a list holds when it holds for any element, and `contains` on a list looks for an element. An empty list is not null.
- **Null.** Every condition on a null field is false, except the test for null.
- **Types.** A value has the field's type: an integer field compares with an integer, never a float, a float field with any number, and a link with the linked collection's key. A query that breaks this, or names a field that is not there, fails with `INVALID_QUERY`.
- **Order.** Without a sort, objects come in primary key order, and objects that sort equal come in primary key order too. Null sorts first ascending and last descending. Strings compare by their bytes.

## Write a query as text

The same query can be written in the query language, which every language parses the same way, since the engine is what parses it.

::: lang rust

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let query = Query::parse(
        r#"age >= $0 AND name STARTSWITH "A" SORT BY age DESC LIMIT 10"#,
        &[18.into()],
    )?;
    let _ = query;
    Ok(())
}
```

:::

::: lang node

```ts
users.find('age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [18, 'A']);
```

The package keeps up to 256 texts it has parsed, so a text run again with other parameters is not parsed again.

:::

::: lang dart

```dart
users.findText(r'age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10', [18, 'A']);
```

The package keeps up to 256 texts it has parsed, so a text run again with other parameters is not parsed again. A raw string, `r'...'`, keeps Dart from reading `$0` as interpolation.

:::

::: lang python

```python
users.find("age >= $0 AND name STARTSWITH $1 SORT BY age DESC LIMIT 10", 18, "A")
```

The values follow the text as positional arguments. The package keeps up to 256 texts it has parsed, so a text run again with other parameters is not parsed again. A text names fields as the file stores them, where a path built with `F` names their Python attributes.

:::

A filter comes first, then `SORT BY`, `LIMIT` and `OFFSET`, each optional. Keywords are case-insensitive, strings are in double quotes, and a field named like a keyword, such as `limit`, goes in backticks. `$0`, `$1` and on take the values passed with the text. A value that comes from outside the program belongs in a parameter, never in the text. Text that does not parse fails with `INVALID_QUERY`, and the message names the character where it went wrong.

## Prepare a query that runs often

A query that runs many times with different values can be parsed once and given its values on each run. A prepared query run without values for all its parameters fails with `INVALID_QUERY`.

::: lang rust

`Query::prepare` keeps `$0`, `$1` and on as parameters, and `bind` gives them values without parsing the text again.

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["alice@example.com".into()])?;
    let _ = query;
    Ok(())
}
```

:::

::: lang node

`db.prepare` takes the collection and the query, as text or built with `param` in place of the values that change. `find`, `findOne` and `count` take the prepared query and the values, in any transaction, synchronous or asynchronous.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const inAges = db.prepare('users', 'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) => {
  const users = txn.collection('users');

  users.findOne(byEmail, ['alice@example.com']);
  users.find(inAges, [18, 30]);
});
```

A prepared query runs only on the collection it was prepared on.

:::

::: lang dart

`db.prepare` takes the schema constant and the text. `findPrepared`, `findOnePrepared` and `countPrepared` take the prepared query and the values, in any transaction, synchronous or asynchronous.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');
final inAges = db.prepare(userSchema, r'age BETWEEN $0 AND $1 SORT BY age');

db.read((txn) {
  final users = txn.collection(userSchema);

  users.findOnePrepared(byEmail, ['alice@example.com']);
  users.findPrepared(inAges, [18, 30]);
});
```

A prepared query runs only on the collection it was prepared on.

:::

::: lang python

`db.prepare` takes the class and the query, as text or built with `param` in place of the values that change. `find`, `find_one` and `count` take the prepared query and the values after it, in any transaction, synchronous or asynchronous.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))
in_ages = db.prepare(User, "age BETWEEN $0 AND $1 SORT BY age")

with db.read() as txn:
    users = txn.collection(User)

    users.find_one(by_email, "alice@example.com")
    users.find(in_ages, 18, 30)
```

A prepared query runs only on the collection it was prepared on, and fails with `INVALID_QUERY` on another.

:::

Preparing saves parsing or encoding the query on each run, which matters most for text. The engine still plans each run for its values.

## How the engine reads

A condition on the primary key or on an indexed field, joined to the rest of the filter with AND, lets the engine read only the objects that meet it. A query sorted by an indexed field alone reads its objects in that order and stops at the limit. Otherwise the engine reads every object of the collection. Whichever way it reads, the result is the same.
