---
title: Query builder
order: 6
pageClass: reference-page
---

# Query builder

The query builder makes a query in your language's code, one condition at a time. This page lists every part of it for the language chosen in the sidebar, each beside the same query in the [query language](./query-language.md).

A built query and one written as text become the same query inside the engine, and [Queries](./queries.md) covers what they have in common: how a condition treats null, lists and types, and how the engine uses indexes. The API section has a page for each type the builder is made of, linked from each part below.

## Run a query

::: lang rust

A [`Query`](../api/rust/query.md) holds a [`Filter`](../api/rust/filter.md), a sort, a limit and an offset, and a collection runs it. Building one never fails: it is checked against the collection's schema when it runs, and a field that is not there, or a value of another type, fails with `INVALID_QUERY` then.

```rust
use darudb::{Database, Filter, Query};

fn find(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;
    let adults = Query::new().filter(Filter::ge("age", 18));

    let found = users.query(&adults)?; // Vec<Object>
    let first = users.query(&adults.clone().first())?; // at most one
    let how_many = users.count(&adults)?; // u64

    println!("{found:?} {first:?} {how_many}");
    Ok(())
}
```

`collection_of::<T>()` runs the same queries and returns the objects as `T`, for a struct with `#[derive(Object)]`.

:::

::: lang node

`find`, `findOne` and `count` take a function that builds the query on the [`Query`](../api/node/query.md) it is given. In TypeScript, the builder accepts only the collection's fields, and values of their types.

```ts
db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('age', '>=', 18)); // every adult
  users.findOne((q) => q.where('email', '==', 'ada@example.com')); // the first, or null
  users.count((q) => q.where('age', '>=', 18)); // a number
});
```

A query can also be made outside a call, with `new Query()`, and passed in place of the function.

:::

::: lang dart

`find`, `findOne` and `count` take a function that builds the query on the [query builder](../api/dart/query-builder.md) the generator wrote for the class. Each field of the class is a [field object](../api/dart/fields.md) whose methods make conditions, and the types check every one: `q.age.atLeast('18')` does not compile.

```dart
db.read((txn) {
  final users = txn.collection(userSchema);

  users.find((q) => q.where(q.age.atLeast(18))); // every adult
  users.findOne((q) => q.where(q.email.equals('ada@example.com'))); // the first, or null
  users.count((q) => q.where(q.age.atLeast(18))); // an int
});
```

:::

::: lang python

`find`, `find_one` and `count` take a condition, or a [`Query`](../api/python/query.md) that adds a sort, a limit and an offset to one. [`F`](../api/python/conditions.md) names a field, and comparing it makes a condition.

```python
from darudb import F, where

with db.read() as txn:
    users = txn.collection(User)

    users.find(F.age >= 18)  # every adult
    users.find_one(F.email == "ada@example.com")  # the first, or None
    users.count(F.age >= 18)  # an int
    users.find(where(F.age >= 18).sort_by(F.age).limit(10))
```

`F` is not typed, so a field the class does not have, or a value of another type, fails with `INVALID_QUERY` when the query runs.

:::

## Conditions

::: lang rust

Each associated function of `Filter` tests one field, named by a path, against values of any type that converts into a `Value`: integers, `f64`, `bool`, `&str` and `String`, and `&[u8]` or `Vec<u8>` for bytes.

| Builder | Text | Finds objects whose field |
| --- | --- | --- |
| `Filter::eq("city", "Seoul")` | `city == "Seoul"` | equals the value |
| `Filter::ne("city", "Seoul")` | `city != "Seoul"` | is not null and differs from the value |
| `Filter::lt("age", 18)` | `age < 18` | is less than the value |
| `Filter::le("age", 18)` | `age <= 18` | is at most the value |
| `Filter::gt("age", 18)` | `age > 18` | is greater than the value |
| `Filter::ge("age", 18)` | `age >= 18` | is at least the value |
| `Filter::between("age", 20, 29)` | `age BETWEEN 20 AND 29` | is between two values, both included |
| `Filter::is_in("city", ["Busan", "Lisbon"])` | `city IN ["Busan", "Lisbon"]` | equals one of the values |
| `Filter::contains("tags", "admin")` | `tags CONTAINS "admin"` | holds the text, or the element in a list |
| `Filter::starts_with("name", "J")` | `name STARTSWITH "J"` | starts with the text |
| `Filter::ends_with("email", "@example.com")` | `email ENDSWITH "@example.com"` | ends with the text |
| `Filter::is_null("email")` | `email IS NULL` | is null |
| `Filter::is_not_null("email")` | `email IS NOT NULL` | holds a value |

`Filter::eq(field, Value::Null)` is `is_null`, and `Filter::ne(field, Value::Null)` is `is_not_null`.

:::

::: lang node

`where` takes a field, an operator and a value. For a condition `where` cannot write, give it a function of the [`conditions`](../api/node/conditions.md), which has a method for each test.

| `where` | `conditions` | Text | Finds objects whose field |
| --- | --- | --- | --- |
| `where('city', '==', 'Seoul')` | `c.eq('city', 'Seoul')` | `city == "Seoul"` | equals the value |
| `where('city', '!=', 'Seoul')` | `c.ne('city', 'Seoul')` | `city != "Seoul"` | is not null and differs from the value |
| `where('age', '<', 18)` | `c.lt('age', 18)` | `age < 18` | is less than the value |
| `where('age', '<=', 18)` | `c.le('age', 18)` | `age <= 18` | is at most the value |
| `where('age', '>', 18)` | `c.gt('age', 18)` | `age > 18` | is greater than the value |
| `where('age', '>=', 18)` | `c.ge('age', 18)` | `age >= 18` | is at least the value |
| `where('age', 'between', [20, 29])` | `c.between('age', 20, 29)` | `age BETWEEN 20 AND 29` | is between two values, both included |
| `where('city', 'in', ['Busan', 'Lisbon'])` | `c.in('city', ['Busan', 'Lisbon'])` | `city IN ["Busan", "Lisbon"]` | equals one of the values |
| `where('tags', 'contains', 'admin')` | `c.contains('tags', 'admin')` | `tags CONTAINS "admin"` | holds the text, or the element in a list |
| `where('name', 'startsWith', 'J')` | `c.startsWith('name', 'J')` | `name STARTSWITH "J"` | starts with the text |
| `where('email', 'endsWith', '@example.com')` | `c.endsWith('email', '@example.com')` | `email ENDSWITH "@example.com"` | ends with the text |
| `where('email', '==', null)` | `c.isNull('email')` | `email IS NULL` | is null |
| `where('email', '!=', null)` | `c.isNotNull('email')` | `email IS NOT NULL` | holds a value |

A `bytes` field compares with a `Uint8Array`, and a link with the linked collection's key.

:::

::: lang dart

A field object has the methods that fit its type: every field tests for null, a value field for equality, an ordered one for order, a string for text, and a list for its elements.

| Builder | Text | Finds objects whose field |
| --- | --- | --- |
| `q.city.equals('Seoul')` | `city == "Seoul"` | equals the value |
| `q.city.notEquals('Seoul')` | `city != "Seoul"` | is not null and differs from the value |
| `q.age.lessThan(18)` | `age < 18` | is less than the value |
| `q.age.atMost(18)` | `age <= 18` | is at most the value |
| `q.age.greaterThan(18)` | `age > 18` | is greater than the value |
| `q.age.atLeast(18)` | `age >= 18` | is at least the value |
| `q.age.between(20, 29)` | `age BETWEEN 20 AND 29` | is between two values, both included |
| `q.city.isIn(['Busan', 'Lisbon'])` | `city IN ["Busan", "Lisbon"]` | equals one of the values |
| `q.name.contains('in')` | `name CONTAINS "in"` | holds the text |
| `q.name.startsWith('J')` | `name STARTSWITH "J"` | starts with the text |
| `q.email.endsWith('@example.com')` | `email ENDSWITH "@example.com"` | ends with the text |
| `q.email.isNull()` | `email IS NULL` | is null |
| `q.email.isNotNull()` | `email IS NOT NULL` | holds a value |

A list field tests its elements, and holds when any element does:

| Builder | Text | Finds objects whose list |
| --- | --- | --- |
| `q.tags.contains('admin')` | `tags CONTAINS "admin"` | has the element |
| `q.tags.containsAny(['admin', 'editor'])` | `tags IN ["admin", "editor"]` | has an element equal to one of the values |
| `q.tags.anyStartsWith('team-')` | `tags STARTSWITH "team-"` | has an element that starts with the text |
| `q.tags.anyEndsWith('-lead')` | `tags ENDSWITH "-lead"` | has an element that ends with the text |

:::

::: lang python

A comparison of `F.field` with a value is a condition, and so are its methods.

| Builder | Text | Finds objects whose field |
| --- | --- | --- |
| `F.city == "Seoul"` | `city == "Seoul"` | equals the value |
| `F.city != "Seoul"` | `city != "Seoul"` | is not null and differs from the value |
| `F.age < 18` | `age < 18` | is less than the value |
| `F.age <= 18` | `age <= 18` | is at most the value |
| `F.age > 18` | `age > 18` | is greater than the value |
| `F.age >= 18` | `age >= 18` | is at least the value |
| `F.age.between(20, 29)` | `age BETWEEN 20 AND 29` | is between two values, both included |
| `F.city.is_in(["Busan", "Lisbon"])` | `city IN ["Busan", "Lisbon"]` | equals one of the values |
| `F.tags.contains("admin")` | `tags CONTAINS "admin"` | holds the text, or the element in a list |
| `F.name.startswith("J")` | `name STARTSWITH "J"` | starts with the text |
| `F.email.endswith("@example.com")` | `email ENDSWITH "@example.com"` | ends with the text |
| `F.email.is_null()`, `F.email == None` | `email IS NULL` | is null |
| `F.email.is_not_null()`, `F.email != None` | `email IS NOT NULL` | holds a value |

`F["between"]` names a field called `between`, whose name is also a method's.

:::

## Combine conditions

::: lang rust

| Builder    | Text      | Holds when             |
| ---------- | --------- | ---------------------- |
| `a.and(b)` | `a AND b` | both hold              |
| `a.or(b)`  | `a OR b`  | either holds           |
| `!a`       | `NOT a`   | the condition does not |

```rust
use darudb::Filter;

fn main() {
    let in_either_city = Filter::eq("city", "Seoul").or(Filter::eq("city", "Busan"));
    let adults_there = in_either_city.and(Filter::ge("age", 18));
    let elsewhere = !Filter::eq("city", "Seoul");
    let _ = (adults_there, elsewhere);
}
```

Calls group as they are written, so no precedence rule applies: `a.or(b).and(c)` is `(a OR b) AND c`.

:::

::: lang node

| Builder                              | Text      | Holds when             |
| ------------------------------------ | --------- | ---------------------- |
| `q.where(a).where(b)`, `c.and(a, b)` | `a AND b` | both hold              |
| `c.or(a, b)`                         | `a OR b`  | either holds           |
| `c.not(a)`                           | `NOT a`   | the condition does not |

```ts
users.find((q) =>
  q.where((c) => c.or(c.eq('city', 'Seoul'), c.eq('city', 'Busan'))).where('age', '>=', 18)
);
users.find((q) => q.where((c) => c.not(c.eq('city', 'Seoul'))));
```

Each `where` adds a condition joined to the others with AND. `and` and `or` take any number of conditions.

:::

::: lang dart

| Builder                   | Text      | Holds when             |
| ------------------------- | --------- | ---------------------- |
| `a & b`, or `where` again | `a AND b` | both hold              |
| `a \| b`                  | `a OR b`  | either holds           |
| `~a`                      | `NOT a`   | the condition does not |

```dart
users.find(
  (q) => q.where(
    (q.city.equals('Seoul') | q.city.equals('Busan')) & q.age.atLeast(18),
  ),
);
users.find((q) => q.where(~q.city.equals('Seoul')));
```

Dart's own precedence applies: `~` binds tightest, then `&`, then `|`.

:::

::: lang python

| Builder                   | Text      | Holds when             |
| ------------------------- | --------- | ---------------------- |
| `a & b`, or `where` again | `a AND b` | both hold              |
| `a \| b`                  | `a OR b`  | either holds           |
| `~a`                      | `NOT a`   | the condition does not |

```python
users.find(((F.city == "Seoul") | (F.city == "Busan")) & (F.age >= 18))
users.find(~(F.city == "Seoul"))
```

`&` and `|` bind more tightly than `==` or `>=` in Python, so a comparison joined to another goes in parentheses. A condition has no truth value, so `and`, `or` and `not` raise `TypeError`.

:::

As in the text, the opposite of a condition also finds the objects whose field is null: `NOT city == "Seoul"` finds them, and `city != "Seoul"` does not.

## Sort, limit and offset

::: lang rust

| Builder                | Text               | Does                              |
| ---------------------- | ------------------ | --------------------------------- |
| `.sort_by("name")`     | `SORT BY name`     | Sorts by a field, ascending       |
| `.sort_by_desc("age")` | `SORT BY age DESC` | Sorts by a field, descending      |
| `.limit(10)`           | `LIMIT 10`         | Returns at most this many objects |
| `.offset(20)`          | `OFFSET 20`        | Skips this many objects first     |
| `.first()`             | `LIMIT 1`          | Returns at most the first object  |

```rust
use darudb::{Filter, Query};

fn main() {
    let page = Query::new()
        .filter(Filter::eq("city", "Seoul"))
        .sort_by("name")
        .sort_by_desc("age")
        .limit(20)
        .offset(40);
    let _ = page;
}
```

Each sort comes after the ones before it. A later `limit` or `offset` replaces an earlier one.

:::

::: lang node

| Builder                  | Text               | Does                              |
| ------------------------ | ------------------ | --------------------------------- |
| `.sortBy('name')`        | `SORT BY name`     | Sorts by a field, ascending       |
| `.sortBy('age', 'desc')` | `SORT BY age DESC` | Sorts by a field, descending      |
| `.limit(10)`             | `LIMIT 10`         | Returns at most this many objects |
| `.offset(20)`            | `OFFSET 20`        | Skips this many objects first     |

```ts
users.find((q) =>
  q.where('city', '==', 'Seoul').sortBy('name').sortBy('age', 'desc').limit(20).offset(40)
);
```

Each sort comes after the ones before it. `findOne` returns the first object the query finds.

:::

::: lang dart

| Builder                            | Text               | Does                              |
| ---------------------------------- | ------------------ | --------------------------------- |
| `.sortBy(q.name)`                  | `SORT BY name`     | Sorts by a field, ascending       |
| `.sortBy(q.age, descending: true)` | `SORT BY age DESC` | Sorts by a field, descending      |
| `.limit(10)`                       | `LIMIT 10`         | Returns at most this many objects |
| `.offset(20)`                      | `OFFSET 20`        | Skips this many objects first     |

```dart
users.find(
  (q) => q
      .where(q.city.equals('Seoul'))
      .sortBy(q.name)
      .sortBy(q.age, descending: true)
      .limit(20)
      .offset(40),
);
```

Each sort comes after the ones before it. `findOne` returns the first object the query finds.

:::

::: lang python

| Builder                            | Text               | Does                              |
| ---------------------------------- | ------------------ | --------------------------------- |
| `.sort_by(F.name)`                 | `SORT BY name`     | Sorts by a field, ascending       |
| `.sort_by(F.age, descending=True)` | `SORT BY age DESC` | Sorts by a field, descending      |
| `.limit(10)`                       | `LIMIT 10`         | Returns at most this many objects |
| `.offset(20)`                      | `OFFSET 20`        | Skips this many objects first     |

```python
from darudb import F, Query, where

users.find(
    where(F.city == "Seoul").sort_by(F.name).sort_by(F.age, descending=True).limit(20).offset(40)
)
users.find(Query().sort_by(F.age, descending=True).limit(10))  # no filter
```

`where` starts a query from a condition, and `Query()` starts one without. Every method returns a new query, so a query kept in a variable can be the start of several.

:::

## Paths

::: lang rust

| Builder | Text | Tests |
| --- | --- | --- |
| `Filter::eq("address.city", "Lisbon")` | `address.city == "Lisbon"` | A field of an embedded object |
| `Filter::eq("team.name", "Core")` | `team.name == "Core"` | A field of the linked object |
| `Filter::eq("team", 3)` | `team == 3` | The key a link holds |

A path is the field names joined by `.`, as in the text, at most 32 of them.

:::

::: lang node

| Builder | Text | Tests |
| --- | --- | --- |
| `where('address.city', '==', 'Lisbon')` | `address.city == "Lisbon"` | A field of an embedded object |
| `where('team.name', '==', 'Core')` | `team.name == "Core"` | A field of the linked object |
| `where('team', '==', 3)` | `team == 3` | The key a link holds |

TypeScript checks a field of the collection itself, and leaves a dotted path to the engine, which checks it when the query runs.

:::

::: lang dart

| Builder                           | Text                       | Tests                         |
| --------------------------------- | -------------------------- | ----------------------------- |
| `q.address.city.equals('Lisbon')` | `address.city == "Lisbon"` | A field of an embedded object |
| `q.team.name.equals('Core')`      | `team.name == "Core"`      | A field of the linked object  |
| `q.team.equals(3)`                | `team == 3`                | The key a link holds          |

A link's field object has the linked class's fields, and an embedded object's has its class's, so the types check a path too.

:::

::: lang python

| Builder                      | Text                       | Tests                         |
| ---------------------------- | -------------------------- | ----------------------------- |
| `F.address.city == "Lisbon"` | `address.city == "Lisbon"` | A field of an embedded object |
| `F.team.name == "Core"`      | `team.name == "Core"`      | A field of the linked object  |
| `F.team == 3`                | `team == 3`                | The key a link holds          |

A path names fields by their Python attributes, and the package gives the engine the names the file stores, which `field(name=...)` may have changed.

:::

## Prepare a query that runs often

::: lang rust

The builder takes its values as it is built, so a query with values that change is written as text and prepared: `Query::prepare` parses it once, and `bind` gives it values on each run.

```rust
use darudb::Query;

fn main() -> Result<(), darudb::Error> {
    let by_email = Query::prepare("email == $0")?;
    let query = by_email.bind(&["ada@example.com".into()])?;
    let _ = query;
    Ok(())
}
```

:::

::: lang node

[`param`](../api/node/param.md) stands in for a value that changes, and `db.prepare` encodes the query once. The prepared query then takes the values on each run.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const inAges = db.prepare('users', (q) => q.where('age', 'between', [param(0), param(1)]));

db.read((txn) => {
  const users = txn.collection('users');

  users.findOne(byEmail, ['ada@example.com']);
  users.find(inAges, [18, 30]);
});
```

:::

::: lang dart

The builder takes its values as it is built, so a query with values that change is written as text and prepared: `db.prepare` parses it once, and `findPrepared`, `findOnePrepared` and `countPrepared` give it values on each run.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');

db.read((txn) {
  txn.collection(userSchema).findOnePrepared(byEmail, ['ada@example.com']);
});
```

:::

::: lang python

[`param`](../api/python/param.md) stands in for a value that changes, and `db.prepare` compiles the query once. The prepared query then takes the values after it on each run.

```python
from darudb import F, param

by_email = db.prepare(User, F.email == param(0))
in_ages = db.prepare(User, F.age.between(param(0), param(1)))

with db.read() as txn:
    users = txn.collection(User)

    users.find_one(by_email, "ada@example.com")
    users.find(in_ages, 18, 30)
```

:::

## Examples

The queries of the [query language's examples](./query-language.md#examples), built in code, on the same collection of users: `name` and `city`, strings; `email`, an optional string with a unique index; `age`, an integer with an index; `tags`, a list of strings; `address`, an embedded object; and `team`, a link to a team.

::: lang rust

| Text | Builder |
| --- | --- |
| `email == "ada@example.com"` | `Query::new().filter(Filter::eq("email", "ada@example.com"))` |
| `age >= 18 AND age < 30` | `Query::new().filter(Filter::ge("age", 18).and(Filter::lt("age", 30)))` |
| `city IN ["Seoul", "Busan"]` | `Query::new().filter(Filter::is_in("city", ["Seoul", "Busan"]))` |
| `name STARTSWITH "Ma" SORT BY name` | `Query::new().filter(Filter::starts_with("name", "Ma")).sort_by("name")` |
| `tags CONTAINS "admin"` | `Query::new().filter(Filter::contains("tags", "admin"))` |
| `email IS NULL` | `Query::new().filter(Filter::is_null("email"))` |
| `NOT city == "Seoul"` | `Query::new().filter(!Filter::eq("city", "Seoul"))` |
| `address.city == "Lisbon"` | `Query::new().filter(Filter::eq("address.city", "Lisbon"))` |
| `team.name == "Core"` | `Query::new().filter(Filter::eq("team.name", "Core"))` |
| `SORT BY age DESC, name LIMIT 10` | `Query::new().sort_by_desc("age").sort_by("name").limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `Query::new().filter(Filter::eq("city", "Seoul")).sort_by("name").limit(20).offset(40)` |

:::

::: lang node

| Text | Builder |
| --- | --- |
| `email == "ada@example.com"` | `(q) => q.where('email', '==', 'ada@example.com')` |
| `age >= 18 AND age < 30` | `(q) => q.where('age', '>=', 18).where('age', '<', 30)` |
| `city IN ["Seoul", "Busan"]` | `(q) => q.where('city', 'in', ['Seoul', 'Busan'])` |
| `name STARTSWITH "Ma" SORT BY name` | `(q) => q.where('name', 'startsWith', 'Ma').sortBy('name')` |
| `tags CONTAINS "admin"` | `(q) => q.where('tags', 'contains', 'admin')` |
| `email IS NULL` | `(q) => q.where('email', '==', null)` |
| `NOT city == "Seoul"` | `(q) => q.where((c) => c.not(c.eq('city', 'Seoul')))` |
| `address.city == "Lisbon"` | `(q) => q.where('address.city', '==', 'Lisbon')` |
| `team.name == "Core"` | `(q) => q.where('team.name', '==', 'Core')` |
| `SORT BY age DESC, name LIMIT 10` | `(q) => q.sortBy('age', 'desc').sortBy('name').limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `(q) => q.where('city', '==', 'Seoul').sortBy('name').limit(20).offset(40)` |

:::

::: lang dart

| Text | Builder |
| --- | --- |
| `email == "ada@example.com"` | `(q) => q.where(q.email.equals('ada@example.com'))` |
| `age >= 18 AND age < 30` | `(q) => q.where(q.age.atLeast(18) & q.age.lessThan(30))` |
| `city IN ["Seoul", "Busan"]` | `(q) => q.where(q.city.isIn(['Seoul', 'Busan']))` |
| `name STARTSWITH "Ma" SORT BY name` | `(q) => q.where(q.name.startsWith('Ma')).sortBy(q.name)` |
| `tags CONTAINS "admin"` | `(q) => q.where(q.tags.contains('admin'))` |
| `email IS NULL` | `(q) => q.where(q.email.isNull())` |
| `NOT city == "Seoul"` | `(q) => q.where(~q.city.equals('Seoul'))` |
| `address.city == "Lisbon"` | `(q) => q.where(q.address.city.equals('Lisbon'))` |
| `team.name == "Core"` | `(q) => q.where(q.team.name.equals('Core'))` |
| `SORT BY age DESC, name LIMIT 10` | `(q) => q.sortBy(q.age, descending: true).sortBy(q.name).limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `(q) => q.where(q.city.equals('Seoul')).sortBy(q.name).limit(20).offset(40)` |

:::

::: lang python

| Text | Builder |
| --- | --- |
| `email == "ada@example.com"` | `F.email == "ada@example.com"` |
| `age >= 18 AND age < 30` | `(F.age >= 18) & (F.age < 30)` |
| `city IN ["Seoul", "Busan"]` | `F.city.is_in(["Seoul", "Busan"])` |
| `name STARTSWITH "Ma" SORT BY name` | `where(F.name.startswith("Ma")).sort_by(F.name)` |
| `tags CONTAINS "admin"` | `F.tags.contains("admin")` |
| `email IS NULL` | `F.email.is_null()` |
| `NOT city == "Seoul"` | `~(F.city == "Seoul")` |
| `address.city == "Lisbon"` | `F.address.city == "Lisbon"` |
| `team.name == "Core"` | `F.team.name == "Core"` |
| `SORT BY age DESC, name LIMIT 10` | `Query().sort_by(F.age, descending=True).sort_by(F.name).limit(10)` |
| `city == "Seoul" SORT BY name LIMIT 20 OFFSET 40` | `where(F.city == "Seoul").sort_by(F.name).limit(20).offset(40)` |

:::
