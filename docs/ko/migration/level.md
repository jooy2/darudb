---
title: LevelDB
order: 7
languages: [node]
---

# LevelDB에서 옮기기

`classic-level`이나 `level`로 쓴 LevelDB 데이터베이스의 데이터를 DaruDB로 옮기는 방법을 다룹니다. 서브레벨과 키, 값이 컬렉션과 객체, 필드에 어떻게 대응하는지, 모든 항목을 복사하는 스크립트, 그리고 애플리케이션에서 무엇이 바뀌는지 설명합니다.

## 무엇이 무엇에 대응하는지

| `classic-level`로 쓰는 LevelDB | DaruDB |
| --- | --- |
| 데이터베이스. 파일 여러 개가 든 폴더 | 데이터베이스. 파일 하나 |
| 값이 레코드인 서브레벨 | 컬렉션 |
| 애플리케이션이 인덱스로 관리하던 서브레벨. 필드 값에서 키로 | 선언한 인덱스나 고유 인덱스. 객체가 바뀌면 엔진이 함께 고칩니다 |
| 키. 문자열 | 기본 키. 여기서는 문자열이고, 키가 숫자라면 정수 |
| `json` 인코딩의 값 | 객체. 속성마다 필드를 둡니다 |
| `batch` | 쓰기 트랜잭션. 통째로 커밋되거나 통째로 중단됩니다 |
| 이터레이터가 읽는 스냅샷 | 읽기 트랜잭션 |
| 쓰기에 준 `sync: true` | 동기 커밋. 기본값입니다 |
| `sync` 없는 쓰기 | 지연 커밋 |
| `on('put')` 리스너, 훅 | 대응하는 것 없음. 쓰기가 끝나면 다시 읽습니다 |

- **스키마는 애플리케이션에서 나옵니다.** LevelDB는 바이트를 저장하고 `json` 인코딩은 어떤 값이든 쓰므로, 스키마는 애플리케이션이 서브레벨마다 쓰던 객체를 보고 선언합니다. 엔진은 값마다 필드의 타입을 검사하고, 맞지 않는 값이 처음 나오면 복사는 `INVALID_ARGUMENT`로 멈춥니다.
- **키를 그대로 옮깁니다.** 문자열 키는 문자열 키로 남으므로, 그 키를 가리키던 참조는 모두 링크로 살아남습니다. `post!2026-10!42`처럼 애플리케이션이 여러 부분을 이어 만든 키는 부분마다 필드와 인덱스를 둡니다.
- **인덱스는 복사하지 않고 선언합니다.** 이메일에서 사용자의 키를 찾는 서브레벨은 애플리케이션이 직접 관리하던 인덱스입니다. 이것은 복사하지 말고 `email`에 고유 인덱스를 선언하세요. 객체가 들어가면서 엔진이 인덱스를 채웁니다.
- **작업은 동기입니다.** `classic-level`은 프로미스로 답하지만, DaruDB의 `read`와 `write`는 결과를 바로 돌려줍니다. 모양이 같은 비동기 API도 있으며 스레드 풀에서 돕니다. [비동기 API](../guide/async.md)를 보세요.

## 스키마 선언하기

예제로 옮길 데이터베이스에는 애플리케이션이 `json` 값으로 쓴 서브레벨이 셋 있습니다.

| 서브레벨 | 키 | 값 |
| --- | --- | --- |
| `users` | 사용자의 id. 문자열 | `name`, `null`일 수도 있는 `email`, `age` |
| `posts` | 글의 슬러그 | 사용자의 id인 `authorId`, `body`, `publishedAt`, `tags` 목록 |
| `users_by_email` | 이메일 | 그 이메일을 쓰는 사용자의 id |

`users`는 문자열 키를 선언한 기본 키로 그대로 쓰고, `authorId`는 링크가 되며, `users_by_email`은 고유 인덱스가 됩니다.

```ts
import { collection, schema, t } from 'darudb';

const app = schema(1, {
  users: collection({
    id: t.string().primaryKey(),
    name: t.string(),
    email: t.string().optional().unique(),
    age: t.int().default(0).index()
  }),
  posts: collection({
    slug: t.string().primaryKey(),
    author: t.link('users').optional().index(),
    body: t.string(),
    publishedAt: t.string().optional(),
    tags: t.list(t.string()).index()
  })
});
```

## 항목 복사하기

스크립트는 `classic-level`로 데이터베이스를 읽습니다. Node.js에서 `level`이 쓰는 것도 이 패키지입니다.

```js
import { ClassicLevel } from 'classic-level';
import { Database } from 'darudb';

const level = new ClassicLevel('app.level', { createIfMissing: false });
const users = level.sublevel('users', { valueEncoding: 'json' });
const posts = level.sublevel('posts', { valueEncoding: 'json' });
const db = Database.open('app.darudb', { schema: app });

/** `entries`를 트랜잭션마다 천 개씩 `name` 컬렉션에 복사합니다. */
async function copy(entries, name, convert) {
  let batch = [];
  const flush = () => {
    db.write((txn) => txn.collection(name).insertMany(batch), { durability: 'deferred' });
    batch = [];
  };

  for await (const entry of entries) {
    batch.push(convert(entry));

    if (batch.length === 1000) {
      flush();
    }
  }

  if (batch.length > 0) {
    flush();
  }
}

await copy(users.iterator(), 'users', ([key, value]) => ({
  id: key,
  name: value.name,
  email: value.email,
  age: value.age
}));

await copy(posts.iterator(), 'posts', ([key, value]) => ({
  slug: key,
  author: value.authorId,
  body: value.body,
  publishedAt: value.publishedAt,
  tags: value.tags
}));

// 이 호출이 반환되면 지연 커밋이 모두 디스크에 기록됩니다.
db.close();
await level.close();
```

- **애플리케이션은 닫아 둡니다.** LevelDB 데이터베이스는 한 번에 한 프로세스만 열 수 있고 읽기 전용 모드가 없습니다. 원본이 바뀌지 않게 폴더를 복사한 것에서 옮기세요.
- **이터레이터는 만들 때 잡은 스냅샷 하나를 읽으므로**, 그사이에 생긴 쓰기는 복사본에 들어가지 않습니다.
- **서브레벨은 애플리케이션이 붙인 이름과 인코딩 그대로 엽니다.** 기본값이 아닌 `keyEncoding`이나 `valueEncoding`으로 열던 서브레벨은 여기서도 그 값으로 엽니다.

## 복사본 확인하기

서브레벨마다 항목 수(`(await users.keys().all()).length`)를 세어 컬렉션의 `count`와 견줍니다. 그다음 새 파일에 [무결성 검사](../guide/tools.md#파일-검사하기)를 돌립니다. 이 검사는 객체마다 인덱스와도 맞는지 확인합니다.

## 쿼리

애플리케이션이 이터레이터와 직접 관리하던 인덱스 서브레벨로 하던 일은 쿼리가 됩니다. 맞는 인덱스가 있으면 엔진이 그 인덱스로 쿼리를 돌립니다.

| 애플리케이션이 하던 일 | DaruDB 쿼리 언어 |
| --- | --- |
| `users_by_email`에서 id를 얻고, `users`에서 사용자를 읽음 | `users`에서 `email == $0` |
| 키 접두사에 `gte`와 `lt`를 주고 `users`를 훑음 | 키의 그 부분을 담은 필드에 `STARTSWITH` |
| `users`를 훑으며 프로그램에서 나이를 검사함 | `age BETWEEN 18 AND 30` |
| `reverse: true`와 `limit: 10`으로 훑음 | `SORT BY age DESC LIMIT 10` |
| 어떤 태그가 있는 글을 찾으려고 `posts`를 훑음 | `posts`에서 `tags CONTAINS "news"` |
| 글을 읽고, id로 작성자를 읽음 | 작성자 이름으로 글을 찾을 때는 `posts`에서 `author.name == "Alice"` |

쿼리 언어 전체와 빌더는 [쿼리](../guide/queries.md)에 있습니다. 빌더의 타입은 TypeScript가 스키마에서 추론합니다.

## 애플리케이션에서 바뀌는 것

- **인덱스를 직접 관리하지 않습니다.** 객체를 쓰면 같은 트랜잭션 안에서 그 컬렉션에 선언한 인덱스가 모두 고쳐지므로, `users_by_email`을 맞추던 코드는 없어집니다.
- **객체는 스키마의 타입을 따릅니다.** TypeScript가 스키마에서 객체마다 타입을 추론하고, 엔진은 쓸 때마다 필드를 검사합니다.
- **여러 프로세스**가 한 파일을 함께 쓸 수 있습니다. Electron 앱의 메인 프로세스와 보조 프로세스가 그런 경우입니다. 쓰는 쪽은 한 번에 하나, 읽는 쪽은 몇이든 됩니다. [여러 프로세스](../guide/processes.md)를 보세요.
- **트랜잭션**은 함수 하나를 범위로 하는 `db.read`와 `db.write`입니다. 함수가 반환되면 커밋하고, 예외를 던지면 중단합니다. [트랜잭션](../guide/transactions.md)을 보세요.
- **파일 전체 암호화**가 엔진에 들어 있습니다. [암호화](../guide/encryption.md)를 보세요.
- **스키마 변경**은 스키마 버전을 올려서 합니다. 새 컬렉션과 필드, 인덱스는 엔진이 알아서 추가합니다. [마이그레이션](../guide/migrations.md)을 보세요.
