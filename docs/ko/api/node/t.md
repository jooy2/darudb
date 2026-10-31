---
title: t
order: 4
counterpart: /types/rust/type
---

# t

`t`에는 필드 타입을 만드는 빌더가 모여 있고, `collection`과 `t.object`는 이 빌더로 필드를 선언합니다.

```ts
const t: TypeBuilders;
```

빌더는 부를 때마다 새 필드 타입을 돌려주고, 필드 타입의 메서드는 사본을 돌려줍니다. 그래서 한 타입을 여러 필드에 써도 됩니다. 빌더가 돌려주는 타입은 [필드 타입](../../types/node/field-types.md)에서 설명하며, 타입 검사기에 필드의 값 타입을 알려 줍니다. 데이터베이스에서 읽은 객체는 그 타입의 값을 갖고, 쓰는 객체도 그 타입의 값을 넣습니다.

타입이 가질 수 없는 메서드는 두 번 걸러집니다. TypeScript에서는 돌려받은 타입에 그 메서드가 없고, 실행 중에는 필드를 선언하거나 데이터베이스를 열 때 `INVALID_ARGUMENT`가 납니다.

```ts
import { collection, t } from 'darudb';

const users = collection({
  name: t.string(),
  email: t.string().optional().unique(),
  age: t.int().default(0).index(),
  tags: t.list(t.string()).optional().index(),
  team: t.link('teams').optional(),
  address: t.object({ city: t.string(), zip: t.int().optional() }).optional()
});
```

## 메서드

### bool

```ts
bool(): FieldType<boolean>;
```

`true`나 `false`입니다.

### int

```ts
int(): KeyableType<number>;
```

64비트 정수이고 number로 읽힙니다. number가 정확히 담지 못하는 2^53 너머의 값은 쓸 때 거부하고 읽을 때 실패하며, 둘 다 `INVALID_ARGUMENT`입니다. 그런 값이 필요한 필드는 `bigint`로 선언하세요. 소수부가 있는 number도 거부합니다.

### bigint

```ts
bigint(): KeyableType<bigint, bigint | number>;
```

64비트 정수이고, 크기와 관계없이 `bigint`로 읽힙니다. 쓸 때는 `bigint`와 number 모두 받습니다. 파일에는 정수 타입이 하나뿐이어서, `int`로 선언한 필드를 `bigint`로 바꾸거나 그 반대로 바꾸면 읽는 방식만 달라지고 스키마 변경으로 치지 않습니다.

### float

```ts
float(): FieldType<number>;
```

64비트 부동소수점 수입니다. 인덱스가 순서를 유지할 수 있도록 `-0`은 `0`과 같고, NaN은 모두 서로 같으며 양의 무한대보다 뒤에 정렬됩니다.

### string

```ts
string(): KeyableType<string>;
```

UTF-8로 저장하는 텍스트입니다. 짝이 없는 서로게이트가 든 문자열처럼 UTF-8로 담을 수 없는 문자열은 `INVALID_ARGUMENT`로 거부합니다. 문자열은 UTF-8 바이트 순서로 비교하고 정렬하며, 언어별 정렬 규칙은 없습니다. 같음이나 텍스트를 검사하는 조건은 [대소문자를 무시](../../guide/query-builder.md#대소문자-무시하기)할 수 있습니다.

### bytes

```ts
bytes(): KeyableType<Uint8Array>;
```

임의의 바이트이고, `Uint8Array`로 쓰고 읽습니다. `Buffer`도 `Uint8Array`입니다.

### date

```ts
date(): FieldType<Date>;
```

시각이며, `Date`로 쓰고 `Date`로 읽습니다. 파일은 날짜를 마이크로초까지 담고 `Date`는 밀리초까지 담으므로, 다른 언어가 밀리초보다 잘게 쓴 날짜는 내림해서 읽습니다. 잘못된 `Date`는 `INVALID_ARGUMENT`로 거부하며, `Date`가 담지 못하는 1970년에서 약 27만 5천 년 밖의 날짜도 읽을 때 같은 오류로 실패합니다. 쿼리는 이 필드를 `Date`와 비교하고 시간 순서로 정렬합니다. 스키마가 처음으로 날짜를 선언하면 파일이 객체 형식 2가 되고, 그 전 릴리스는 이 파일을 열지 못합니다.

### link

```ts
link(collection: string): LinkType;
```

`collection` 컬렉션에 있는 객체의 기본 키입니다. 그 컬렉션은 같은 스키마에 있어야 하고, 없으면 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다. 필드에는 키만 들어 있고, 없는 객체를 가리켜도 됩니다. 쿼리 경로가 링크를 지나면 `team.city`처럼 링크가 가리키는 객체를 읽습니다. 링크에는 기본값이 없습니다.

### list

```ts
list<T, I>(
  element: FieldType<T, 'required', I> | KeyableType<T, I>
): FieldType<T[], 'required', I[]>;
list(element: LinkType): FieldType<Key[]>;
```

`element` 타입 값의 목록입니다. 원소 타입은 스칼라 타입이나 링크입니다. 링크의 목록은 여러 객체를 가리키는 링크가 됩니다. 목록에는 null이 들어갈 수 없고, 빈 목록은 null이 아닙니다. 원소 타입에 `optional`, `default`, `index`, `unique`를 붙였거나 목록의 목록이면 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다. `t.object(...)`의 목록은 내장 객체의 목록인 [`EmbeddedListType`](../../types/node/field-types.md#embeddedlisttype)입니다. `stops.city`처럼 이 목록을 지나는 경로는 원소마다 그 필드를 검사하며, 인덱스와 정렬이 없고 기본값은 `[]`뿐입니다. 인덱스는 `t.list(t.string()).index()`처럼 목록 자체에 두며, 원소마다 항목이 생깁니다. 목록에 건 조건은 원소 하나라도 맞으면 참입니다.

### object

```ts
object<F extends Fields>(fields: F): EmbeddedType<EmbeddedOf<F>, EmbeddedInputOf<F>>;
```

자기 필드를 가진 내장 객체입니다. 내장 객체는 자신을 담은 객체 안에 저장되고, 그 객체와 함께 읽고 씁니다. 키와 기본값과 인덱스가 없습니다. 내장 객체의 필드는 저마다 타입과 메서드를 따르되, 키가 되거나 인덱스를 가질 수는 없습니다. 쿼리에서는 `address.city` 같은 경로로 닿습니다. `fields`에 `t`로 만들지 않은 값이 있으면 곧바로 `INVALID_ARGUMENT`를 던집니다.

## 필드 타입의 메서드

아래 메서드는 위 빌더가 돌려준 필드 타입에 있습니다. 어떤 메서드를 갖는지는 타입마다 다르며, [필드 타입](../../types/node/field-types.md)에 정리돼 있습니다.

### optional

```ts
optional(): FieldType<T, 'optional', I>;
```

필드가 null일 수 있고, 빠지면 null이 됩니다. 기본 키는 선택 필드가 될 수 없습니다.

### default

```ts
default(value: I): FieldType<T, 'default', I>;
```

필드는 필수이고, 빠지면 `value`가 들어갑니다. 기본값은 객체에 함께 기록되므로, 나중에 기본값을 바꿔도 이미 쓴 객체는 그대로입니다. 필드가 생기기 전에 쓴 객체만 바뀐 기본값을 읽습니다. 다음 스키마 버전에서 기본값을 바꿀 수는 있지만, 필드가 필수로 남아 있는 한 없앨 수는 없습니다. 링크, 내장 객체, 기본 키에는 기본값이 없습니다.

### index

```ts
index(): FieldType<T, M, I>;
```

그 필드로 찾는 쿼리가 모든 객체를 읽는 대신 인덱스를 읽게 합니다. 내장 객체 안의 필드에는 [collection](./collection.md#여러-필드에-두는-인덱스)의 `indexes` 옵션에서 경로로 인덱스를 둡니다.

### unique

```ts
unique(): FieldType<T, M, I>;
```

값이 같은 객체 둘을 `DUPLICATE_KEY`로 거부하는 인덱스입니다. null은 몇 개가 있어도 됩니다. `index` 없이 `unique`만으로 인덱스가 생깁니다.

### cascade

```ts
cascade(): LinkType<M>;
```

링크가 가리키는 객체를 지우면 그 링크를 가진 객체도 같은 트랜잭션에서 지워지고, 사슬을 따라 계속 이어집니다. 링크 타입에만 있고, 컬렉션 자신의 객체 하나를 가리키는 링크에만 쓸 수 있습니다. `t.list`나 `t.object` 안에 쓰면 데이터베이스를 열 때 `INVALID_ARGUMENT`로 실패합니다. 필드에 인덱스도 두세요. 인덱스가 없으면 필드가 가리킬 수 있는 객체를 지울 때마다 컬렉션 전체를 읽습니다. 연쇄 삭제 링크가 있는 파일은 객체 형식 2입니다. [규칙](../../guide/objects.md#링크로-이어진-객체-함께-지우기)은 가이드에 있습니다.

### primaryKey

```ts
primaryKey(): KeyType<T, I>;
```

필드를 컬렉션의 기본 키로 삼습니다. `int`, `bigint`, `string`, `bytes`가 돌려준 타입에만 있고, 다른 메서드를 거친 타입에는 없습니다. 키 타입에는 `index`와 `unique`만 남습니다. 기본 키는 [collection](./collection.md#기본-키)에서 설명합니다.
