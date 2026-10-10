---
title: Database
order: 1
---

# Database

`Database`는 Node.js에서 연 DaruDB 파일을 나타내며, 프로그램은 이 객체로 트랜잭션을 실행하고 쿼리를 준비하고 파일 도구를 씁니다.

```ts
interface Database<S extends Schema<any> = Schema>
```

생성자는 없습니다. `Database.open`이나 `Database.openAsync`가 만들어 돌려주고, `new Database()`를 부르면 예외가 납니다. 아래 정적 메서드는 `Database` 값에 있으며, 이 값의 타입은 `DatabaseOpener`입니다. `S`는 데이터베이스를 열 때 준 [스키마](./schema.md)입니다. 컬렉션 이름과 객체의 타입을 모든 트랜잭션에 전할 뿐, 실행할 때는 아무 역할도 하지 않습니다. 스키마 없이 연 데이터베이스에는 컬렉션이 없습니다.

`close`나 `closeAsync`를 부르기 전까지 쓸 수 있습니다. 닫은 뒤에는 `path`와 `isOpen`만 읽을 수 있고, `close`와 `closeAsync`는 아무 일도 하지 않으며, 나머지는 모두 `CLOSED`를 던집니다. 프로세스가 이미 연 파일을 다시 열면 같은 데이터베이스의 핸들이 하나 더 생기고, 페이지 캐시도 함께 씁니다. 스키마는 핸들마다 열 때 받은 것을 계속 씁니다.

파일에 접근하는 메서드에는 이름이 `Async`로 끝나는 짝이 있습니다. 짝 메서드는 엔진의 일을 libuv 스레드 풀에서 하고 promise를 돌려주므로, 이벤트 루프가 디스크나 다른 프로세스의 쓰기를 기다리지 않습니다. 두 방식이 한 파일을 어떻게 함께 쓰는지는 [비동기 API](../../guide/async.md)에서 설명합니다.

## 정적 메서드

### open

```ts
open<S extends Schema<any>>(path: string, options: OpenOptions<S> & { schema: S }): Database<S>;
open(path: string, options?: OpenOptions<never>): Database;
```

`path`의 데이터베이스를 엽니다. 그 자리에 아무것도 없으면 새로 만들고, 스키마를 저장하거나 비교하거나 마이그레이션합니다. 옵션은 [OpenOptions](../../types/node/open-options.md)에 있습니다. 마이그레이션 함수는 이 호출 안에서 동기로 실행되며 [Migrating](./migrating.md)을 받습니다. 함수가 예외를 던지면 파일은 예전 스키마와 데이터를 그대로 유지하고, `open`도 같은 예외를 던집니다.

- `NOT_FOUND`: `path`에 아무것도 없는데 `create`가 `false`입니다.
- `NOT_A_DATABASE`: DaruDB 데이터베이스가 아닌 파일입니다.
- `KEY_REQUIRED`, `WRONG_KEY`: 암호화된 파일인데 옵션에 키나 비밀번호가 없거나, 있어도 틀렸습니다.
- `SCHEMA_MISMATCH`: 파일에 버전은 같지만 내용이 다른 스키마가 있습니다. `SCHEMA_TOO_NEW`: 파일의 스키마 버전이 더 높습니다.
- `BUSY`: 다른 프로세스가 `busyTimeout`보다 오래 파일을 붙잡고 있거나, 되살리기가 파일을 쓰고 있습니다.
- `INVALID_ARGUMENT`: 쓸 수 없는 옵션이 있습니다. 4096부터 65536 사이의 2의 거듭제곱이 아닌 페이지 크기, 32바이트가 아닌 키, 키와 비밀번호를 함께 준 경우, 엔진이 저장할 수 없는 스키마가 그 예입니다.

```ts
import { collection, Database, schema, t } from 'darudb';

const app = schema(1, { users: collection({ name: t.string() }) });
const db = Database.open('app.darudb', { schema: app });
```

### openAsync

```ts
openAsync<S extends Schema<any>>(
  path: string,
  options: AsyncOpenOptions<S> & { schema: S }
): Promise<Database<S>>;
openAsync(path: string, options?: AsyncOpenOptions<never>): Promise<Database>;
```

스레드 풀에서 실행하는 `open`입니다. 마이그레이션 함수는 비동기여도 되고 [AsyncMigrating](./migrating.md#asyncmigrating)을 받습니다. 한 단계는 함수의 promise와, 함수가 부른 작업이 모두 끝나야 마무리됩니다. 실패하는 경우는 `open`과 같고, 예외 대신 promise가 거부됩니다.

### salvage

```ts
salvage(from: string, into: string, options?: SalvageOptions): SalvageReport;
```

손상된 `from`의 데이터베이스에서 건질 수 있는 것을 `into`의 새 데이터베이스로 옮기고, 무엇을 건졌고 무엇을 건지지 못했는지 보고합니다. 파일을 여는 대신 페이지 단위로 읽으므로 열리지 않는 파일에도 쓸 수 있습니다. 파일에 기록된 가장 새 커밋에서 시작하고, 그 커밋에서 읽지 못한 부분은 같은 페이지의 옛 버전에서 가져옵니다. 인덱스는 모두 다시 만들므로 새 파일은 무결성 검사를 통과합니다. 암호화한 파일은 [SalvageOptions](../../types/node/salvage-options.md)에 키나 비밀번호를 넣어야 하고, 새 파일도 그것으로 열립니다. 결과는 [SalvageReport](../../types/node/salvage-report.md)에, 언제 쓰는지는 [도구](../../guide/tools.md)에 있습니다.

- `BUSY`: 이 프로세스나 다른 프로세스가 파일을 열고 있습니다. 되살리는 동안 파일을 열어도 `BUSY`로 실패합니다.
- `INVALID_ARGUMENT`: `into`에 이미 무언가 있거나 경로가 비었습니다. 되살리기는 파일을 덮어쓰지 않습니다.
- `NOT_FOUND`: `from`에 아무것도 없습니다.
- `KEY_REQUIRED`: 암호화한 파일인데 옵션에 키나 비밀번호가 없습니다.

### salvageAsync

```ts
salvageAsync(from: string, into: string, options?: SalvageOptions): Promise<SalvageReport>;
```

스레드 풀에서 실행하는 `salvage`입니다.

## 속성

### path

```ts
readonly path: string;
```

데이터베이스를 열 때 준 경로 그대로입니다. `close` 뒤에도 읽을 수 있습니다.

### isOpen

```ts
readonly isOpen: boolean;
```

데이터베이스가 열려 있는지 나타냅니다. `close`나 `closeAsync`를 부르는 즉시 `false`가 됩니다.

### pageSize

```ts
readonly pageSize: number;
```

파일의 페이지 크기이며 단위는 바이트입니다. 파일은 만들 때 정한 페이지 크기를 계속 씁니다.

### formatVersion

```ts
readonly formatVersion: number;
```

파일에 기록된 파일 형식 버전입니다. 이 패키지가 쓰는 6([FORMAT_VERSION](../../types/node/constants.md))이거나, 열 때 올리지 않은 파일이라면 5입니다([`upgradeFormat`](../../types/node/open-options.md#upgradeformat)).

### isEncrypted

```ts
readonly isEncrypted: boolean;
```

파일이 암호화돼 있는지 나타냅니다.

### schemaVersion

```ts
readonly schemaVersion: number | null;
```

이 핸들이 파일을 열 때 파일에 있던 스키마의 버전입니다. 스키마 없이 열었으면 `null`입니다.

## 메서드

### prepare

```ts
prepare<N extends NameOf<S>>(
  collection: N,
  query: QueryInput<ObjectOf<FieldsOf<S, N>>> | string
): Prepared<ObjectOf<FieldsOf<S, N>>>;
```

`collection` 컬렉션에서 실행할 쿼리를 한 번 준비해 두고, 실행할 때마다 매개변수 값만 넘기게 합니다. 쿼리는 `$0`, `$1` 같은 매개변수를 쓴 쿼리 언어 문자열이거나, 값 자리에 [param](./param.md)을 넣어 만든 쿼리입니다. `N`은 스키마에 있는 컬렉션 이름 중 하나이고, `FieldsOf<S, N>`은 그 컬렉션의 필드입니다. 준비한 쿼리인 [Prepared](../../types/node/prepared.md)는 데이터베이스나 트랜잭션에 묶이지 않으므로, 그 컬렉션이라면 동기든 비동기든 어느 트랜잭션에서나 실행할 수 있습니다.

스키마에 없는 컬렉션이면 `INVALID_ARGUMENT`로, 해석할 수 없는 문자열이면 `INVALID_QUERY`로 실패합니다.

```ts
import { param } from 'darudb';

const byEmail = db.prepare('users', (q) => q.where('email', '==', param(0)));
const alice = db.read((txn) => txn.collection('users').findOne(byEmail, ['alice@example.com']));
```

### read

```ts
read<R>(fn: (txn: ReadTransaction<S>) => R): R;
```

`fn`을 [읽기 트랜잭션](./read-transaction.md) 안에서 실행하고 `fn`이 반환한 값을 돌려줍니다. 트랜잭션은 `fn`이 도는 동안 커밋 하나를 보며, 시작할 때 쓰기를 기다리지 않습니다. `fn`은 동기 함수여야 합니다. promise를 돌려주면 `INVALID_ARGUMENT`로 거부합니다.

### readAsync

```ts
readAsync<R>(fn: (txn: AsyncReadTransaction<S>) => R): Promise<Awaited<R>>;
```

비동기 API로 실행하는 `read`입니다. `fn`은 비동기여도 되고, 컬렉션의 작업은 스레드 풀에서 실행되며, 트랜잭션은 `fn`이 끝날 때까지 커밋 하나를 봅니다. 트랜잭션은 부른 스레드에서 시작합니다. 읽기를 시작할 때는 쓰기를 기다리지 않고, 스레드 풀을 한 번 오가는 것보다 비용이 적기 때문입니다.

### write

```ts
write<R>(fn: (txn: WriteTransaction<S>) => R, options?: WriteOptions): R;
```

`fn`을 [쓰기 트랜잭션](./write-transaction.md) 안에서 실행합니다. `fn`이 반환하면 커밋하고, 예외를 던지면 취소하며, `fn`이 반환한 값을 돌려줍니다. 기본으로는 커밋이 디스크에 기록된 뒤 반환하고, `{ durability: 'deferred' }`를 주면 디스크를 기다리지 않습니다. 자세한 내용은 [WriteOptions](../../types/node/write-options.md)에 있습니다.

- `BUSY`: 다른 프로세스의 쓰기가 `busyTimeout`보다 오래 파일을 붙잡고 있었습니다.
- `INVALID_ARGUMENT`: `fn`이 promise를 돌려줘서 트랜잭션을 취소했습니다.
- `INVALID_ARGUMENT`: 같은 파일에 대한 쓰기 트랜잭션의 함수 안에서 불렀거나, 이 프로세스의 비동기 쓰기가 파일을 쥐고 있는 동안 불렀습니다. 쓰기 트랜잭션은 겹칠 수 없고, 여기서 기다리면 다른 쓰기가 끝나는 데 필요한 스레드를 막게 됩니다.
- `SYNC_FAILED`: 디스크 동기화가 실패해서 커밋이 반영됐는지 알 수 없습니다. 데이터베이스를 닫고 다시 열어야 합니다.

### writeAsync

```ts
writeAsync<R>(
  fn: (txn: AsyncWriteTransaction<S>) => R,
  options?: WriteOptions
): Promise<Awaited<R>>;
```

비동기 API로 실행하는 `write`입니다. `fn`은 비동기여도 됩니다. 트랜잭션은 `fn`의 promise가 이행되고 `fn`이 부른 작업이 모두 끝나면 커밋하고, promise가 거부되면 취소합니다. 이 프로세스가 한 파일에 하는 비동기 쓰기는 `Database` 객체가 몇 개든 JavaScript 안에서 차례를 기다렸다가 하나씩 스레드 풀로 가므로, 기다리는 쓰기는 스레드를 잡지 않습니다. 같은 파일에 대한 비동기 쓰기의 함수 안에서 부르면 자기 자신을 기다리는 대신 `INVALID_ARGUMENT`로 거부됩니다.

### check

```ts
check(): CheckReport;
```

게시된 커밋을 빠짐없이 검사합니다. 모든 페이지가 검사값과 맞는지, 키가 순서대로 있는지, 항목 수가 맞는지, 모든 페이지가 사용 중이거나 비었거나 보류 중인 상태 중 정확히 하나인지, 모든 객체가 인덱스와 맞는지 봅니다. 예외를 던지지 않고 찾은 문제를 모두 [CheckReport](../../types/node/check-report.md)에 담아 돌려주며, 다른 핸들과 프로세스가 쓰는 동안에도 읽습니다.

### checkAsync

```ts
checkAsync(): Promise<CheckReport>;
```

스레드 풀에서 실행하는 `check`입니다. 쓰기를 기다리는 일이 없습니다.

### backup

```ts
backup(path: string, options?: BackupOptions): BackupReport;
```

게시된 커밋을 `path`의 새 파일로 복사합니다. 그동안 다른 핸들과 프로세스는 계속 쓸 수 있습니다. 사본에는 빈 공간이 없고, 페이지 크기는 원본과 같으며, 같은 키나 비밀번호로 열립니다. 다만 [`options`](../../types/node/backup-options.md)에 `key`나 `password`를 주면 사본은 새 데이터 키로 암호화됩니다. 이미 있는 파일은 덮어쓰지 않습니다. 경로에 파일이 있거나 경로가 비었으면 `INVALID_ARGUMENT`로 실패합니다. 결과는 [BackupReport](../../types/node/backup-report.md)에 있습니다.

### backupAsync

```ts
backupAsync(path: string, options?: BackupOptions): Promise<BackupReport>;
```

스레드 풀에서 실행하는 `backup`입니다.

### compact

```ts
compact(): CompactReport;
```

파일을 그 자리에서 줄입니다. 삽입으로 페이지가 덜 찬 트리를 꽉 채워 다시 쓰고, 파일 끝쪽 페이지를 앞쪽 빈 페이지로 옮긴 뒤, 비게 된 끝을 파일 시스템에 돌려줍니다. 자체 쓰기 트랜잭션으로 일하므로 다른 쓰기처럼 쓰기 잠금을 기다립니다. `write`와 마찬가지로, 같은 파일에 대한 쓰기 트랜잭션의 함수 안에서나 이 프로세스의 비동기 쓰기가 파일을 쥐고 있는 동안에는 `INVALID_ARGUMENT`로 거부됩니다. 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 옮기지 않습니다. 결과는 [CompactReport](../../types/node/compact-report.md)에 있습니다.

### compactAsync

```ts
compactAsync(): Promise<CompactReport>;
```

스레드 풀에서 실행하는 `compact`입니다. 이 프로세스가 그 파일에 먼저 시작한 쓰기가 끝난 뒤에 실행됩니다.

### upgradeFormat

```ts
upgradeFormat(): boolean;
```

파일의 형식 버전을 [FORMAT_VERSION](../../types/node/constants.md)으로 올리고, 올렸는지 돌려줍니다. [`upgradeFormat`](../../types/node/open-options.md#upgradeformat) 옵션이 `false`가 아니라면 여는 쪽이 하는 일과 같습니다. 이미 그 버전인 파일이면 `false`를 돌려줍니다. 쓰기처럼 쓰기 잠금을 기다리고, 파일을 혼자 써야 합니다. 다른 프로세스가 파일을 열고 있으면 아무것도 바꾸지 않고 `BUSY`로 실패합니다. `write`가 거부되는 곳에서는 똑같이 `INVALID_ARGUMENT`로 거부됩니다. 올린 뒤에는 예전 버전만 아는 릴리스가 파일을 거부합니다.

### upgradeFormatAsync

```ts
upgradeFormatAsync(): Promise<boolean>;
```

스레드 풀에서 실행하는 `upgradeFormat`입니다. 이 프로세스가 그 파일에 먼저 시작한 쓰기가 끝난 뒤에 실행됩니다.

### setKey

```ts
setKey(key: Uint8Array): void;
```

암호화한 데이터베이스의 키를 32바이트 `key`로 바꿉니다. 페이지를 다시 암호화하지는 않으며, 반환한 뒤로는 옛 키나 비밀번호로 파일을 열 수 없습니다. 커밋을 하므로 `write`와 마찬가지로, 같은 파일에 대한 쓰기 트랜잭션의 함수 안에서나 이 프로세스의 비동기 쓰기가 파일을 쥐고 있는 동안에는 `INVALID_ARGUMENT`로 거부됩니다. 암호화하지 않은 데이터베이스이거나 키가 32바이트가 아니어도 `INVALID_ARGUMENT`로 실패합니다. 패키지는 호출하는 순간 키를 복사하므로, 호출한 쪽은 그 뒤에 자기 버퍼를 지워도 됩니다. 키와 비밀번호는 [암호화](../../guide/encryption.md)에서 다룹니다.

### setKeyAsync

```ts
setKeyAsync(key: Uint8Array): Promise<void>;
```

스레드 풀에서 실행하는 `setKey`입니다. 이 프로세스가 그 파일에 먼저 시작한 쓰기가 끝난 뒤에 실행됩니다. 키는 차례를 기다리기 전, 호출하는 순간에 복사합니다.

### setPassword

```ts
setPassword(password: string | Uint8Array): void;
```

암호화한 데이터베이스의 키를 `password`에서 Argon2id로 얻은 키로 바꿉니다. 해시 비용은 데이터베이스를 열 때 `passwordHashing`으로 정한 값이고, 정하지 않았으면 기본값입니다. 거부되거나 실패하는 경우는 `setKey`와 같습니다. `Uint8Array`는 호출이 반환되면 지워도 됩니다. 문자열은 가비지 컬렉터가 거둘 때까지 메모리에 남습니다.

### setPasswordAsync

```ts
setPasswordAsync(password: string | Uint8Array): Promise<void>;
```

스레드 풀에서 실행하는 `setPassword`입니다. 이 프로세스가 그 파일에 먼저 시작한 쓰기가 끝난 뒤에 실행됩니다.

### sync

```ts
sync(): void;
```

미룬 커밋까지 모든 커밋을 디스크에 기록합니다. 쓰기를 기다릴 수 있으므로 `write`와 마찬가지로, 같은 파일에 대한 쓰기 트랜잭션의 함수 안에서나 이 프로세스의 비동기 쓰기가 파일을 쥐고 있는 동안에는 `INVALID_ARGUMENT`로 거부됩니다.

### syncAsync

```ts
syncAsync(): Promise<void>;
```

스레드 풀에서 실행하는 `sync`입니다. 이 프로세스가 그 파일에 먼저 시작한 쓰기가 끝난 뒤에 실행됩니다.

### close

```ts
close(): void;
```

미룬 커밋을 디스크에 기록하고 데이터베이스를 닫습니다. 이미 닫은 데이터베이스를 닫으면 아무 일도 하지 않습니다. 쓰기를 기다릴 수 있으므로 `write`와 마찬가지로, 같은 파일에 대한 쓰기 트랜잭션의 함수 안에서나 이 프로세스의 비동기 쓰기가 파일을 쥐고 있는 동안에는 `INVALID_ARGUMENT`로 거부됩니다. 거부되면 데이터베이스는 열린 채로 남습니다.

### closeAsync

```ts
closeAsync(): Promise<void>;
```

스레드 풀에서 실행하는 `close`입니다. 이 프로세스가 그 파일에 먼저 시작한 쓰기가 끝난 뒤에 실행됩니다. 부르는 순간부터 데이터베이스는 새 작업을 받지 않고, `isOpen`도 곧바로 `false`가 됩니다. 같은 파일에 대한 비동기 쓰기의 함수 안에서 부르면 `INVALID_ARGUMENT`로 거부되고, 데이터베이스는 열린 채로 남습니다.
