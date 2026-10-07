---
title: 컬렉션과 객체
order: 3
---

# 컬렉션과 객체

스키마를 주고 연 데이터베이스에는 타입이 있는 객체를 담는 컬렉션이 생기고, 인덱스는 엔진이 객체와 함께 고쳐 둡니다.

## 스키마 선언하기

스키마는 1부터 시작하는 버전과 컬렉션으로 이뤄집니다. 컬렉션마다 타입이 정해진 필드와 기본 키, 인덱스가 있습니다.

::: lang rust

```rust
use darudb::{Collection, OpenOptions, Schema, Type};

fn schema() -> Schema {
    Schema::new(1)
        .collection(
            Collection::new("users")
                .field("name", Type::String)
                .optional("email", Type::String)
                .with_default("age", Type::Int, 0)
                .unique("email"),
        )
        .collection(
            Collection::new("posts")
                .primary_key("slug", Type::String)
                .field("author", Type::link("users"))
                .optional("tags", Type::list(Type::String))
                .index("author")
                .index("tags"),
        )
}

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new().schema(schema()).open("app.darudb")?;
    db.close()
}
```

타입은 `Bool`, 64비트 `Int`, 64비트 `Float`, `String`, `Bytes`, 다른 컬렉션의 객체를 가리키는 링크(`Type::link`), 이들의 목록(`Type::list`), 그리고 자기 필드를 가진 내장 객체(`Type::object(Embedded::new().field(...))`)가 있습니다.

- **필수 필드와 선택 필드.** `field`는 필수여서 빠진 객체는 쓸 수 없습니다. `optional`은 null일 수 있고, 빠지면 null이 됩니다. `with_default`는 필수지만 빠지면 기본값이 들어갑니다.
- **기본 키.** `primary_key`로 `Int`, `String`, `Bytes` 필드 하나를 지정합니다.
- **인덱스.** `index`는 필드에 인덱스를 두고, `unique`는 값이 같은 객체 둘을 받지 않는 인덱스를 둡니다.

:::

::: lang node

필드 타입은 `t`에 있고, `collection`으로 필드를 묶고, `schema`로 컬렉션에 버전을 붙입니다. 모든 객체의 TypeScript 타입이 이 선언에서 나옵니다.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, {
  teams: collection({
    name: t.string().primaryKey(),
    city: t.string().optional()
  }),
  users: collection({
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index(),
    tags: t.list(t.string()).optional().index(),
    team: t.link('teams').optional(),
    address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
  })
});

const db = Database.open('app.darudb', { schema: app });
```

타입은 `t.bool()`, `t.int()`, `t.bigint()`, `t.float()`, `t.string()`, `t.bytes()`, `t.link(collection)`, `t.list(type)`, `t.object(fields)`가 있습니다.

- **필수 필드와 선택 필드.** 따로 정하지 않은 필드는 필수여서 빠진 객체는 쓸 수 없습니다. `optional()`은 null일 수 있게 하고, 빠지면 null이 됩니다. `default(value)`는 필수로 두되 빠지면 그 값을 채웁니다.
- **기본 키.** `primaryKey()`는 `int`, `bigint`, `string`, `bytes` 필드를 키로 삼습니다.
- **인덱스.** `index()`는 필드에 인덱스를 두고, `unique()`는 값이 같은 객체 둘을 받지 않는 인덱스를 둡니다.
- **숫자.** `t.int()` 필드는 number입니다. number가 정확히 담지 못하는 2^53 너머의 값은 쓸 때 거부하고 읽을 때 실패합니다. 그런 값이 필요하면 `t.bigint()`로 선언하세요. 언제나 `bigint`로 읽힙니다. 바이트는 `Uint8Array`입니다.

:::

::: lang dart

컬렉션마다 `@Collection()`을 붙인 클래스를 하나 둡니다. 필드는 `final`이고, 생성자가 필드를 모두 받습니다. `dart run build_runner build`는 라이브러리의 `.g.dart` 파트에 클래스 이름을 딴 스키마 상수를 씁니다. `User`라면 `userSchema`이고, 객체를 읽고 쓰는 코드와 쿼리 빌더, `copyWith`도 함께 들어 있습니다.

```dart
import 'package:darudb/darudb.dart';

part 'models.g.dart';

@Collection('teams')
class Team {
  const Team({required this.name, this.city});

  @PrimaryKey()
  final String name;
  final String? city;
}

@Embedded()
class Address {
  const Address({required this.city, this.zip});

  final String city;
  final int? zip;
}

@Collection('users')
class User {
  const User({
    this.id,
    required this.name,
    this.email,
    this.age = 0,
    this.tags,
    this.team,
    this.address,
  });

  final int? id;
  final String name;
  @Unique()
  final String? email;
  @Index()
  final int age;
  @Index()
  final List<String>? tags;
  final Link<Team>? team;
  final Address? address;
}

final db = Database.open(
  'app.darudb',
  schema: const Schema(1, [teamSchema, userSchema]),
);
```

필드의 타입은 Dart 타입 그대로입니다. `bool`, 64비트 `int`, `double`, `String`, 바이트를 담는 `Uint8List`, 이들이나 링크의 `List`, 다른 컬렉션의 객체를 가리키는 `Link<T>`, 그리고 `@Embedded()`를 붙인 클래스로 만드는 내장 객체가 있습니다.

- **필수 필드와 선택 필드.** null이 될 수 없는 타입의 필드는 필수여서 빠진 객체는 쓸 수 없습니다. null이 될 수 있는 필드는 선택이고, 빠지면 null이 됩니다. 생성자 매개변수의 기본값은 필드의 기본값이 됩니다. 그 필드는 필수로 남고, 레코드에 없으면 기본값이 들어갑니다.
- **기본 키.** `@PrimaryKey()`는 `int`, `String`, `Uint8List` 필드를 키로 삼습니다. 기본 키가 없는 클래스에는 `final int? id` 필드를 두고, 이 필드는 객체를 넣기 전까지 `null`입니다.
- **인덱스.** `@Index()`는 필드에 인덱스를 두고, `@Unique()`는 값이 같은 객체 둘을 받지 않는 인덱스를 둡니다. `@Name('...')`을 붙이면 파일 안의 필드 이름을 Dart의 이름과 다르게 정할 수 있습니다.
- **객체는 값입니다.** 필드는 모두 final이므로, 바꾼 객체는 생성된 `copyWith`로 만든 사본이고 `put`으로 다시 씁니다.

:::

::: lang python

컬렉션마다 `@darudb.collection`을 붙인 클래스를 하나 둡니다. 이 데코레이터는 클래스를 필드를 바꿀 수 없고 키워드 인자로만 만드는 데이터 클래스로 만듭니다. 어노테이션은 필드의 타입이 되고, 어노테이션으로 나타낼 수 없는 기본값과 인덱스, 기본 키, 링크, 파일에 저장할 이름은 `field`로 더합니다.

```python
import darudb
from darudb import field


@darudb.collection("teams")
class Team:
    name: str = field(primary_key=True)
    city: str | None = None


@darudb.embedded
class Address:
    city: str
    zip: int | None = None


@darudb.collection("users")
class User:
    id: int | None = None
    name: str
    email: str | None = field(default=None, unique=True)
    age: int = field(default=0, index=True)
    tags: list[str] | None = field(default=None, index=True)
    team: str | None = field(default=None, link=Team)
    address: Address | None = None


db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [Team, User]))
```

필드의 타입은 어노테이션 그대로입니다. `bool`, 64비트 `int`, `float`, `str`, `bytes`, 이들이나 링크의 `list`, 그리고 `@darudb.embedded`를 붙인 클래스로 만드는 내장 객체가 있습니다. 클래스는 `Schema`를 만들 때 읽으므로, 어노테이션에 뒤에서 선언한 클래스를 써도 됩니다.

- **필수 필드와 선택 필드.** 기본값이 없고 타입이 `X | None`이 아닌 필드는 필수여서 빠진 객체는 쓸 수 없습니다. `X | None` 필드는 선택이고, 빠지면 `None`이 되며, `None` 말고는 기본값을 받지 않습니다. `= value`나 `field(default=...)`, `field(default_factory=...)`로 기본값을 준 필드는 필수로 남고, 빠지면 기본값이 들어갑니다. 내가 만든 객체에서도, 필드가 생기기 전에 파일에 들어간 객체에서도 그렇습니다.
- **기본 키.** `field(primary_key=True)`는 `int`, `str`, `bytes` 필드를 키로 삼습니다. 키는 필수이고 기본값이 없습니다. 기본 키가 없는 클래스에는 `id: int | None = None` 필드를 두고, 이 필드는 객체를 넣기 전까지 `None`입니다.
- **링크.** `field(link=Team)`을 붙인 `int`, `str`, `bytes` 필드나 그 목록에는 다른 컬렉션의 기본 키가 들어갑니다. 대상 컬렉션은 클래스로도, 이름으로도 가리킬 수 있습니다.
- **인덱스.** `field(index=True)`는 필드에 인덱스를 두고, `field(unique=True)`는 값이 같은 객체 둘을 받지 않는 인덱스를 둡니다. `field(name="...")`를 주면 파일 안의 필드 이름을 Python의 이름과 다르게 정할 수 있습니다. 내장 객체의 필드에는 인덱스를 둘 수 없습니다.
- **객체는 값입니다.** 클래스의 필드는 바꿀 수 없으므로, 바꾼 객체는 `dataclasses.replace`로 만든 사본이고 `put`으로 다시 씁니다. 파일에서 읽은 객체는 클래스의 `__init__`을 부르지 않고 만들기 때문에 `__post_init__`도 돌지 않습니다.
- **값.** `float` 필드는 `int`도 받지만, `int` 필드는 `float`와 `bool`, 64비트를 넘는 `int`를 받지 않습니다. 바이트는 `bytes`나 `bytearray`, `memoryview`로 넣고 `bytes`로 읽습니다. 목록에는 `None`을 담을 수 없고, 목록의 목록이나 내장 객체의 목록은 선언할 수 없습니다.

:::

엔진이 지키는 규칙은 언어와 관계없이 같습니다.

- **자동 키.** 기본 키를 지정하지 않은 컬렉션에는 `id`라는 정수 필드가 생기고, `id` 없이 쓴 객체는 1부터 차례로 다음 번호를 받습니다. 객체를 지워도 한 파일 안에서 같은 번호를 두 번 주지 않습니다.
- **링크**에는 대상 컬렉션에 있는 객체의 기본 키가 들어갑니다. 없는 객체를 가리켜도 되고, 그때는 담긴 키가 그대로 읽힙니다.
- **인덱스**가 있으면 그 필드로 찾는 쿼리는 찾는 객체만 읽습니다. 고유 인덱스에서도 null은 몇 개가 있어도 되고, 목록 필드의 인덱스에는 원소마다 항목이 생깁니다.

처음 열 때 스키마를 파일에 저장합니다. 그 뒤로는 열 때마다 선언한 스키마를 저장된 것과 비교합니다. 버전이 같은데 내용이 다르면 `SCHEMA_MISMATCH`, 파일의 버전이 더 높으면 `SCHEMA_TOO_NEW`로 실패합니다. 컬렉션이나 인덱스를 선언하는 순서만 바꾼 것은 변경이 아닙니다. 스키마를 바꾸려면 버전을 올립니다. [마이그레이션](./migrations.md)을 보세요.

## 객체 읽고 쓰기

::: lang rust

객체는 이름이 붙은 값의 모음입니다. 쓰기 트랜잭션에서 `collection`을 부르면 그 컬렉션의 객체와 객체를 바꾸는 메서드를 쓸 수 있습니다.

```rust
use darudb::{Database, Object, Value};

fn write(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;

    let alice = users.insert(Object::new().with("name", "Alice").with("email", "alice@example.com"))?;
    users.insert(Object::new().with("name", "Bob"))?;

    // `put`은 키가 같은 객체를 바꿉니다.
    users.put(Object::new().with("id", alice.clone()).with("name", "Alice").with("age", 31))?;
    // `update`는 받은 필드만 바꾸고 나머지는 그대로 둡니다.
    users.update(alice.clone(), Object::new().with("age", 32).with("email", Value::Null))?;

    let mut posts = txn.collection("posts")?;
    posts.insert(
        Object::new()
            .with("slug", "hello")
            .with("author", alice)
            .with("tags", vec![Value::from("intro")]),
    )?;

    txn.commit()
}

fn read(db: &Database) -> Result<(), darudb::Error> {
    let read = db.begin_read()?;
    let users = read.collection("users")?;

    if let Some(user) = users.get(1)? {
        println!("{:?}", user.get("name"));
    }

    for user in users.iter()? {
        println!("{:?}", user?);
    }

    println!("{} users", users.len()?);
    Ok(())
}
```

- `insert`는 새 객체의 키를 돌려줍니다. `put`은 없으면 넣고 있으면 바꿉니다. `delete`는 키를 받아 지우고, 지운 객체가 있었는지 돌려줍니다.
- `update`는 키와 바꿀 필드를 받고, 객체가 있었는지 돌려줍니다. 객체가 없으면 아무것도 넣지 않습니다. null을 주면 선택 필드는 null이 되고 기본값이 있는 필드는 기본값이 됩니다.

:::

::: lang node

`write`는 함수를 쓰기 트랜잭션 안에서, `read`는 읽기 트랜잭션 안에서 실행합니다. 그 안에서 `collection`을 부르면 그 컬렉션의 객체와 객체를 바꾸는 메서드를 쓸 수 있습니다.

```ts
db.write((txn) => {
  txn.collection('teams').insert({ name: 'north', city: 'Seoul' });

  const users = txn.collection('users');

  users.insertMany([
    { name: 'Alice', email: 'alice@example.com', age: 31, team: 'north' },
    { name: 'Bob', tags: ['new'] }
  ]);
  users.put({ id: 2, name: 'Robert', age: 18 });
  users.update(1, { age: 32, email: null });
  users.delete(3);
});

const alice = db.read((txn) => txn.collection('users').get(1));
```

- `insert`와 `insertMany`는 키를 돌려줍니다. `put`과 `putMany`는 없으면 넣고 있으면 바꿉니다. `delete`는 지운 객체가 있었는지 돌려줍니다.
- `update`는 받은 필드만 바꾸고 나머지는 그대로 두며, 객체가 있었는지 돌려줍니다. `null`을 주면 선택 필드는 null이 되고 기본값이 있는 필드는 기본값이 됩니다. `undefined`인 필드는 바뀌지 않습니다.
- 여러 객체를 한 번에 넘기면 버퍼 하나에 담아 엔진을 한 번만 부릅니다. 객체마다 부르는 것보다 훨씬 쌉니다.

:::

::: lang dart

`txn.collection(userSchema)`는 컬렉션의 객체를 클래스로 돌려주고, 객체를 바꾸는 메서드도 줍니다.

```dart
db.write((txn) {
  txn.collection(teamSchema).insert(const Team(name: 'north', city: 'Seoul'));

  final users = txn.collection(userSchema);
  final alice = users.insert(
    const User(name: 'Alice', email: 'alice@example.com', age: 31, team: Link<Team>('north')),
  );

  users.insertMany(const [User(name: 'Bob', tags: ['new'])]);
  // `put`은 키가 같은 객체를 바꿉니다.
  users.put(users.get(alice)!.copyWith(age: 32));
  // `update`는 받은 필드만 바꾸고 나머지는 그대로 둡니다.
  users.update(alice, (q) => [q.email.set(null)]);
  users.delete(2);
});

final alice = db.read((txn) => txn.collection(userSchema).get(1));
```

- `insert`는 새 객체의 키를, `insertMany`는 한 묶음의 키를 돌려줍니다. 묶음은 버퍼 하나에 담겨 엔진을 한 번만 부릅니다. `put`과 `putMany`는 없으면 넣고 있으면 바꿉니다. `delete`는 지운 객체가 있었는지 돌려줍니다.
- `update`는 키와, 바꿀 내용을 돌려주는 함수를 받습니다. 바꿀 내용은 필드의 `set`으로 만들고, 객체가 있었는지 돌려줍니다. `set(null)`을 주면 선택 필드는 null이 되고 기본값이 있는 필드는 기본값이 됩니다.
- `copyWith`는 받지 않은 필드를 모두 그대로 둡니다. 필드를 null로 만들 수는 없으므로, 그럴 때는 객체를 새로 만듭니다.

:::

::: lang python

`txn.collection(User)`는 컬렉션의 객체를 클래스의 인스턴스로 돌려주고, 객체를 바꾸는 메서드도 줍니다. `txn.collection("users")`처럼 이름으로 같은 컬렉션을 가져올 수도 있습니다.

```python
with db.write() as txn:
    txn.collection(Team).insert(Team(name="north", city="Seoul"))

    users = txn.collection(User)
    alice = users.insert(User(name="Alice", email="alice@example.com", age=31, team="north"))

    users.insert_many([User(name="Bob", tags=["new"])])
    # `put`은 키가 같은 객체를 바꿉니다.
    users.put(User(id=alice, name="Alice", age=32, team="north"))
    # `update`는 받은 필드만 바꾸고 나머지는 그대로 둡니다.
    users.update(alice, email=None, tags=["admin"])
    users.delete(2)

with db.read() as txn:
    alice = txn.collection(User).get(1)
```

- `insert`는 새 객체의 키를, `insert_many`는 한 묶음의 키를 돌려줍니다. 묶음은 엔진을 한 번만 부릅니다. `put`과 `put_many`는 없으면 넣고 있으면 바꿉니다. `delete`는 지운 객체가 있었는지 돌려줍니다.
- `update(key, **changes)`는 Python 이름으로 지정한 필드만 바꾸고 나머지는 그대로 두며, 객체가 있었는지 돌려줍니다. `None`을 주면 선택 필드는 `None`이 되고 기본값이 있는 필드는 기본값이 됩니다.
- 묶음은 엔진이 거부한 첫 객체에서 그 오류와 함께 멈추고, 그 앞의 객체는 트랜잭션에 넣은 채로 남습니다. 다른 클래스의 객체처럼 패키지가 변환할 수 없는 객체가 섞여 있으면, 아무것도 쓰기 전에 묶음 전체를 거부합니다.

:::

다음은 모든 언어에서 같습니다.

- `insert`는 키가 이미 있거나, 고유 인덱스에 같은 값이 이미 있으면 `DUPLICATE_KEY`로 실패합니다.
- `update`는 내장 객체와 목록을 통째로 바꾸고, 기본 키를 바꾸려 하면 `INVALID_ARGUMENT`로 실패합니다. 엔진이 레코드를 그 자리에서 고치므로 객체를 읽어 다시 넣는 것보다 쌉니다.
- 값의 타입이 틀렸거나, 스키마에 없는 필드가 있거나, 필수 필드가 빠져 스키마에 맞지 않는 객체는 `INVALID_ARGUMENT`로 실패합니다.
- 거부된 쓰기는 아무것도 바꾸지 않으므로, 트랜잭션은 계속 쓰다가 커밋해도 됩니다.
- 읽어 온 객체는 트랜잭션이 끝나도 남는 평범한 값입니다. 스키마의 필드가 모두 들어 있고, 빠진 필드에는 기본값이나 null이 들어갑니다.

::: lang rust

## 객체를 Rust 타입으로

크레이트의 `derive` 기능을 켜면 `#[derive(Object)]`로 구조체를 컬렉션의 객체로 만들 수 있습니다. 스키마는 구조체에서 컬렉션을 선언하고, `collection_of`는 레코드를 구조체로 바로 읽고 구조체를 레코드로 씁니다. 이름 붙은 값을 담는 `Object`를 거치지 않으므로, 레코드를 찾는 데 드는 만큼의 비용이 빠집니다.

```rust
use darudb::{Collection, Database, Filter, Object, OpenOptions, Query, Schema};

#[derive(Object, Debug, Clone)]
#[darudb(collection = "users")]
struct User {
    id: Option<i64>,
    name: String,
    #[darudb(unique)]
    email: Option<String>,
    #[darudb(index, default = 0)]
    age: i64,
}

fn open() -> darudb::Result<Database> {
    OpenOptions::new()
        .schema(Schema::new(1).collection(Collection::of::<User>()))
        .open("app.darudb")
}

fn write_and_read(db: &Database) -> darudb::Result<()> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection_of::<User>()?;
    let id = users.insert(&User { id: None, name: "Alice".to_owned(), email: None, age: 31 })?;

    drop(users);
    txn.commit()?;

    let read = db.begin_read()?;
    let users = read.collection_of::<User>()?;
    let alice: Option<User> = users.get(id)?;
    let adults: Vec<User> = users.query(&Query::new().filter(Filter::ge("age", 18)))?;

    println!("{alice:?} {adults:?}");
    Ok(())
}
```

`Option` 필드는 선택 필드이고, `Vec<T>`는 목록, `Link<T>`는 링크, `#[derive(Embedded)]`를 붙인 구조체는 내장 객체입니다. `#[darudb(key)]`를 붙인 필드가 없으면 구조체에 `id: Option<i64>`가 있어야 하고, 이 필드는 객체를 넣기 전까지 `None`입니다. 쓸 수 있는 속성은 [파생 매크로](../api/rust/derive.md)에 있습니다. 타입으로 읽고 쓰는 쪽과 `Object`로 읽고 쓰는 쪽은 같은 객체를 다루므로 두 API를 섞어 써도 됩니다.

:::

## 여러 핸들과 프로세스

핸들은 열 때 받은 스키마를 계속 씁니다. 다른 프로세스나 같은 프로세스의 다른 핸들이 파일을 마이그레이션하면, 예전 핸들로 컬렉션에 접근하는 다음 트랜잭션은 `SCHEMA_MISMATCH`로 실패합니다. 그러면 새 스키마로 다시 열어야 합니다. 마이그레이션 전에 시작한 읽기 트랜잭션은 시작할 때의 커밋을 보므로 예전 스키마로 계속 읽습니다.
