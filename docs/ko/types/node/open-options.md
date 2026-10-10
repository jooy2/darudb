---
title: OpenOptions
order: 1
group: database
counterpart: /api/rust/open-options
pageClass: reference-page
---

# OpenOptions

`OpenOptions`는 `Database.open`이 경로와 함께 받는 옵션으로, 파일 생성 여부, 페이지 크기, 다른 프로세스를 기다리는 시간, 페이지 캐시, 예전 파일 형식을 올릴지, 스키마와 마이그레이션, 암호화한 파일의 키나 비밀번호를 정합니다.

```ts
interface OpenOptions<S = Schema>
```

필드는 모두 생략할 수 있습니다. 옵션 없이 `Database.open(path)`를 부르면 스키마가 없고 암호화하지 않은 데이터베이스를 만들거나 엽니다. `S`는 [스키마](../../api/node/schema.md)의 타입입니다. [`Database.open`](../../api/node/database.md)이 `schema` 옵션에서 추론해 마이그레이션 함수에 넘기며, 타입 정보만 담습니다.

몇몇 옵션은 `Database` 하나가 아니라 파일에 딸립니다. 같은 프로세스에서 이미 연 파일을 다시 열면 같은 데이터베이스의 핸들이 하나 더 생기는데, `busyTimeout`, `cacheSize`, `upgradeFormat`, `passwordHashing`은 처음 연 핸들의 값을 그대로 씁니다. `pageSize`는 파일을 만들 때만 쓰입니다. `schema`는 핸들마다 열 때 준 것을 따로 쓰고, 암호화한 파일은 열 때마다 `key`나 `password`가 있어야 합니다.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, { notes: collection({ text: t.string() }) });

const db = Database.open('notes.darudb', {
  schema: app,
  busyTimeout: 2000,
  cacheSize: 8 * 1024 * 1024
});
```

## 필드

| 필드 | 타입 | 기본값 | 정하는 것 |
| --- | --- | --- | --- |
| [`create`](#create) | `boolean` | `true` | 없는 파일을 만들지 여부 |
| [`pageSize`](#pagesize) | `number` | `4096` | 새 파일의 페이지 크기, 바이트 단위 |
| [`busyTimeout`](#busytimeout) | `number` | `5000` | 다른 쓰기를 기다리는 시간, 밀리초 단위 |
| [`cacheSize`](#cachesize) | `number` | 32 MiB | 페이지 캐시가 쓸 수 있는 메모리, 바이트 단위 |
| [`upgradeFormat`](#upgradeformat) | `boolean` | `true` | 예전 형식의 파일을 올릴지 여부 |
| [`schema`](#schema) | `S` | 없음 | 데이터베이스가 담는 컬렉션 |
| [`migrations`](#migrations) | `Migration<S>[]` | 없음 | 예전 스키마 버전을 이 버전으로 바꾸는 방법 |
| [`key`](#key) | `Uint8Array` | 없음 | 암호화한 파일의 32바이트 키 |
| [`password`](#password) | `string \| Uint8Array` | 없음 | 암호화한 파일의 비밀번호 |
| [`passwordHashing`](#passwordhashing) | `PasswordHashing` | 19 MiB, 2, 1 | 비밀번호 해시에 드는 작업량 |

### create

```ts
create?: boolean;
```

경로에 아무것도 없을 때 데이터베이스를 새로 만들지 정합니다. 기본값은 `true`입니다. `false`이면 아무것도 없는 경로를 열 때 `NOT_FOUND`로 실패합니다. 어느 쪽이든 이미 있는 파일을 덮어쓰지는 않습니다. 새 데이터베이스는 임시 파일에 먼저 쓴 다음 제자리로 옮기므로, 경로에는 아무것도 없거나 온전한 데이터베이스가 있습니다.

### pageSize

```ts
pageSize?: number;
```

새 데이터베이스의 페이지 크기를 바이트 단위로 정합니다. 4096부터 65536 사이의 2의 거듭제곱이어야 하고, 기본값은 4096입니다. 다른 값은 파일이 이미 있어도 `INVALID_ARGUMENT`로 실패합니다. 이미 있는 파일은 헤더에 기록된 페이지 크기를 그대로 쓰고, 그 값은 [`Database.pageSize`](../../api/node/database.md)로 읽습니다. 페이지가 크면 훑기와 세기는 빨라집니다. 대신 작은 커밋과, 페이지 캐시에 다 들어가지 않는 파일에서 하는 조회는 느려집니다. 둘 다 페이지를 통째로 쓰거나 읽기 때문입니다.

### busyTimeout

```ts
busyTimeout?: number;
```

데이터베이스를 열 때와 쓰기 트랜잭션이 다른 프로세스의 쓰기를 기다리는 시간을 밀리초 단위로 정합니다. 이 시간이 지나면 `BUSY`로 실패하고, 기본값은 5000입니다. 프로세스들이 어떻게 차례로 쓰는지는 [여러 프로세스](../../guide/processes.md)에 있습니다.

### cacheSize

```ts
cacheSize?: number;
```

페이지 캐시가 쓸 수 있는 메모리를 바이트 단위로 정합니다. 0 이상의 정수여야 하고, 기본값은 32MiB입니다. 다른 값은 `INVALID_ARGUMENT`로 실패합니다. 캐시는 파일에서 읽어 검사와 복호화까지 마친 페이지를 담아 두므로, 같은 페이지를 다시 읽을 때는 읽기도 검사도 다시 하지 않습니다. 이 값과 관계없이 페이지를 적어도 16개는 담을 수 있고, 페이지를 읽는 만큼만 차므로 캐시보다 작은 데이터베이스가 캐시를 다 차지하는 일은 없습니다. 캐시에 다 들어가지 않는 데이터베이스는 캐시를 키우면 빨라지고, 메모리가 빠듯한 프로세스는 캐시를 줄여도 됩니다.

### upgradeFormat

```ts
upgradeFormat?: boolean;
```

예전 형식 버전의 파일을 열 때 이 패키지가 쓰는 가장 새 버전인 [`FORMAT_VERSION`](./constants.md)으로 올릴지 정합니다. 기본값은 `true`입니다.

- **드는 비용.** 파일에 무엇이 있든 헤더만 다시 쓰고, 동기화를 세 번 합니다. 전에 쓴 리프는 쓰기가 바꿀 때까지 배치를 그대로 두다가, 바뀔 때 더 작은 새 배치로 쓰입니다. [`compact`](../../api/node/database.md#compact)는 새 배치로 공간이 줄어드는 트리를 다시 씁니다.
- **올리는 때.** 다른 프로세스가 파일을 열고 있지 않을 때, 그때 파일을 여는 프로세스가 올립니다. 다른 프로세스가 열고 있으면, 혼자 여는 다음 번까지 파일을 그대로 둡니다.
- **되돌아가기.** 예전 버전만 아는 릴리스는 올린 파일을 거부합니다. 그런 릴리스로 되돌아갈 수 있는 애플리케이션은 이 값을 `false`로 두고, 되돌아갈 일이 없어지면 [`upgradeFormat`](../../api/node/database.md#upgradeformat)으로 버전을 올립니다. `false`면 새 데이터베이스는 모든 릴리스가 읽는 형식 버전 5로 만듭니다.

### schema

```ts
schema?: S;
```

데이터베이스에 둘 컬렉션으로, [`schema`](../../api/node/schema.md)로 선언합니다. 처음 열 때 스키마를 파일에 저장하고, 그 뒤로는 열 때마다 저장된 스키마와 비교합니다.

- 버전이 같은데 스키마가 다르면 `SCHEMA_MISMATCH`로 실패합니다.
- 파일의 버전이 더 높으면 `SCHEMA_TOO_NEW`로 실패합니다.
- 파일의 버전이 더 낮으면 `open`이 반환하기 전에 `migrations`로 마이그레이션합니다.

스키마에 없는 컬렉션을 가리키는 링크처럼 엔진이 저장할 수 없는 선언은 `INVALID_ARGUMENT`로 실패합니다. 스키마가 없으면 컬렉션도 없어서 `schemaVersion`은 `null`이고, `collection`은 `INVALID_ARGUMENT`로 실패합니다. 스키마가 지키는 규칙은 [컬렉션과 객체](../../guide/objects.md)에 있습니다.

### migrations

```ts
migrations?: Migration<S>[];
```

예전 스키마 버전을 담은 파일을 이 버전으로 올리는 방법입니다. 엔진이 알아서 하는 것 말고도 할 일이 있는 버전 단계마다 [Migration](./migration.md)을 하나씩 둡니다. 마이그레이션은 파일에 필요할 때만, 버전 순서대로, 쓰기 트랜잭션 하나 안에서 실행됩니다. `schema` 없이 준 마이그레이션이나 배열이 아닌 값은 `INVALID_ARGUMENT`로 실패합니다.

### key

```ts
key?: Uint8Array;
```

새 데이터베이스를 암호화하거나 암호화한 데이터베이스를 여는 32바이트 키입니다. 길이가 다르거나 `Uint8Array`가 아니면 `INVALID_ARGUMENT`로 실패합니다.

- 암호화한 데이터베이스를 키나 비밀번호 없이 열면 `KEY_REQUIRED`, 다른 키로 열면 `WRONG_KEY`로 실패합니다.
- 암호화하지 않은 데이터베이스를 키로 열면 `INVALID_ARGUMENT`로 실패합니다. 그 파일이 암호화되는 일은 없으며, 암호화하려면 새 파일을 만들어야 합니다.
- 키는 운영체제 키스토어처럼 잃어버리지 않을 곳에 두세요. 키가 없으면 데이터를 읽을 수 없습니다.

패키지는 `open`을 부르는 순간 키를 복사하고, 엔진이 제 사본을 가져가면 그 복사본을 지웁니다. 그래서 넘긴 버퍼는 호출이 반환되자마자 `fill(0)`으로 지워도 됩니다. 키가 파일을 어떻게 지키는지는 [암호화](../../guide/encryption.md)에 있습니다.

### password

```ts
password?: string | Uint8Array;
```

새 데이터베이스를 암호화하거나 암호화한 데이터베이스를 여는 비밀번호입니다. `passwordHashing`이 정한 비용으로 Argon2id 해시를 거쳐 파일을 지키는 키가 되고, 나머지는 `key`와 같습니다. 문자열은 UTF-8 바이트로 씁니다. 빈 비밀번호를 주거나 `key`와 `password`를 함께 주면 `INVALID_ARGUMENT`로 실패합니다. `Uint8Array`는 `open`이 반환한 뒤 지울 수 있지만, 문자열은 지울 수 없어서 가비지 컬렉터가 거둘 때까지 메모리에 남습니다.

### passwordHashing

```ts
passwordHashing?: PasswordHashing;
```

비밀번호로 새 데이터베이스를 암호화하거나 `setPassword`로 비밀번호를 바꿀 때 해시에 드는 비용입니다. 기본값은 19456KiB, 반복 2회, 병렬도 1입니다. 이미 있는 파일을 열 때는 이 값과 관계없이 파일에 기록된 비용을 씁니다. 각 값의 범위는 [PasswordHashing](./password-hashing.md)에 있습니다.

## AsyncOpenOptions

```ts
interface AsyncOpenOptions<S = Schema> extends Omit<OpenOptions<S>, 'migrations'> {
  migrations?: AsyncMigration<S>[];
}
```

`Database.openAsync`가 받는 옵션입니다. `OpenOptions`와 같지만 `migrations`에 [AsyncMigration](./migration.md#asyncmigration)을 담습니다. 그 `run`은 비동기여도 되고, 비동기 API를 받습니다. `openAsync`는 일이 스레드 풀로 넘어가기 전, 호출하는 순간에 키나 비밀번호를 복사합니다. 그래서 버퍼는 promise가 끝날 때를 기다리지 않고 호출이 반환되자마자 지워도 됩니다.
