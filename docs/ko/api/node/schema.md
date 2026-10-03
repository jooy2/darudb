---
title: schema
order: 2
---

# schema

`schema`는 데이터베이스의 컬렉션을 버전과 함께 선언하고, `Database.open`이 받는 `Schema`를 돌려줍니다.

```ts
const schema: <C extends Record<string, Collection<any>>>(
  version: number,
  collections: C
) => Schema<C>;
```

`version`은 1 이상의 정수이고, 스키마가 바뀔 때마다 올립니다. `collections`에는 [collection](./collection.md)으로 만든 컬렉션을 파일에서 쓸 이름으로 넣습니다. TypeScript 타입은 이 선언에서 나오므로, 이 스키마로 연 `Database`는 컬렉션 이름과 각 컬렉션의 객체 타입을 모두 압니다.

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

이 섹션의 다른 페이지에 있는 예제는 모두 이 `db`를 씁니다.

버전이 1 이상의 정수가 아니거나 `collection`으로 만들지 않은 컬렉션이 있으면 `schema`가 곧바로 `INVALID_ARGUMENT`를 던집니다. 나머지는 데이터베이스를 열 때 검사합니다. 스키마에 없는 컬렉션을 가리키는 링크나 기본 키가 둘인 컬렉션처럼 엔진이 저장할 수 없는 스키마라면 이때 `INVALID_ARGUMENT`가 납니다.

처음 열 때 스키마를 파일에 저장하고, 그 뒤로는 열 때마다 선언한 스키마를 저장된 것과 비교합니다.

- **버전과 내용이 모두 같으면** 할 일이 없습니다. 컬렉션이나 인덱스를 선언한 순서만 다른 것은 변경이 아닙니다.
- **버전은 같은데 내용이 다르면** 버전을 올리지 않고 스키마를 바꾼 것이므로 `SCHEMA_MISMATCH`로 실패합니다.
- **파일의 버전이 더 높으면** 더 새 애플리케이션이 쓴 파일이므로 `SCHEMA_TOO_NEW`로 실패합니다.
- **파일의 버전이 더 낮으면** 마이그레이션합니다. 자세한 내용은 [마이그레이션](../../guide/migrations.md)에 있습니다.

돌려받는 `Schema`는 동결된 객체여서 여러 번 열 때 함께 써도 됩니다.

```ts
interface Schema<C extends Record<string, Collection<any>> = Record<string, Collection>>
```

## 속성

### version

```ts
readonly version: number;
```

스키마 버전입니다.

### collections

```ts
readonly collections: C;
```

받은 그대로의 컬렉션을 이름별로 담고 있습니다.
