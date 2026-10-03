---
title: 트랜잭션
order: 5
---

# 트랜잭션

모든 읽기는 읽기 트랜잭션 안에서, 모든 변경은 쓰기 트랜잭션 안에서 일어나며, 쓰기 트랜잭션은 전부 커밋되거나 하나도 남지 않습니다.

## 읽고 쓰기

::: lang rust

`begin_read`는 읽기 트랜잭션을, `begin_write`는 쓰기 트랜잭션을 시작합니다. 쓰기 트랜잭션의 변경은 `commit`이 반환될 때 한꺼번에 파일에 반영됩니다. `commit` 없이 버린 트랜잭션은 취소되고, 그 안에서 한 일은 파일에 남지 않습니다.

```rust
use darudb::{Database, Object};

fn add_user(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    txn.collection("users")?.insert(Object::new().with("name", "Alice"))?;
    txn.commit()?;

    let read = db.begin_read()?;
    println!("{} users", read.collection("users")?.len()?);

    Ok(())
}
```

:::

::: lang node

`write`는 함수를 쓰기 트랜잭션 안에서 실행하고, 함수가 반환하면 커밋합니다. 함수가 예외를 던지면 그 안에서 한 일은 남지 않습니다. `read`는 함수를 읽기 트랜잭션 안에서 실행합니다. 둘 다 함수가 반환한 값을 돌려주고, 트랜잭션이 함수보다 오래 남지 않습니다. 함수가 반환한 뒤에 트랜잭션이나 컬렉션을 쓰면 `CLOSED`가 납니다.

```ts
const key = db.write((txn) => txn.collection('users').insert({ name: 'Alice' }));

const count = db.read((txn) => txn.collection('users').count());
```

- 트랜잭션은 동기입니다. promise를 돌려주는 함수는 거부하고, 그 트랜잭션은 취소합니다. 비동기 함수는 [비동기 API](./async.md)가 받습니다.
- 쓰기 트랜잭션은 겹칠 수 없습니다. 다른 쓰기 함수 안에서 `db.write`를 부르면 자기 자신을 기다리는 대신 곧바로 실패합니다.

:::

읽기 트랜잭션은 시작한 뒤에 무엇이 커밋되든 살아 있는 동안 커밋 하나만 보고, 쓰기를 기다리지 않습니다. 쓰기 트랜잭션은 모든 핸들과 프로세스를 통틀어 파일마다 한 번에 하나입니다. 쓰기는 이미 실행 중인 트랜잭션을 바쁨 대기 시간만큼 기다리고, 그래도 끝나지 않으면 `BUSY`로 실패합니다. 바쁨 대기 시간은 <LangCode rust="OpenOptions::busy_timeout" node="busyTimeout" />으로 바꾸지 않으면 5초입니다.

## 동기 커밋과 지연 커밋

커밋은 기본적으로 디스크에 기록될 때까지 기다렸다가 반환합니다. 지연 커밋은 기다리지 않습니다. 변경은 곧바로 읽기에 보이고, 이후 커밋과 함께 디스크에 기록됩니다.

::: lang rust

```rust
use darudb::{Database, Object};

fn record_click(db: &Database) -> Result<(), darudb::Error> {
    let mut txn = db.begin_write()?;
    txn.collection("events")?.insert(Object::new().with("kind", "click"))?;
    txn.commit_deferred()?;

    // 이 호출이 반환되면 지금까지 커밋한 것은 모두 디스크에 있습니다.
    db.sync()
}
```

:::

::: lang node

```ts
db.write((txn) => txn.collection('events').insert({ kind: 'click' }), { durability: 'deferred' });

// 이 호출이 반환되면 지금까지 커밋한 것은 모두 디스크에 있습니다.
db.sync();
```

:::

지연 커밋은 다음 동기 커밋이나 <LangCode rust="Database::sync" node="db.sync()" />, 데이터베이스를 닫을 때, 또는 1초를 기다린 뒤에 디스크에 기록됩니다. 이미 파일에 들어가 있으므로 프로세스가 비정상 종료돼도 하나도 잃지 않습니다. 정전이 나면 가장 최근 것부터 되돌려질 수 있지만, 중간이 빠지거나 파일이 손상되지는 않습니다. 되돌아온 상태는 실제로 있었던 커밋과 그 앞의 모든 커밋입니다.

::: lang rust

`OpenOptions::max_unsynced_time`은 지연 커밋이 기다릴 수 있는 시간이며 기본값은 1초입니다. `OpenOptions::max_unsynced_pages`는 지연 커밋이 쓴 페이지가 몇 개까지 쌓이면 다음 커밋에서 동기화할지 정하며 기본값은 16,384입니다.

:::

동기 커밋은 디스크를 한 번 기다리는 비용이 들고, 작은 커밋에서는 대부분의 기계에서 이 비용이 가장 큽니다. 정전으로 잃어도 괜찮은 커밋, 예를 들어 이벤트 기록 같은 것은 미루고, 잃으면 안 되는 커밋은 동기로 하세요.

## 페이지 캐시

프로세스는 읽은 페이지를 캐시에 두므로, 같은 페이지를 다시 읽을 때는 파일을 읽지도 검사하지도 않습니다. 캐시는 열린 파일마다 기본 32 MiB까지 쓰며 페이지를 읽는 만큼만 차므로, 그보다 작은 데이터베이스는 그만큼을 다 쓰지 않습니다. 크기는 <LangCode rust="OpenOptions::cache_size" node="cacheSize" />에 바이트 단위로 정합니다. 자주 읽는 큰 데이터베이스라면 늘리고, 모바일 앱 확장처럼 메모리가 적은 프로세스라면 줄이세요.

프로세스 안에서 이미 연 파일을 다시 열면 같은 데이터베이스의 핸들이 하나 더 생기고, 두 핸들은 캐시와 쓰기를 함께 씁니다.
