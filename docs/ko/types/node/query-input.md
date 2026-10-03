---
title: QueryInput
order: 8
---

# QueryInput

`QueryInput`은 JavaScript로 만든 쿼리를 `find`, `findOne`, `count`, `Database.prepare`에 넘기는 형태로, 쿼리를 만드는 함수나 `Query`입니다.

```ts
type QueryInput<O> = ((query: Query<O>) => Query<O> | void) | Query<O>;
```

`O`는 컬렉션의 객체 타입이고, 이 타입 덕분에 [`where`](../../api/node/query.md)와 `sortBy`는 컬렉션의 필드와 그 타입의 값만 받습니다.

- **함수**는 새 [Query](../../api/node/query.md)를 받아 조건을 더합니다. 그 쿼리를 돌려주거나 아무것도 돌려주지 않아도 되고, 다른 값을 돌려주면 `INVALID_QUERY`로 실패합니다.
- **`Query`**는 `new Query<O>()`로 만들고, 한 번 만들어 여러 번 넘길 수 있습니다.
- **아무것도 주지 않으면** `find()`는 모든 객체를 기본 키 순서로 돌려주고, `count()`는 모든 객체를 셉니다.

[ReadCollection](../../api/node/read-collection.md)은 쿼리 언어로 쓴 문자열과 매개변수, [Prepared](./prepared.md) 쿼리도 받는데, 이들은 따로 정의된 오버로드입니다. 쿼리로 무엇을 할 수 있는지는 [쿼리](../../guide/queries.md)에 있습니다.

```ts
const adults = db.read((txn) =>
  txn.collection('users').find((q) => q.where('age', '>=', 18).sortBy('age', 'desc').limit(10))
);
```

## QueryValue

```ts
type QueryValue = boolean | number | bigint | string | Uint8Array;
```

조건이 비교하는 값이나 매개변수의 값입니다. 값의 타입은 JavaScript 타입이 정합니다.

- `Number.MAX_SAFE_INTEGER` 안의 정수인 `number`는 정수이고, 나머지 number는 부동소수점 수입니다. 정수 필드는 정수와만 비교하고 부동소수점 필드는 어떤 number와도 비교하므로, 정수 필드를 `2.5`와 비교하면 `INVALID_QUERY`로 실패합니다.
- `bigint`는 정수입니다. 64비트를 넘으면 `INVALID_QUERY`로 실패합니다.
- `boolean`, `string`, `Uint8Array`는 같은 타입의 필드와 비교하고, 링크는 대상 컬렉션의 키 타입의 값과 비교합니다.

리스트나 객체는 값이 될 수 없습니다. 리스트 필드에 건 조건은 원소 하나와 비교하며, 어느 원소에서든 맞으면 참입니다. 필드와 타입이 맞지 않는 값은 쿼리를 실행할 때 `INVALID_QUERY`로 실패합니다.

## QueryParameters

```ts
type QueryParameters = readonly (QueryValue | null)[];
```

쿼리 매개변수의 값을 차례로 담은 배열로, `$0`이나 `param(0)`이 첫 번째입니다. 문자열 쿼리와 준비한 쿼리가 쿼리 다음 인자로 받습니다.

- `null`은 `==`나 `!=`로 비교하는 매개변수에만 줄 수 있고, 그러면 필드가 null인지를 봅니다. 다른 곳에서는 `INVALID_QUERY`로 실패합니다.
- 값을 받지 못한 매개변수가 있으면 `INVALID_QUERY`로 실패합니다. 배열이 아니거나 `QueryValue`가 아닌 값이 들어 있어도 마찬가지입니다.

```ts
users.find('age >= $0 AND name STARTSWITH $1', [18, 'A']);
users.count('email == $0', [null]); // email이 없는 사용자
```

프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요.

## Comparison

```ts
type Comparison = '==' | '!=' | '<' | '<=' | '>' | '>=';
```

`Query.where`의 비교 연산자로, `where('age', '>=', 18)`처럼 씁니다. `==`와 `!=`에 `null`을 주면 필드가 null인지를 봅니다. `where`는 이 밖에 값의 쌍을 받는 `'between'`, 배열을 받는 `'in'`, 그리고 `'contains'`, `'startsWith'`, `'endsWith'`도 받습니다. 모르는 연산자는 `INVALID_QUERY`로 실패합니다. [`conditions`](../../api/node/conditions.md)의 `eq`, `ne`, `lt`, `le`, `gt`, `ge`가 같은 여섯 가지 비교를 만듭니다.
