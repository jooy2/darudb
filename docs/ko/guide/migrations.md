---
title: 마이그레이션
order: 8
---

# 마이그레이션

스키마를 바꾸려면 버전을 올리고, 더 낮은 버전을 가진 파일을 열면 쓰기 트랜잭션 하나 안에서 마이그레이션해 전부 커밋하거나 파일을 그대로 둡니다.

## 엔진이 알아서 하는 변경

새 컬렉션, 선택 필드나 기본값이 있는 새 필드, 필드 삭제, 인덱스 추가와 삭제는 따로 적지 않아도 엔진이 알아서 합니다. 레코드는 다시 쓰지 않습니다. 필드가 생기기 전에 쓴 객체는 그 필드의 기본값을 읽으므로, 필수 필드에 한번 준 기본값은 없앨 수 없습니다.

그 밖의 변경은 그 변경을 하는 버전의 마이그레이션에 적습니다.

## 그 밖의 변경 적기

::: lang rust

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

함수가 오류를 돌려주면 여는 작업도 그 오류로 실패합니다. 애플리케이션이 이유를 직접 적으려면 `Error::MigrationFailed`를 씁니다.

:::

::: lang node

```ts
const app2 = schema(2, {
  people: collection({
    fullName: t.string(),
    email: t.string().optional().unique(),
    age: t.string().default('')
  })
});

const db = Database.open('app.darudb', {
  schema: app2,
  migrations: [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      deleteCollections: ['teams'],
      run(m) {
        const people = m.collection('people');

        for (const key of m.previousKeys('users')) {
          const before = m.previous('users', key);
          const person = people.get(key);

          if (before !== null && person !== null) {
            people.put({ ...person, age: `${before.age} years` });
          }
        }
      }
    }
  ]
});
```

함수가 예외를 던지면 `open`도 같은 예외를 던집니다. `Database.openAsync`에서는 함수가 비동기여도 되고, `previous`와 `previousKeys`가 promise를 돌려줍니다.

:::

::: lang dart

```dart
@Collection('people')
class Person {
  const Person({this.id, required this.fullName, this.email, this.age = ''});

  final int? id;
  @Name('full_name')
  final String fullName;
  @Unique()
  final String? email;
  final String age;
}

final db = Database.open(
  'app.darudb',
  schema: const Schema(2, [personSchema]),
  migrations: [
    Migration(
      2,
      renameCollections: const {'users': 'people'},
      renameFields: const {
        'users': {'name': 'full_name'},
      },
      replaceFields: const {
        'users': ['age'],
      },
      deleteCollections: const ['teams'],
      run: (m) {
        final people = m.collection(personSchema);

        for (final key in m.previousKeys('users')) {
          final before = m.previous('users', key);
          final person = people.get(key as int);

          if (before != null && person != null) {
            people.put(person.copyWith(age: '${before['age']} years'));
          }
        }
      },
    ),
  ],
);
```

`previous`는 객체를 예전 스키마대로 읽어, 필드를 예전 이름으로 담은 `Map`으로 돌려줍니다. 예전 스키마의 클래스는 보통 프로그램에 남아 있지 않기 때문입니다. 함수가 예외를 던지면 `open`도 같은 예외를 던집니다. `Database.openAsync`에서는 함수가 비동기여도 되지만, 컨텍스트에서 부르는 메서드는 동기 그대로입니다.

:::

::: lang python

```python
import darudb
from darudb import Migration, field


@darudb.collection("people")
class Person:
    id: int | None = None
    full_name: str
    email: str | None = field(default=None, unique=True)
    age: str = ""


def to_v2(m: darudb.Migrating) -> None:
    people = m.collection(Person)

    for key in m.previous_keys("users"):
        before = m.previous("users", key)

        if before is not None:
            people.update(key, age=f"{before['age']} years")


db = darudb.Database.open(
    "app.darudb",
    schema=darudb.Schema(2, [Person]),
    migrations=[
        Migration(
            2,
            rename_collections=[("users", "people")],
            rename_fields=[("users", "name", "full_name")],
            replace_fields=[("users", "age")],
            delete_collections=["teams"],
            run=to_v2,
        )
    ],
)
```

`previous`는 객체를 예전 스키마대로 읽어, 필드를 예전 이름으로 담은 `dict`로 돌려줍니다. 예전 스키마의 클래스는 보통 프로그램에 남아 있지 않기 때문입니다. `run`이 예외를 일으키면 `open`도 같은 예외를 일으킵니다. `Database.open_async`에서는 `run`이 코루틴 함수여도 되고, 이 함수가 받는 `AsyncMigrating`의 호출은 await로 기다립니다.

:::

- **이름 바꾸기**는 데이터를 옮기지 않으므로 객체가 아무리 많아도 비용이 없습니다. 컬렉션은 마이그레이션 전의 이름으로 적습니다.
- **필드 교체**는 타입을 바꿀 때 씁니다. 같은 이름으로 새 필드를 만드는 것과 같습니다. **컬렉션 삭제**는 그 객체와 인덱스를 함께 지웁니다.
- **마이그레이션 함수**는 이름 바꾸기가 끝난 뒤 마이그레이션의 쓰기 트랜잭션 안에서 새 스키마로 실행됩니다. `previous`로 읽으면 예전 이름과, 지우거나 교체한 필드의 값까지 예전 스키마대로 읽을 수 있습니다. 쓴 객체에는 새 스키마의 필드만 남으니, 객체를 쓰기 전에 이렇게 읽어 두세요. 지울 컬렉션도 마이그레이션이 커밋되기 전까지는 이렇게 읽을 수 있습니다.
- **실패한 마이그레이션**은 파일의 예전 스키마와 데이터를 그대로 남깁니다.

## 여러 버전을 한 번에

여러 버전을 건너뛰면 버전 순서대로 차례로 실행합니다. 두 버전 뒤처진 파일은 두 단계를 모두 거치고, 이미 선언한 버전인 파일은 아무 단계도 거치지 않습니다. 어느 버전에서 쓴 파일이든 열 수 있도록, 애플리케이션이 거쳐 온 모든 버전의 마이그레이션을 넘기세요.

파일의 스키마 버전이 선언한 것보다 높으면 `SCHEMA_TOO_NEW`로 실패합니다. 애플리케이션은 자기보다 새 릴리스가 쓴 파일을 열 수 없습니다.
