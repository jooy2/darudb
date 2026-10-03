---
title: 바인딩 만들기
order: 7
languages: [rust]
---

# 바인딩 만들기

이 페이지는 언어 바인딩이 기대는 Rust 크레이트의 호출과, 바인딩이 엔진과 주고받는 바이트 형식, 그리고 Node.js 패키지가 JavaScript와 엔진 사이에 일을 나누는 방식을 설명합니다.

## 바인딩이 하는 일

바인딩은 옮기고, 결정은 엔진이 합니다. 객체를 스키마와 대조하는 일, 인덱스를 객체와 맞춰 두는 일, 쿼리를 파싱하고 실행 계획을 세워 실행하는 일, 마이그레이션에서 저장된 스키마를 바꾸는 일, 그리고 모든 오류 코드가 엔진에 있습니다. 그래야 어느 언어든 파일을 똑같이 읽습니다. 바인딩이 하는 일은 다음과 같습니다.

- 스키마와 마이그레이션을 자기 언어로 선언해 엔진에 넘깁니다.
- 객체를 레코드로 쓰고, 레코드를 읽어 객체로 돌려줍니다.
- 쿼리를 IR로 만들거나, 쿼리 언어로 쓴 문자열을 엔진에 넘겨 파싱하게 합니다.
- 엔진이 버전 단계마다 멈춰 주는 마이그레이션 사이사이에 자기 언어로 마이그레이션 함수를 실행합니다.
- 모든 오류의 코드를 바꾸지 않고 그대로 전달합니다.

Node.js 패키지가 napi-rs로 이렇게 만들어졌고, Dart 패키지도 같은 방식으로 만들 계획입니다. 아래에 나오는 형식은 저장소의 [design/objects.md](https://github.com/jooy2/darudb/blob/main/design/objects.md)에 명세돼 있습니다.

## 스키마를 주고 열기

바인딩은 스키마를 자기 언어로 선언하고, 파일이 스키마를 저장하는 형식으로 인코딩합니다. 이때 컬렉션과 필드의 ID는 바인딩이 마음대로 정합니다. 이렇게 인코딩한 스키마는 [`Schema::decode`](../api/rust/schema.md)가 읽습니다. 바인딩이 정한 ID는 링크와 인덱스가 가리키는 대상을 잇는 데만 쓰이고, 컬렉션과 필드의 실제 ID는 파일이 따로 매깁니다.

마이그레이션마다 구조를 바꾸는 부분, 곧 이름을 바꾸는 컬렉션과 필드, 지우는 컬렉션, 교체하는 필드는 함수 없는 [`Migration`](../api/rust/migration.md)으로 넘깁니다. 함수는 바인딩의 언어에 남겨 두고, 파일은 `OpenOptions::open_migrating`으로 엽니다.

```rust
pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening>
```

파일에 선언한 스키마가 이미 있거나, 스키마가 없던 파일에 새로 저장했다면 데이터베이스를 담은 [`Opening::Open`](../api/rust/opening.md)을 돌려줍니다. 파일에 더 낮은 버전의 스키마가 있으면 `PendingMigration`을 담은 `Opening::Migrating`을 돌려줍니다.

### 마이그레이션 실행하기

[`PendingMigration`](../api/rust/opening.md#pendingmigration)은 마이그레이션의 쓰기 트랜잭션입니다. 새 스키마는 이미 그 안에 저장돼 있고 새 인덱스도 만들어져 있습니다. 바인딩은 버전 단계를 하나씩 받아 오고, 단계마다 자기 함수를 같은 트랜잭션 안에서 실행합니다.

```rust
use darudb::{Database, Migrating, Opening, OpenOptions};

/// Runs the binding's own migration function for `version`, if it has one.
fn run_step(version: u64, migrating: &mut Migrating<'_>) -> darudb::Result<()> {
    // Call into the binding's language here.
    let _ = (version, migrating);
    Ok(())
}

fn open(options: &OpenOptions, path: &str) -> darudb::Result<Database> {
    match options.open_migrating(path)? {
        Opening::Open(database) => Ok(database),
        Opening::Migrating(mut pending) => {
            while let Some(version) = pending.next_step()? {
                run_step(version, &mut pending.migrating())?;
            }

            pending.finish()
        }
    }
}
```

| `PendingMigration`의 멤버 | 주는 것 |
| --- | --- |
| `previous_version`, `version` | 파일에 있는 스키마 버전과 마이그레이션이 도달할 버전 |
| `schema_record` | 파일이 저장한 형태 그대로의 새 스키마. 파일이 매긴 ID가 들어 있습니다 |
| `previous_schema_record` | 파일이 저장했던 형태 그대로의 옛 스키마. 옛 레코드를 해석할 때 씁니다 |
| `next_step` | 다음 단계의 `Migration`에 Rust 함수가 있으면 실행하고 그 단계의 버전을 돌려줍니다. 모든 단계를 마치면 `None`입니다 |
| `migrating` | 마이그레이션 함수가 받는 형태의 트랜잭션인 [`Migrating`](../api/rust/migrating.md). `collection`, `previous_keys`, `previous_record`가 있습니다 |
| `transaction` | 쓰기 트랜잭션 자체. 컬렉션은 새 스키마를 따릅니다 |
| `finish` | 남은 단계를 실행하고, 단계들이 지우는 컬렉션을 지우고, 커밋한 뒤 데이터베이스를 돌려줍니다 |

어느 단계에서 오류가 나면 마이그레이션은 거기서 끝납니다. 오류 뒤든 마치는 대신이든 `PendingMigration`을 버리면 파일은 옛 스키마와 데이터를 그대로 유지합니다. 그때까지 마이그레이션이 쓰기 잠금을 쥐고 있으므로, 바인딩은 마이그레이션 함수가 같은 파일의 쓰기를 기다리지 않게 해야 합니다.

## 필드 ID

레코드에는 필드 이름 대신 필드 ID가 들어갑니다. `Database::schema_record`는 핸들을 열 때 쓴 스키마를 파일이 저장한 레코드 형태로 돌려주며, 여기에는 모든 컬렉션과 필드, 인덱스에 파일이 매긴 ID가 들어 있습니다. 스키마 없이 연 핸들이면 `None`입니다. 바인딩은 이 레코드를 한 번 해석해 두고, 그 뒤로는 이름을 ID로 바꿔 씁니다.

```rust
pub fn schema_record(&self) -> Option<&[u8]>
```

## 레코드

객체는 레코드로 경계를 넘습니다.

```text
record  = count field*          the fields present, ascending by field id
field   = id value
value   = tag payload
```

`count`와 `id`, 모든 길이는 부호 없는 LEB128 가변 길이 정수이고, 값이 null인 필드는 빠집니다.

| 태그   | 타입         | 내용                           |
| ------ | ------------ | ------------------------------ |
| `0x02` | `bool` false | 없음                           |
| `0x03` | `bool` true  | 없음                           |
| `0x04` | `int`        | 지그재그 LEB128 가변 길이 정수 |
| `0x05` | `float`      | 리틀 엔디언 8바이트            |
| `0x06` | `string`     | 길이, 그다음 UTF-8 바이트      |
| `0x07` | `bytes`      | 길이, 그다음 바이트            |
| `0x08` | `list`       | 개수, 그다음 그만큼의 값       |
| `0x09` | 내장 객체    | 길이, 그다음 레코드            |
| `0x0A` | `link`       | 값 하나. 대상의 기본 키        |

갱신할 때 보내는 변경 내용은 바꿀 필드만 담은 레코드이며, 여기서는 필드에 내용 없는 태그 `0x01`을 넣어 그 필드를 null로 만들 수 있습니다. 다른 레코드에는 이 태그가 들어가지 않습니다.

**쓰기.** [`CollectionWriter`](../api/rust/collection-writer.md)는 호출마다 레코드를 하나씩 받습니다.

| 호출                          | 하는 일                                                     |
| ----------------------------- | ----------------------------------------------------------- |
| `insert_record(record)`       | 객체를 넣고 기본 키를 돌려줍니다                            |
| `put_record(record)`          | 객체를 넣거나 같은 키의 객체를 바꾸고, 기본 키를 돌려줍니다 |
| `update_record(key, changes)` | 변경 내용에 있는 필드를 바꾸고, 객체가 있었는지 돌려줍니다  |

세 호출 모두 레코드를 스키마와 대조하고, `insert`처럼 기본값과 자동 증가 키를 채우며, 인덱스를 맞춰 둡니다. 해석할 수 없는 레코드나 컬렉션에 없는 ID 또는 타입이 든 레코드는 `INVALID_ARGUMENT`로, 이미 있는 기본 키나 고유 값은 `DUPLICATE_KEY`로 거부합니다. 쓰기가 거부돼도 트랜잭션은 그대로 커밋할 수 있습니다.

**읽기.** [`CollectionReader`](../api/rust/collection-reader.md)와, 트랜잭션의 변경을 반영하는 `CollectionWriter`가 레코드를 돌려줍니다.

| 호출 | 하는 일 |
| --- | --- |
| `get_record(key)` | 그 기본 키를 가진 객체의 레코드 |
| `get_record_with(key, visit)` | 같은 레코드를 따로 벡터에 복사하지 않고, 있던 자리 그대로 `visit`에 빌려줍니다 |
| `query_records(query)` | 쿼리가 찾은 레코드를 쿼리의 순서대로 |
| `query_records_with(query, visit)` | 같은 레코드를 하나씩 `visit`에 빌려줍니다 |

`_with`가 붙은 호출을 쓰면, 레코드를 자기 버퍼로 옮기는 바인딩은 레코드마다 한 번만 복사합니다. 레코드는 파일에서 온 것이고 엔진은 내보낼 때 다시 검사하지 않으므로, 바인딩은 레코드를 믿을 수 없는 입력으로 다뤄야 합니다. 레코드에 없는 필드는 그 필드가 생기기 전에 쓴 레코드에서만 생기며, 기본값이나 null로 읽습니다. 스키마에 더는 없는 ID는 지운 필드의 것이므로 건너뜁니다.

기본 키는 [`Value`](../types/rust/value.md)로 넘깁니다. 컬렉션의 키 타입에 따라 `Value::Int`, `Value::String`, `Value::Bytes` 중 하나입니다. 호출은 레코드를 하나씩 받으므로, 여러 개를 묶어 보내는 일은 바인딩이 맡습니다. Node.js 패키지는 객체 여러 개를 레코드마다 길이를 앞에 붙여 버퍼 하나로 보내고, 네이티브 계층이 레코드마다 `insert_record`나 `put_record`를 부릅니다. 그래서 묶음 하나가 언어 경계를 한 번만 넘습니다.

## 쿼리

쿼리는 IR로, 버퍼 하나에 담겨 넘어갑니다. 모양이 정해진 레코드입니다.

| 필드 | 이름 | 값 |
| --- | --- | --- |
| 1 | collection | `string` |
| 2 | filter | 식. 내장 객체이며, 필드 1은 연산자, 필드 2는 문자열 목록으로 된 경로, 필드 3은 값, 필드 4는 하위 식입니다 |
| 3 | sort | 객체 목록. 객체마다 경로와 내림차순 여부가 있습니다 |
| 4 | offset | `int` |
| 5 | limit | `int` |
| 6 | count | `bool`. 객체를 돌려주는 대신 개수를 셉니다 |

식 안의 값 자리에는 매개변수가 올 수도 있습니다. 필드 1에 매개변수 번호가 든 내장 객체입니다.

[`QueryRequest::decode`](../api/rust/query-request.md)는 IR을 읽어 컬렉션과 [`Query`](../api/rust/query.md), 개수를 셀지 여부로 나누고, 해석할 수 없는 IR은 `INVALID_QUERY`로 거부합니다. 바인딩은 그다음 컬렉션에서 쿼리를 실행합니다.

```rust
use darudb::{QueryRequest, ReadTransaction};

/// What a query in IR finds: the records, or how many there are.
enum Found {
    Records(Vec<Vec<u8>>),
    Count(u64),
}

fn run(txn: &ReadTransaction, ir: &[u8]) -> darudb::Result<Found> {
    let request = QueryRequest::decode(ir)?;
    let collection = txn.collection(&request.collection)?;

    if request.count {
        collection.count(&request.query).map(Found::Count)
    } else {
        collection.query_records(&request.query).map(Found::Records)
    }
}
```

**쿼리 언어로 쓴 문자열**은 엔진이 파싱하므로, 모든 바인딩이 파서 하나를 함께 씁니다. `Query::prepare`는 문자열을 같은 쿼리로 파싱하면서 `$0`, `$1` 같은 자리를 매개변수로 남겨 두고, `Query::bind_encoded`는 버퍼 하나로 그 매개변수에 값을 줍니다. 이 버퍼는 레코드이며, 필드 0은 값의 개수이고 필드 `n + 1`은 매개변수 `n`의 값입니다. null인 값은 빠집니다. 준비해 둔 쿼리를 보관하는 바인딩은 실행할 때마다 다시 파싱하지 않고 값만 묶습니다. `QueryRequest::encode`는 요청을 다시 IR로 바꾸고, 묶은 값은 제자리에 넣습니다. 파싱한 결과를 바인딩이 직접 들고 있고 싶을 때 씁니다.

엔진은 쿼리를 실행할 때마다 컬렉션의 스키마와 대조합니다. 컬렉션에 없는 필드를 쓰거나 필드를 다른 타입의 값과 비교하는 쿼리는 `INVALID_QUERY`로 거부합니다.

## 오류

모든 [`Error`](../types/rust/error.md)에는 `NOT_FOUND`나 `DUPLICATE_KEY` 같은 고정된 코드가 있고, `Error::code`로 얻습니다. 바인딩은 이 코드를 바꾸지 않고 그대로 전달하며, JavaScript에서는 `error.code`가 됩니다. 바인딩 자신의 실패에도 같은 코드를 씁니다. 닫은 뒤에 쓴 객체에는 `CLOSED`를, 변환할 수 없는 값에는 `INVALID_ARGUMENT`를 쓰는 식입니다. 한 번 릴리스한 코드는 이름을 바꾸지 않습니다. `Error`는 배리언트가 늘어날 수 있으므로 `match`에는 나머지를 받는 갈래가 필요하고, 코드로 판단하는 편이 대개 더 간단합니다.

## 스레드와 이벤트 루프

엔진 호출은 끝날 때까지 스레드를 붙잡습니다. 디스크를 기다리거나, 바쁨 대기 시간만큼 쓰기 잠금을 기다릴 수 있습니다. 이벤트 루프가 있는 언어의 바인딩은 호출을 다른 곳에서 실행합니다. Node.js 패키지의 [비동기 API](../guide/async.md)는 napi-rs `AsyncTask`로 libuv 스레드 풀에서 호출을 실행하며, 트랜잭션은 뮤텍스로 감싸 풀 스레드로 옮깁니다. 한 트랜잭션의 작업은 부른 순서대로 묶어서, 한 번에 한 묶음씩 보냅니다.

여기에는 세 가지 규칙이 따릅니다.

- **풀 스레드가 같은 프로세스의 쓰는 쪽을 기다리게 하지 마세요.** `begin_write`는 같은 프로세스에서 이미 실행 중인 쓰기 트랜잭션을 기다립니다. 그 트랜잭션을 기다리는 작업들이 풀 스레드를 모두 차지했는데 실행 중인 트랜잭션이 끝나려면 스레드가 필요하다면, 어느 쪽도 나아가지 못합니다. Node.js 패키지는 프로세스의 쓰기를 파일마다 JavaScript에서 줄 세우고 하나씩 풀에 넘깁니다. 줄은 파일의 장치와 아이노드로, Windows에서는 실제 경로로 구별하므로, 같은 파일에 이르는 두 경로는 한 줄을 씁니다. `syncAsync`와 `closeAsync`도 같은 줄에 섭니다. 동기화가 쓰는 쪽을 기다리기 때문입니다.
- **데이터베이스 파일을 직접 열지 마세요.** 유닉스 계열에서는 그 디스크립터를 닫는 순간 엔진이 그 파일에 쥔 잠금이 모두 풀립니다. `stat`으로 장치와 아이노드를 알아내는 것은 파일을 열지 않으므로 괜찮습니다.
- **읽기 트랜잭션은 바로 끝내세요.** 열어 둔 읽기 트랜잭션은 모든 프로세스에서 자기 커밋의 페이지가 다시 쓰이지 못하게 하므로 파일이 커집니다. Node.js 패키지처럼 트랜잭션을 함수 하나의 범위에 묶는 식으로, 바인딩은 읽기 트랜잭션을 잊고 열어 두기 어렵게 만드는 편이 좋습니다.

`OpenOptions::key`와 `OpenOptions::password`는 비밀 값을 엔진이 버릴 때 지우는 버퍼에 복사합니다. Node.js 패키지는 호출하는 쪽의 비밀 값을 자기 버퍼에 복사해 비동기 작업이 돌기 전에 그 버퍼로 옵션을 만들고, 그다음 버퍼를 0으로 채웁니다.

## Node.js 패키지가 일을 나누는 방식

| JavaScript, `packages/node/lib` | 엔진 |
| --- | --- |
| `t`, `collection`, `schema`로 스키마를 선언하고 스키마 레코드로 인코딩합니다(`lib/codec.ts`의 `encodeSchema`) | 스키마를 검사하고, 저장하고, 파일의 스키마와 비교하고, ID를 매깁니다 |
| 스키마 레코드에서 파일이 매긴 ID를 읽습니다(`decodeSchema`) | 저장된 스키마를 관리하고, 다른 핸들이나 프로세스의 마이그레이션을 알아챕니다 |
| 객체를 레코드로 인코딩하고 레코드를 객체로 해석합니다. 레이아웃마다 코드를 생성해 씁니다 | 모든 레코드를 스키마와 대조하고, 인덱스 항목과 함께 씁니다 |
| 빌더로 쿼리의 IR을 만들고(`encodeQuery`), 매개변수 값을 인코딩합니다(`encodeParameters`) | IR을 해석하고, 쿼리 문자열을 파싱하고, 쿼리를 스키마와 대조하고, 인덱스를 골라 실행합니다 |
| `next_step`이 돌려주는 단계 사이에 마이그레이션 함수를 실행합니다 | 이름 바꾸기, 새 필드와 인덱스, 단계들이 지우는 것 지우기, 커밋을 맡습니다 |
| 파일마다 쓰기를 줄 세워 스레드 풀에 넘기고, 비밀 값을 지웁니다 | 잠금, 트랜잭션, 커밋, 복구, 암호화를 맡습니다 |

둘 사이에서 `packages/node/src/lib.rs`의 네이티브 계층이 인자를 변환하고, 쿼리가 찾은 레코드를 레코드마다 길이를 앞에 붙여 버퍼 하나로 돌려주며, `Error`를 같은 `code`를 가진 JavaScript 오류로 바꿉니다.
