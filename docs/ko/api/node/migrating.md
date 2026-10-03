---
title: Migrating
order: 12
---

# Migrating

`Migrating`은 마이그레이션 함수가 실행되는 쓰기 트랜잭션으로, 새 스키마의 컬렉션과 함께 마이그레이션 전 스키마로 읽은 객체를 보여 줍니다.

```ts
interface Migrating<S> extends WriteTransaction<S>
```

`Database.open`은 파일의 스키마 버전이 더 낮으면 쓰기 트랜잭션 하나 안에서 파일을 마이그레이션합니다. 먼저 단계마다 적힌 이름 바꾸기와 엔진이 알아서 하는 변경을 적용하고, 두 버전 사이에 있는 [Migration](../../types/node/migration.md)의 `run` 함수를 버전 순서대로 `Migrating`과 함께 부릅니다. `collection`은 [WriteTransaction](./write-transaction.md)처럼 새 스키마의 컬렉션을 돌려줍니다. 함수가 예외를 던지면 마이그레이션에서 한 일은 하나도 남지 않고, 파일은 예전 스키마와 데이터를 그대로 유지하며, `open`도 같은 예외를 던집니다. 엔진이 알아서 하는 변경과 함수가 필요한 변경은 [마이그레이션](../../guide/migrations.md)에서 설명합니다.

`open`의 마이그레이션 함수는 동기여야 합니다. promise를 돌려주면 `INVALID_ARGUMENT`로 거부하고, 여는 작업도 실패합니다. 함수가 도는 동안에는 마이그레이션이 파일의 쓰기 잠금을 쥐고 있으므로, 같은 프로세스의 다른 핸들로 같은 파일에 쓰면 `INVALID_ARGUMENT`로 실패합니다.

```ts
const app2 = schema(2, {
  teams: collection({ name: t.string().primaryKey(), city: t.string().optional() }),
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

## 속성

### previousVersion

```ts
readonly previousVersion: number;
```

마이그레이션 전에 파일에 있던 스키마 버전입니다. 모든 단계에서 같습니다.

### version

```ts
readonly version: number;
```

이 단계가 마이그레이션해 가는 버전입니다.

## 메서드

### previous

```ts
previous(collection: string, key: Key): Record<string, unknown> | null;
```

`collection`에서 기본 키가 `key`인 객체를 마이그레이션 전 스키마대로 읽어 돌려주고, 없으면 `null`을 돌려줍니다. `collection`과 객체의 필드는 마이그레이션 전의 이름을 쓰고, 마이그레이션이 지우거나 교체한 필드의 값도 그대로 나옵니다. 마이그레이션이 지우는 컬렉션도 커밋 전까지는 이렇게 읽을 수 있습니다. 예전 스키마에 없던 컬렉션이면 `INVALID_ARGUMENT`로 실패합니다.

객체는 지금 상태 그대로 읽히고, 객체를 쓰면 새 스키마의 필드만 남습니다. 그러니 객체를 쓰기 전에 이렇게 읽어 두세요. 어느 정수가 `bigint`인지 알려 주는 선언이 없으므로, 정수는 number로 읽히고 2^53 너머면 `bigint`로 읽힙니다.

### previousKeys

```ts
previousKeys(collection: string): Key[];
```

마이그레이션 전 이름으로 가리킨 `collection`에 있는 모든 객체의 기본 키를 키 순서대로 돌려줍니다.

## AsyncMigrating

```ts
interface AsyncMigrating<S> extends AsyncWriteTransaction<S> {
  readonly previousVersion: number;
  readonly version: number;
  previous(collection: string, key: Key): Promise<Record<string, unknown> | null>;
  previousKeys(collection: string): Promise<Key[]>;
}
```

`Database.openAsync`가 하는 마이그레이션의 쓰기 트랜잭션입니다. 함수는 [AsyncMigration](../../types/node/migration.md)으로 선언하며 비동기여도 됩니다. `previous`와 `previousKeys`는 promise를 돌려주고, `collection`은 [AsyncWriteCollection](./write-collection.md#asyncwritecollection)을 돌려줍니다. 작업은 부른 순서대로 실행되고, 한 단계는 함수의 promise와 함수가 부른 작업이 await 여부와 관계없이 모두 끝나야 마무리됩니다.

```ts
const db = await Database.openAsync('app.darudb', {
  schema: app2,
  migrations: [
    {
      version: 2,
      renameCollections: [['users', 'people']],
      renameFields: [['users', 'name', 'fullName']],
      replaceFields: [['users', 'age']],
      async run(m) {
        const people = m.collection('people');

        for (const key of await m.previousKeys('users')) {
          const before = await m.previous('users', key);

          await people.update(key, { age: `${before?.age} years` });
        }
      }
    }
  ]
});
```
