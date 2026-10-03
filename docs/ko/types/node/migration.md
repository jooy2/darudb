---
title: Migration
order: 4
counterpart: /api/rust/migration
---

# Migration

`Migration`은 스키마 버전 `version`이 바로 앞 버전에서 바꾼 것 가운데 엔진이 스스로 알아내지 못하는 변경을 적어 두는 객체입니다.

```ts
interface Migration<S = Schema>
```

[OpenOptions](./open-options.md#migrations)의 `migrations` 필드가 이 객체의 배열을 받습니다. 예전 스키마 버전을 담은 파일을 열면 선언한 버전까지의 모든 버전 단계를 버전 순서대로, 쓰기 트랜잭션 하나 안에서 실행합니다. 어느 한 부분이라도 실패하면 파일은 예전 스키마와 데이터를 그대로 유지합니다. `S`는 `Database.open`에 준 스키마의 타입이고, 컬렉션의 타입을 `run`까지 전합니다.

엔진은 몇 가지를 알아서 바꿉니다. 새 컬렉션을 만들고, 선택 필드나 기본값이 있는 새 필드를 더하고, 새 인덱스를 만들고 없어진 인덱스를 지우며, 없어진 필드를 정리합니다. 나머지는 마이그레이션에 적습니다. 이름이 바뀐 컬렉션이나 필드, 지운 컬렉션, 타입이 바뀐 필드가 여기에 해당합니다. 없어진 컬렉션이나 타입이 바뀐 필드를 적지 않으면 `INVALID_ARGUMENT`로 실패하고, 단계 전의 스키마에 없는 이름을 적어도 마찬가지입니다. `renameFields`에 적지 않고 이름을 바꾼 필드는 없어진 필드 하나와 새 필드 하나로 보므로, 값이 새 이름으로 따라가지 않습니다. 한 단계에 적는 이름은 모두 그 단계 전의 이름입니다. 그래서 같은 단계에서 이름을 바꾸는 컬렉션의 필드는 컬렉션의 옛 이름으로 적습니다. 마이그레이션 전체의 흐름은 [마이그레이션](../../guide/migrations.md)에 있습니다.

다음 마이그레이션은 `users`에 `name`과 정수 `age`가 있던 버전 1의 파일을 버전 2로 올립니다.

```ts
import { collection, Database, schema, t } from 'darudb';

const v2 = schema(2, {
  people: collection({ fullName: t.string(), age: t.string().default('') })
});

const db = Database.open('app.darudb', {
  schema: v2,
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

## 필드

### version

```ts
version: number;
```

이 단계가 올리는 스키마 버전입니다. 2부터 선언한 스키마의 버전까지의 정수여야 합니다. 그 밖의 수를 주거나 같은 버전으로 가는 마이그레이션이 둘이면 `INVALID_ARGUMENT`로 실패합니다.

### renameCollections

```ts
renameCollections?: [from: string, to: string][];
```

컬렉션의 옛 이름과 새 이름의 쌍입니다. 컬렉션의 객체는 제자리에 그대로 있으므로 이름을 바꿔도 아무것도 복사하지 않습니다. 새 이름을 다른 컬렉션이 쓰고 있으면 `INVALID_ARGUMENT`로 실패합니다.

### renameFields

```ts
renameFields?: [collection: string, from: string, to: string][];
```

단계 전의 컬렉션 이름, 필드의 옛 이름, 새 이름을 차례로 적습니다. 레코드에는 필드 이름이 아니라 필드 id가 들어 있어서 객체를 다시 쓰지 않습니다. 컬렉션에 이미 있는 이름으로 바꾸면 `INVALID_ARGUMENT`로 실패합니다.

### deleteCollections

```ts
deleteCollections?: string[];
```

객체와 함께 없앨 컬렉션입니다. 새 스키마에서 빠진 컬렉션은 반드시 여기에 적어야 합니다. 객체는 `run`이 끝난 뒤 마지막에 지우므로, `run`에서는 아직 `previous`로 읽을 수 있습니다.

### replaceFields

```ts
replaceFields?: [collection: string, field: string][];
```

이름이 같은 새 필드로 갈아 끼울 필드입니다. 필드의 타입이 바뀔 때 씁니다. 교체한 필드는 없어진 필드 하나와 새 필드 하나로 치므로, 새 필드가 필수라면 기본값이 있어야 합니다. 옛 값은 `run`에서 `previous`로 계속 읽을 수 있습니다. 기본 키는 교체할 수 없고, 적으면 `INVALID_ARGUMENT`로 실패합니다.

### run

```ts
run?(migrating: Migrating<S>): void;
```

마이그레이션의 쓰기 트랜잭션 안에서 실행하는 함수입니다. 이름 바꾸기가 끝나 새 스키마가 자리 잡고 인덱스까지 만든 뒤에 실행됩니다. [Migrating](../../api/node/migrating.md)은 새 스키마의 컬렉션을 주고, `previous`와 `previousKeys`로는 옛 스키마로 읽은 객체를 줍니다. 객체를 쓰기 전에 이렇게 읽어 두세요. 쓴 객체에는 새 스키마의 필드만 남습니다. 함수가 있는 버전 단계마다 하나씩, 버전 순서대로 실행됩니다.

- 함수는 동기여야 합니다. promise를 돌려주면 `INVALID_ARGUMENT`로 실패합니다.
- 함수가 예외를 던지면 마이그레이션을 버리고, 파일은 예전 스키마와 데이터를 유지하며, `open`은 같은 예외를 던집니다.
- 함수가 도는 동안 마이그레이션이 파일의 쓰기 잠금을 쥐고 있습니다. 그래서 함수 안에서 어느 `Database`로든 같은 파일에 쓰면, 자기 자신을 기다리는 대신 `INVALID_ARGUMENT`로 실패합니다.

## AsyncMigration

```ts
interface AsyncMigration<S = Schema> extends Omit<Migration<S>, 'run'> {
  run?(migrating: AsyncMigrating<S>): Promise<void> | void;
}
```

`Database.openAsync`가 받는 마이그레이션입니다. 필드는 `Migration`과 같지만 `run`이 비동기여도 되고, [AsyncMigrating](../../api/node/migrating.md#asyncmigrating)을 받습니다. 여기서는 컬렉션의 메서드와 `previous`, `previousKeys`가 promise를 돌려줍니다. 단계는 `run`이 돌려준 값과 `run`이 부른 작업이 모두 끝나야 마무리되므로, await하지 않은 작업도 그 단계에 속합니다. `run`이 거부되면 `openAsync`도 같은 오류로 거부되고, 파일은 예전 스키마와 데이터를 유지합니다.
