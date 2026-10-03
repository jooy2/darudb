---
title: Query
order: 9
---

# Query

`Query`는 컬렉션의 `find`, `findOne`, `count`가 어떤 객체를 어떤 순서로 몇 개 찾을지 적습니다.

```ts
interface Query<O = Record<string, unknown>>
```

쿼리에는 필터, 정렬, 오프셋, 개수 제한이 있고 모두 생략할 수 있습니다. `find`, `findOne`, `count`는 받은 함수에 새 쿼리를 넘겨 줍니다. 쿼리를 보관하거나 다른 곳에 넘기거나 준비해 두려면 `new Query<O>()`로 만듭니다. 메서드는 모두 쿼리에 내용을 더하고 같은 쿼리를 돌려줍니다. 그래서 보관해 둔 쿼리를 다시 실행하면 그때까지 더한 내용이 모두 적용됩니다. 정렬을 주지 않으면 기본 키 순서로 나오고, 정렬 값이 같은 객체끼리도 기본 키 순서를 따릅니다.

`O`는 쿼리가 찾는 객체의 타입입니다. 함수로 쿼리를 줄 때는 컬렉션의 객체 타입이므로, `where`와 `sortBy`는 그 컬렉션의 필드와 필드 타입의 값만 받습니다. 타입 없이 `new Query()`로 만든 쿼리는 어떤 필드와 값이든 받습니다. 이런 필드와 값은 점으로 이은 경로와 마찬가지로 쿼리를 실행할 때 엔진이 검사합니다. 스키마에 맞지 않는 쿼리는 실행할 때 `INVALID_QUERY`로 실패합니다.

시그니처에는 패키지가 내부에서 쓰는 타입 네 가지가 나옵니다. `FieldNames<O>`는 `O`의 필드 이름이고, `DottedPath`는 `.`이 들어간 문자열입니다. `ElementOf<T>`는 필드 타입에서 null을 뺀 타입이며, 목록이면 원소 타입입니다. `Operand<T>`는 `T`이거나, 준비한 쿼리의 매개변수인 [Param](./param.md)입니다.

```ts
import { Query } from 'darudb';
import type { ObjectOf } from 'darudb';

type User = ObjectOf<typeof app.collections.users.fields>;

const query = new Query<User>().where('age', '>=', 18).sortBy('name').limit(20);
const adults = db.read((txn) => txn.collection('users').find(query));
```

기본 키나 인덱스가 있는 필드에 건 조건이 나머지 필터와 `and`로 이어져 있으면, 엔진은 그 조건에 맞는 객체만 읽습니다. 인덱스가 있는 필드 하나로만 정렬하면 그 순서대로 읽다가 개수 제한에서 멈춥니다. 어느 쪽이든 결과는 같습니다. 자세한 설명은 [쿼리](../../guide/queries.md)에 있습니다.

## 메서드

### where

```ts
where<K extends FieldNames<O>>(
  field: K,
  op: '==' | '!=',
  value: Operand<ElementOf<O[K]> | null>
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: Comparison,
  value: Operand<ElementOf<O[K]>>
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: 'between',
  value: readonly [Operand<ElementOf<O[K]>>, Operand<ElementOf<O[K]>>]
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: 'in',
  value: readonly Operand<ElementOf<O[K]>>[]
): Query<O>;
where<K extends FieldNames<O>>(
  field: K,
  op: 'contains' | 'startsWith' | 'endsWith',
  value: Operand<ElementOf<O[K]>>
): Query<O>;
where(
  path: DottedPath,
  op: Comparison | 'contains' | 'startsWith' | 'endsWith',
  value: Operand<QueryValue | null>
): Query<O>;
where(
  path: DottedPath,
  op: 'between',
  value: readonly [Operand<QueryValue>, Operand<QueryValue>]
): Query<O>;
where(path: DottedPath, op: 'in', value: readonly Operand<QueryValue>[]): Query<O>;
where(condition: Condition | ((conditions: Conditions<O>) => Condition)): Query<O>;
```

조건에 맞는 객체만 남깁니다. 앞서 `where`에 준 조건도 모두 지켜야 하며, 부를 때마다 앞의 조건과 `and`로 이어집니다. 조건은 다음 세 형태 중 하나로 줍니다.

- **필드, 연산자, 값.** 연산자는 [Comparison](../../types/node/query-input.md)에 있는 `==`, `!=`, `<`, `<=`, `>`, `>=`와, 쌍 `[low, high]`를 받는 `between`, 배열을 받는 `in`, 그리고 `contains`, `startsWith`, `endsWith`입니다. `== null`은 필드가 null인지, `!= null`은 null이 아닌지 검사합니다.
- **점으로 이은 경로.** 필드 대신 `address.city`나 `team.city`처럼 내장 객체나 링크를 지나는 경로를 씁니다. TypeScript는 경로를 검사하지 않고 엔진이 검사합니다.
- **조건.** [conditions](./conditions.md)로 만든 조건이나, 컬렉션에 맞게 타입이 정해진 조건을 받아 조건 하나를 돌려주는 함수입니다. `or`, `not`, `isNull`, 중첩된 묶음은 이 형태로 씁니다.

이 밖의 연산자나 쌍이 아닌 `between` 값은 `INVALID_QUERY`로 실패합니다.

```ts
db.read((txn) => {
  const users = txn.collection('users');

  users.find((q) => q.where('email', '==', null));
  users.find((q) => q.where('age', 'between', [18, 30]).where('tags', 'contains', 'new'));
  users.find((q) => q.where('address.city', '==', 'Seoul'));
  users.find((q) => q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email'))));
});
```

### sortBy

```ts
sortBy(field: FieldNames<O> | DottedPath, direction?: 'asc' | 'desc'): Query<O>;
```

`field`로 정렬합니다. `direction`이 `'desc'`가 아니면 오름차순이고, 앞서 준 정렬 다음 순위로 적용됩니다. null은 오름차순에서 맨 앞, 내림차순에서 맨 뒤에 오고, 문자열은 UTF-8 바이트 순서로 정렬됩니다. `'asc'`나 `'desc'`가 아닌 방향은 `INVALID_QUERY`로 실패합니다. 목록이나 내장 객체로 정렬하거나, 여러 객체를 가리키는 링크를 지나 정렬해도 마찬가지입니다.

### limit

```ts
limit(count: number): Query<O>;
```

객체를 최대 `count`개까지 돌려줍니다. 다시 부르면 앞의 값을 대신합니다. `count`는 0 이상의 정수여야 하고, 아니면 쿼리를 실행할 때 `INVALID_QUERY`로 실패합니다.

### offset

```ts
offset(count: number): Query<O>;
```

정렬한 결과에서 처음 `count`개를 건너뜁니다. 다시 부르면 앞의 값을 대신하고, `count`의 규칙은 `limit`과 같습니다.

## 쿼리 언어

`find`, `findOne`, `count`, `Database.prepare`는 쿼리를 문자열로도 받습니다. 엔진은 이 문자열을 빌더가 만드는 것과 같은 쿼리로 해석합니다. 컬렉션은 문자열에 들어가지 않고, 쿼리를 실행하는 호출이 정합니다.

```ts
db.read((txn) =>
  txn
    .collection('users')
    .find('age >= $0 AND (name STARTSWITH "A" OR email IS NULL) SORT BY age DESC LIMIT 10', [18])
);
```

- **순서.** 필터를 먼저 쓰고, 그다음 `SORT BY`와 쉼표로 구분한 필드, `LIMIT`, `OFFSET`을 차례로 씁니다. 필드 뒤에 `DESC`를 붙이지 않으면 오름차순입니다. 각 부분은 생략할 수 있습니다.
- **조건.** `field == value`와 `!=`, `<`, `<=`, `>`, `>=`, 그리고 `field BETWEEN a AND b`, `field IN [a, b]`, `field CONTAINS value`, `STARTSWITH`, `ENDSWITH`, `field IS NULL`, `field IS NOT NULL`이 있습니다. 조건은 `AND`, `OR`, `NOT`과 괄호로 엮고, `AND`가 `OR`보다 먼저 묶입니다. 필드 자리에는 점으로 이은 경로도 쓸 수 있습니다.
- **값.** 정수에는 소수점이 없고, 실수에는 소수점이나 지수가 있으며, 둘 다 `-`로 시작할 수 있습니다. 문자열은 큰따옴표로 감싸고, `\"`, `\\`, `\n`, `\t`, `\u{...}` 이스케이프를 씁니다. 그 밖의 값은 `true`, `false`, `null`입니다.
- **매개변수.** `$0`, `$1` 같은 매개변수에는 문자열과 함께 넘긴 값이 순서대로 들어갑니다. 프로그램 바깥에서 들어온 값은 문자열에 끼워 넣지 말고 매개변수로 넘기세요.
- **이름.** 키워드는 대소문자를 가리지 않습니다. `limit`처럼 키워드와 이름이 같은 필드는 경로의 첫머리에서 백틱으로 감쌉니다.
- **오류.** 해석할 수 없는 문자열은 `INVALID_QUERY`로 실패하고, 메시지에 문제가 생긴 글자의 위치가 1부터 센 번호로 나옵니다. 괄호와 `NOT`은 48단계까지만 중첩할 수 있습니다.
