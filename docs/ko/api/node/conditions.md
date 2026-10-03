---
title: conditions
order: 10
counterpart: /api/rust/filter
---

# conditions

`conditions`는 쿼리 필터를 이루는 조건을 만들고, 조건을 `and`, `or`, `not`으로 엮습니다.

```ts
const conditions: Conditions;
```

`Query.where`는 조건을 받거나, 컬렉션에 맞게 타입이 정해진 같은 조건을 받는 함수를 받습니다. `q.where((c) => c.or(c.eq('name', 'Alice'), c.isNull('email')))`처럼 씁니다. 이 함수에서 `c`는 `Conditions<O>`이고, 메서드는 컬렉션의 필드와 그 필드 타입의 값만 받습니다. 내보낸 `conditions`는 특정 컬렉션에 묶이지 않은 `Conditions`로, 쿼리 밖에서 조건을 만들 때 씁니다. 어떤 필드와 값이든 받고, 쿼리를 실행할 때 엔진이 검사합니다.

메서드는 모두 `Condition`을 돌려줍니다.

```ts
interface Condition
```

조건은 동결된 객체여서 여러 쿼리에 넣어도 됩니다. 하나뿐인 속성 `node`는 패키지 내부용이므로 쓰지 않습니다. 조건이 와야 할 자리에 다른 값을 주면 `INVALID_QUERY`로 실패합니다.

시그니처에 나오는 `FieldNames<O>`, `DottedPath`, `ElementOf<T>`, `Operand<T>`는 [Query](./query.md)에서 설명합니다.

- **경로.** 필드는 이름으로 가리키고, 내장 객체나 링크를 지날 때는 `.`으로 잇습니다. `address.city`처럼 쓰고, `team.city`처럼 쓰면 링크가 가리키는 객체를 검사합니다. 가리키는 객체가 없으면 null로 읽습니다. 경로에는 이름이 32개까지 들어갈 수 있고, 점으로 이은 경로는 TypeScript가 검사하지 않습니다.
- **목록.** 목록에 건 조건은 원소 하나라도 맞으면 참입니다. 목록에 `contains`를 쓰면 그 원소가 있는지 봅니다. 빈 목록에는 원소가 없으므로 `isNotNull`만 참입니다.
- **null.** null인 필드에 건 조건은 `isNull`을 빼고 모두 거짓입니다. `eq(field, null)`은 `isNull`과, `ne(field, null)`은 `isNotNull`과 같습니다.
- **타입.** 값은 필드의 타입과 맞아야 합니다. 정수 필드는 정수와만 비교하고 실수와는 비교하지 않으며, 실수 필드는 어떤 number와도 비교합니다. 링크는 대상 컬렉션의 키와 비교합니다. 이를 어기거나 컬렉션에 없는 필드를 쓴 쿼리는 `INVALID_QUERY`로 실패합니다.
- **중첩.** 필터는 24단계까지만 중첩할 수 있습니다. `and` 안의 `and`와 `or` 안의 `or`는 한 단계로 합쳐지므로, `not`과 번갈아 나오는 묶음만 단계로 셉니다.

```ts
import { conditions } from 'darudb';

const young = conditions.and(conditions.ge('age', 18), conditions.lt('age', 30));
const found = db.read((txn) =>
  txn.collection('users').find((q) => q.where(conditions.or(young, conditions.isNull('email'))))
);
```

## 메서드

### eq

```ts
eq<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]> | null>): Condition;
eq(path: DottedPath, value: Operand<QueryValue | null>): Condition;
```

필드가 `value`와 같습니다. `null`을 주면 필드가 null인지 봅니다.

### ne

```ts
ne<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]> | null>): Condition;
ne(path: DottedPath, value: Operand<QueryValue | null>): Condition;
```

필드가 `value`와 다릅니다. `null`을 주면 필드가 null이 아닌지 봅니다.

### lt

```ts
lt<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
lt(path: DottedPath, value: Operand<QueryValue>): Condition;
```

필드가 `value`보다 작습니다.

### le

```ts
le<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
le(path: DottedPath, value: Operand<QueryValue>): Condition;
```

필드가 `value`보다 작거나 같습니다.

### gt

```ts
gt<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
gt(path: DottedPath, value: Operand<QueryValue>): Condition;
```

필드가 `value`보다 큽니다.

### ge

```ts
ge<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
ge(path: DottedPath, value: Operand<QueryValue>): Condition;
```

필드가 `value`보다 크거나 같습니다.

### between

```ts
between<K extends FieldNames<O>>(
  field: K,
  low: Operand<ElementOf<O[K]>>,
  high: Operand<ElementOf<O[K]>>
): Condition;
between(path: DottedPath, low: Operand<QueryValue>, high: Operand<QueryValue>): Condition;
```

필드가 `low` 이상 `high` 이하입니다.

### in

```ts
in<K extends FieldNames<O>>(field: K, values: readonly Operand<ElementOf<O[K]>>[]): Condition;
in(path: DottedPath, values: readonly Operand<QueryValue>[]): Condition;
```

필드가 `values` 중 하나와 같습니다. 배열이 아닌 값을 주면 `INVALID_QUERY`로 실패합니다.

### contains

```ts
contains<K extends FieldNames<O>>(field: K, value: Operand<ElementOf<O[K]>>): Condition;
contains(path: DottedPath, value: Operand<QueryValue>): Condition;
```

문자열 필드가 `value`를 포함하거나, 목록에 `value`라는 원소가 있습니다.

### startsWith

```ts
startsWith<K extends FieldNames<O>>(field: K, value: Operand<string>): Condition;
startsWith(path: DottedPath, value: Operand<string>): Condition;
```

문자열 필드가 `value`로 시작합니다. 문자열 목록이면 원소 하나라도 `value`로 시작할 때 참입니다.

### endsWith

```ts
endsWith<K extends FieldNames<O>>(field: K, value: Operand<string>): Condition;
endsWith(path: DottedPath, value: Operand<string>): Condition;
```

문자열 필드가 `value`로 끝납니다. 문자열 목록이면 원소 하나라도 `value`로 끝날 때 참입니다.

### isNull

```ts
isNull(field: FieldNames<O> | DottedPath): Condition;
```

필드가 null입니다. 목록은 목록 자체가 null일 때만 null이고, 비어 있다고 null이 되지는 않습니다.

### isNotNull

```ts
isNotNull(field: FieldNames<O> | DottedPath): Condition;
```

필드가 null이 아닙니다.

### and

```ts
and(...conditions: Condition[]): Condition;
```

`conditions`가 모두 참입니다.

### or

```ts
or(...conditions: Condition[]): Condition;
```

`conditions` 중 하나 이상이 참입니다.

### not

```ts
not(condition: Condition): Condition;
```

`condition`이 거짓입니다.
