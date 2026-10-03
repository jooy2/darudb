---
title: Object
order: 1
counterpart: /types/node/object-types
---

# Object

`Object`는 필드 값을 이름으로 묶은 것으로, 컬렉션에 저장하는 단위이자 컬렉션에서 읽어 오는 단위입니다.

```rust
#[derive(Clone, PartialEq, Default)]
pub struct Object
```

프로그램은 `Object::new`와 `with`로 객체를 만들어 [`insert`, `put`, `update`](../../api/rust/collection-writer.md)에 넘기고, [`get`](../../api/rust/collection-reader.md)과 `iter`, 쿼리에서 객체를 돌려받습니다. 값 하나하나는 [`Value`](./value.md)입니다.

데이터베이스에서 읽은 객체는 사본입니다. 자동 증가 `id`를 포함해 컬렉션 스키마의 필드를 모두 담고 있고, 값이 없는 선택 필드에는 [`Value::Null`](./value.md)이 들어 있습니다. 트랜잭션과 데이터베이스가 사라진 뒤에도 그대로 쓸 수 있습니다. 프로그램이 만든 객체에는 넣은 필드만 있고, 나머지는 쓸 때 스키마에 따라 채워집니다. 그 규칙은 [Value](./value.md#필드-타입)에 있습니다.

필드는 이름의 바이트 순서로 정렬해 둡니다. 그래서 어떤 순서로 넣었든 `fields`는 이 순서로 돌려주고, 필드와 값이 같은 두 객체는 만든 방법과 상관없이 같습니다. `Debug`는 객체를 `{"age": Int(31), "name": String("Alice")}`처럼 맵으로 출력합니다.

```rust
use darudb::{Database, Object, Value};

fn add_user(db: &Database) -> darudb::Result<Value> {
    let mut txn = db.begin_write()?;
    let id = txn
        .collection("users")?
        .insert(Object::new().with("name", "Alice").with("email", "alice@example.com"))?;

    txn.commit()?;
    Ok(id)
}

fn email_of(db: &Database, id: i64) -> darudb::Result<Option<String>> {
    let read = db.begin_read()?;
    let user = read.collection("users")?.get(id)?;

    Ok(user.and_then(|user| user.get("email")?.as_str().map(str::to_owned)))
}
```

## 연관 함수

### new

```rust
pub fn new() -> Self
```

필드가 하나도 없는 객체를 만듭니다. `Object::default()`도 같습니다.

## 메서드

### with

```rust
pub fn with(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self
```

필드 `name`을 `value`로 설정한 객체를 돌려줍니다. 그 필드에 값이 있었다면 바꿉니다. `"text"`, `42`, `None::<i64>`처럼 `Value`로 바뀌는 값이면 무엇이든 받습니다. 어떤 타입이 바뀌는지는 [Value](./value.md#변환)에 있습니다.

### set

```rust
pub fn set(&mut self, name: impl Into<String>, value: impl Into<Value>) -> Option<Value>
```

필드 `name`을 그 자리에서 `value`로 설정하고, 원래 있던 값을 돌려줍니다. 객체에 그 필드가 없었다면 `None`입니다.

### get

```rust
pub fn get(&self, name: &str) -> Option<&Value>
```

필드 `name`의 값입니다. 객체에 그 필드가 없으면 `None`입니다. 데이터베이스에서 읽은 객체에는 스키마의 필드가 모두 있으므로, `None`이 나오면 그런 필드가 없다는 뜻입니다. 값이 없는 선택 필드는 `Some(&Value::Null)`을 돌려줍니다.

### remove

```rust
pub fn remove(&mut self, name: &str) -> Option<Value>
```

필드 `name`을 객체에서 빼고 그 값을 돌려줍니다. 그 필드가 없었다면 `None`입니다.

### fields

```rust
pub fn fields(&self) -> impl Iterator<Item = (&str, &Value)>
```

필드 이름과 값을 이름의 바이트 순서대로 돌려줍니다.

### len

```rust
pub fn len(&self) -> usize
```

객체에 있는 필드 수입니다.

### is_empty

```rust
pub fn is_empty(&self) -> bool
```

객체에 필드가 하나도 없는지 알려 줍니다.
