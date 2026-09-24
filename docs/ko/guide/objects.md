---
title: 컬렉션과 객체
order: 3
---

# 컬렉션과 객체

스키마를 주고 연 데이터베이스에는 타입이 있는 객체를 담는 컬렉션이 생깁니다. 인덱스는 엔진이 객체와 함께 고쳐 두고, 스키마 버전이 오르면 파일을 마이그레이션합니다. 이 페이지는 Rust API를 다룹니다. Node.js 패키지에는 아직 없습니다.

## 스키마 선언하기

스키마는 1부터 시작하는 버전과 컬렉션으로 이뤄집니다. 컬렉션마다 타입이 정해진 필드와 기본 키, 인덱스가 있습니다.

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

타입은 `Bool`, 64비트 `Int`, 64비트 `Float`, `String`, `Bytes`, 다른 컬렉션의 객체를 가리키는 링크, 이들의 목록, 그리고 자기 필드를 가진 내장 객체(`Type::object(Embedded::new().field(...))`)가 있습니다.

- **필수 필드와 선택 필드.** `field`는 필수여서 빠진 객체는 쓸 수 없습니다. `optional`은 null일 수 있고, 빠지면 null이 됩니다. `with_default`는 필수지만 빠지면 기본값이 들어갑니다.
- **기본 키.** `primary_key`로 `Int`, `String`, `Bytes` 필드 하나를 지정합니다. 지정하지 않으면 `id`라는 `Int` 필드가 생기고, `id` 없이 쓴 객체는 1부터 차례로 다음 번호를 받습니다. 객체를 지워도 한 파일 안에서 같은 번호를 두 번 주지 않습니다.
- **링크**에는 대상 컬렉션에 있는 객체의 기본 키가 들어갑니다. 없는 객체를 가리켜도 됩니다.
- **인덱스.** `index`는 필드에 인덱스를 두어 그 필드로 찾을 때 모든 객체를 읽지 않게 합니다. `unique`는 여기에 더해 값이 같은 객체 둘을 받지 않습니다. null은 몇 개가 있어도 됩니다. 목록 필드의 인덱스에는 원소마다 항목이 생깁니다.

처음 열 때 스키마를 파일에 저장합니다. 그 뒤로는 열 때마다 선언한 스키마를 저장된 것과 비교합니다. 버전이 같은데 내용이 다르면 `SCHEMA_MISMATCH`, 파일의 버전이 더 높으면 `SCHEMA_TOO_NEW`로 실패합니다. 컬렉션이나 인덱스를 선언하는 순서만 바꾼 것은 변경이 아닙니다.

## 객체 읽고 쓰기

객체는 이름이 붙은 값의 모음입니다. 쓰기 트랜잭션에서 `collection`을 부르면 그 컬렉션의 객체와 객체를 바꾸는 메서드를 쓸 수 있습니다.

```rust
use darudb::{Database, Object, Value};

fn write(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    let mut users = txn.collection("users")?;

    let alice = users.insert(Object::new().with("name", "Alice").with("email", "alice@example.com"))?;
    users.insert(Object::new().with("name", "Bob"))?;

    // `put` replaces the object with the same key.
    users.put(Object::new().with("id", alice.clone()).with("name", "Alice").with("age", 31))?;

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

- `insert`는 키가 이미 있거나, 고유 인덱스에 같은 값이 이미 있으면 `DUPLICATE_KEY`로 실패합니다. `put`은 없으면 넣고 있으면 바꿉니다. `delete`는 키를 받아 지우고, 지운 객체가 있었는지 돌려줍니다.
- 값의 타입이 틀렸거나 필수 필드가 빠져 스키마에 맞지 않는 객체는 `INVALID_ARGUMENT`로 실패합니다.
- 거부된 쓰기는 아무것도 바꾸지 않으므로, 트랜잭션은 계속 쓰다가 커밋해도 됩니다.
- 읽어 온 객체는 트랜잭션이 끝나도 남는 평범한 값입니다. 스키마의 필드가 모두 들어 있고, 빠진 필드에는 기본값이나 null이 들어갑니다.

## 객체 조회하기

`Query`에는 어떤 객체를 어떤 순서로 몇 개 찾을지 적습니다. `query`는 찾은 객체를 돌려주고, `count`는 개수를 셉니다.

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

- **조건**은 `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `between`, `is_in`, `contains`, `starts_with`, `ends_with`, `is_null`, `is_not_null`이 있고, `and`와 `or`, `!`로 엮습니다.
- **경로**는 필드 이름이고, 내장 객체나 링크를 지날 때는 `.`으로 잇습니다. `address.city`처럼 쓰고, `author.name`처럼 쓰면 링크가 가리키는 객체를 검사합니다. 가리키는 객체가 없으면 null로 읽습니다.
- **목록.** 목록에 건 조건은 원소 하나라도 맞으면 참입니다. 목록에 `contains`를 쓰면 그 원소가 있는지 봅니다. 빈 목록은 null이 아닙니다.
- **null.** null인 필드에 건 조건은 `is_null`을 빼고 모두 거짓입니다. `Filter::eq(field, Value::Null)`은 `is_null`과 같습니다.
- **타입.** 값은 필드의 타입과 같아야 합니다. `Int` 필드는 정수와 비교하고 실수와는 비교하지 않습니다. 링크는 대상 컬렉션의 키와 비교합니다. 이를 어기거나 없는 필드를 쓴 쿼리는 `INVALID_QUERY`로 실패합니다.
- **순서.** 정렬을 주지 않으면 기본 키 순서로 나오고, 정렬 값이 같은 객체끼리도 기본 키 순서를 따릅니다. null은 오름차순에서 맨 앞, 내림차순에서 맨 뒤에 옵니다. 문자열은 바이트 순서로 비교합니다.

같은 쿼리를 문자열로 쓸 수도 있습니다. 바뀌지 않는 쿼리를 적기에 편하고, 다른 언어에서도 같은 문법을 쓰게 됩니다.

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

필터를 먼저 쓰고 `SORT BY`, `LIMIT`, `OFFSET`을 차례로 붙이며, 모두 생략할 수 있습니다. 키워드는 대소문자를 가리지 않고, 문자열은 큰따옴표로 감쌉니다. `limit`처럼 키워드와 이름이 같은 필드는 백틱으로 감쌉니다. `$0`, `$1` 같은 매개변수에는 함께 넘긴 값이 순서대로 들어갑니다. 프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요. 문법에 맞지 않는 문자열은 `INVALID_QUERY`로 실패하고, 메시지에 문제가 생긴 글자의 위치가 나옵니다.

기본 키나 인덱스가 있는 필드에 건 조건이 나머지 조건과 `and`로 이어져 있으면, 엔진은 그 조건에 맞는 객체만 읽습니다. 인덱스가 있는 필드 하나로만 정렬하면 그 순서대로 읽다가 개수 제한에서 멈춥니다. 그렇지 않으면 컬렉션의 객체를 모두 읽습니다. 어느 쪽으로 읽든 결과는 같습니다.

## 새 버전으로 마이그레이션하기

스키마를 바꾸려면 버전을 올립니다. 더 낮은 버전을 가진 파일을 열면 쓰기 트랜잭션 하나 안에서 마이그레이션합니다. 전부 커밋되거나, 파일이 그대로 남거나 둘 중 하나입니다.

새 컬렉션, 선택 필드나 기본값이 있는 새 필드, 필드 삭제, 인덱스 추가와 삭제는 엔진이 알아서 합니다. 레코드를 다시 쓰지 않으므로, 필드가 생기기 전에 쓴 객체는 그 필드의 기본값을 읽습니다. 그래서 필수 필드에 한번 준 기본값은 없앨 수 없습니다. 그 밖의 변경은 `Migration`에 적습니다.

```rust
use darudb::{Collection, Migration, OpenOptions, Schema, Type};

fn main() -> Result<(), darudb::Error> {
    let v2 = Schema::new(2).collection(
        Collection::new("people")
            .field("full_name", Type::String)
            .optional("email", Type::String)
            .with_default("age", Type::String, "")
            .unique("email"),
    );
    let migration = Migration::to(2)
        .rename_collection("users", "people")
        .rename_field("users", "name", "full_name")
        .replace_field("users", "age")
        .delete_collection("posts")
        .run(|migrating| {
            for key in migrating.previous_keys("users")? {
                let before = migrating.previous("users", key.clone())?;
                let age = before.and_then(|user| user.get("age")?.as_int()).unwrap_or(0);
                let mut people = migrating.collection("people")?;

                if let Some(mut person) = people.get(key)? {
                    person.set("age", format!("{age} years"));
                    people.put(person)?;
                }
            }

            Ok(())
        });

    let db = OpenOptions::new().schema(v2).migration(migration).open("app.darudb")?;
    db.close()
}
```

- **이름 바꾸기**는 데이터를 옮기지 않으므로 객체가 아무리 많아도 비용이 없습니다.
- **필드 교체**는 타입을 바꿀 때 씁니다. 같은 이름으로 새 필드를 만드는 것과 같습니다. **컬렉션 삭제**는 그 객체와 인덱스를 함께 지웁니다.
- **마이그레이션 함수**는 새 스키마로 실행됩니다. `previous`로 읽으면 예전 이름과, 지우거나 교체한 필드의 값까지 예전 스키마대로 읽을 수 있습니다. 그러니 객체를 쓰기 전에 이렇게 읽어 두세요. 지울 컬렉션도 마이그레이션이 커밋되기 전까지는 이렇게 읽을 수 있습니다.
- 함수가 오류를 돌려주면 여는 작업도 그 오류로 실패합니다. 애플리케이션이 이유를 직접 적으려면 `Error::MigrationFailed`를 씁니다.

여러 버전을 건너뛰면 버전 순서대로 차례로 실행합니다. 두 버전 뒤처진 파일은 두 단계를 모두 거치고, 이미 선언한 버전인 파일은 아무 단계도 거치지 않습니다.

## 여러 프로세스와 핸들

핸들은 열 때 받은 스키마를 계속 씁니다. 다른 프로세스나 같은 프로세스의 다른 핸들이 파일을 마이그레이션하면, 예전 핸들로 컬렉션에 접근하는 다음 트랜잭션은 `SCHEMA_MISMATCH`로 실패합니다. 그러면 새 스키마로 다시 열어야 합니다. 마이그레이션 전에 시작한 읽기 트랜잭션은 시작할 때의 커밋을 보므로 예전 스키마로 계속 읽습니다.
