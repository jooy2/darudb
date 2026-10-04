---
title: Database
order: 1
---

# Database

`Database`는 Dart에서 연 DaruDB 파일을 나타내며, 프로그램은 이 객체로 트랜잭션을 실행하고 쿼리를 준비하고 파일 도구를 씁니다.

```dart
final class Database implements Finalizable
```

생성자는 없습니다. `Database.open`이나 `Database.openAsync`가 만들어 돌려줍니다. [스키마](./schema.md) 없이 연 데이터베이스에는 컬렉션이 없고, 스키마를 주고 열면 `darudb_generator`가 클래스마다 써 준 상수로 컬렉션에 접근합니다. `userSchema`가 그런 상수입니다.

`close`나 `closeAsync`로 닫기 전까지 쓸 수 있습니다. 닫은 뒤에는 `path`와 `isOpen`만 읽을 수 있고, 나머지는 모두 코드가 `CLOSED`인 [DaruException](../../types/dart/error.md)을 던집니다. 프로세스가 이미 연 파일을 이 isolate에서든 다른 isolate에서든 다시 열면 같은 데이터베이스의 핸들이 하나 더 생기고, 페이지 캐시도 함께 씁니다. 스키마는 핸들마다 열 때 받은 것을 계속 씁니다. 닫지 않은 핸들은 가비지 컬렉터가 거둘 때 닫히지만, 그 시점에 기대면 안 됩니다.

호출은 모두 부른 isolate에서 실행되고 엔진이 반환할 때까지 그 isolate를 붙잡습니다. 그래서 다른 쓰기를 기다리는 쓰기나 디스크를 기다리는 동기 커밋은 Flutter 앱의 UI isolate도 붙잡습니다. 파일에 접근하는 메서드에는 이름이 `Async`로 끝나는 짝이 있고, 짝 메서드는 엔진의 일을 패키지의 네이티브 라이브러리가 가진 스레드에서 하고 `Future`를 돌려줍니다. 두 방식이 한 파일을 어떻게 함께 쓰는지는 [비동기 API](../../guide/async.md)에서 설명합니다.

## 정적 메서드

### open

```dart
static Database open(
  String path, {
  Schema? schema,
  List<Migration> migrations = const [],
  bool create = true,
  int? pageSize,
  Duration? busyTimeout,
  int? cacheSize,
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

`path`의 데이터베이스를 엽니다. 그 자리에 아무것도 없으면 새로 만들고, 스키마를 저장하거나 비교하거나 마이그레이션합니다.

| 옵션 | 설명 |
| --- | --- |
| `schema` | 파일에 담을 컬렉션. 처음 열 때 저장하고, 그 뒤로는 비교하며, 더 낮은 버전을 가진 파일은 마이그레이션합니다 |
| `migrations` | [Migration](./migration.md) 단계 목록. 엔진이 알아서 하는 것 말고도 할 일이 있는 버전마다 하나씩 둡니다 |
| `create` | 없는 파일을 만들지 여부. `false`면 만들지 않고 `NOT_FOUND`로 실패합니다 |
| `pageSize` | 새 파일의 페이지 크기. 4096부터 65536 사이의 2의 거듭제곱이며 기본값은 4096입니다. 이미 있는 파일은 자기 페이지 크기를 씁니다 |
| `busyTimeout` | 쓰기가 다른 프로세스의 쓰기를 기다리다 `BUSY`로 실패하기까지의 시간. 기본값은 5초입니다 |
| `cacheSize` | 페이지 캐시가 쓸 메모리. 단위는 바이트이며 기본값은 32 MiB입니다 |
| `key` | 32바이트 키. 새 파일을 암호화하고, 암호화한 파일을 엽니다 |
| `password` | 비밀번호. Argon2id로 만든 키로 `key`와 같은 일을 합니다 |
| `passwordHashing` | 비밀번호에서 키를 만드는 비용. 새 파일과 `setPassword`에 씁니다. [PasswordHashing](../../types/dart/password-hashing.md)을 보세요 |

마이그레이션 함수는 이 호출 안에서 버전 단계마다 차례로 실행되며 [MigrationContext](./migration-context.md)를 받습니다. 여기서는 동기 함수여야 하고, `Future`를 돌려주면 `INVALID_ARGUMENT`로 실패합니다. 함수가 예외를 던지면 파일은 예전 스키마와 데이터를 그대로 유지하고, `open`도 같은 예외를 던집니다.

- `NOT_FOUND`: `path`에 아무것도 없는데 `create`가 `false`입니다.
- `NOT_A_DATABASE`: DaruDB 데이터베이스가 아닌 파일입니다.
- `KEY_REQUIRED`, `WRONG_KEY`: 암호화된 파일인데 `key`도 `password`도 없거나, 있어도 틀렸습니다.
- `SCHEMA_MISMATCH`: 파일에 버전은 같지만 내용이 다른 스키마가 있습니다. `SCHEMA_TOO_NEW`: 파일의 스키마 버전이 더 높습니다.
- `BUSY`: 다른 프로세스가 `busyTimeout`보다 오래 파일을 붙잡고 있거나, 되살리기가 파일을 쓰고 있습니다.
- `INVALID_ARGUMENT`: 쓸 수 없는 옵션이 있습니다. 4096부터 65536 사이의 2의 거듭제곱이 아닌 페이지 크기, 32바이트가 아닌 키, 키와 비밀번호를 함께 준 경우, 엔진이 저장할 수 없는 스키마가 그 예입니다.

```dart
final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));
```

패키지는 호출하는 동안 키와 비밀번호를 네이티브 메모리로 복사하고, 엔진이 제 사본을 가져가면 그 복사본을 0으로 채웁니다. `Uint8List` 키는 `open`이 반환되자마자 `fillRange`로 지워도 됩니다. `String`은 지울 수 없습니다.

### openAsync

```dart
static Future<Database> openAsync(
  String path, {
  Schema? schema,
  List<Migration> migrations = const [],
  bool create = true,
  int? pageSize,
  Duration? busyTimeout,
  int? cacheSize,
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

네이티브 라이브러리의 스레드에서 실행하는 `open`입니다. isolate는 파일이나 복구, 마이그레이션의 커밋을 기다리지 않습니다. 마이그레이션 함수는 비동기여도 되고, 한 단계는 함수의 `Future`가 완료되면 마무리됩니다. 실패하는 경우는 `open`과 같고, 예외 대신 `Future`가 그 오류로 완료됩니다.

### salvage

```dart
static SalvageReport salvage(
  String from,
  String into, {
  Duration? busyTimeout,
  Uint8List? key,
  String? password,
});
```

손상된 `from`의 데이터베이스에서 건질 수 있는 것을 `into`의 새 데이터베이스로 옮기고, 무엇을 건졌고 무엇을 건지지 못했는지 보고합니다. 파일을 여는 대신 페이지 단위로 읽으므로 열리지 않는 파일에도 쓸 수 있습니다. 파일에 기록된 가장 새 커밋에서 시작하고, 그 커밋에서 읽지 못한 부분은 같은 페이지의 옛 버전에서 가져옵니다. 인덱스는 모두 다시 만들므로 새 파일은 무결성 검사를 통과합니다. 암호화한 파일은 `key`나 `password`가 있어야 하고, 새 파일도 그것으로 열립니다. `busyTimeout`은 다른 프로세스가 파일을 닫기를 기다리는 시간입니다. 결과는 [SalvageReport](../../types/dart/salvage-report.md)에, 언제 쓰는지는 [도구](../../guide/tools.md)에 있습니다.

- `BUSY`: 이 프로세스나 다른 프로세스가 파일을 열고 있습니다. 되살리는 동안 파일을 열어도 `BUSY`로 실패합니다.
- `INVALID_ARGUMENT`: `into`에 이미 무언가 있거나 경로가 비었습니다. 되살리기는 파일을 덮어쓰지 않습니다.
- `NOT_FOUND`: `from`에 아무것도 없습니다.
- `KEY_REQUIRED`: 암호화한 파일인데 키도 비밀번호도 주지 않았습니다.

### salvageAsync

```dart
static Future<SalvageReport> salvageAsync(
  String from,
  String into, {
  Duration? busyTimeout,
  Uint8List? key,
  String? password,
});
```

네이티브 라이브러리의 스레드에서 실행하는 `salvage`입니다.

## 속성

### path

```dart
final String path;
```

데이터베이스를 열 때 준 경로 그대로입니다. `close` 뒤에도 읽을 수 있습니다.

### isOpen

```dart
bool get isOpen;
```

데이터베이스가 열려 있는지 나타냅니다. `close`나 `closeAsync`가 데이터베이스를 닫으면 `false`가 됩니다.

### pageSize

```dart
int get pageSize;
```

파일의 페이지 크기이며 단위는 바이트입니다. 파일은 만들 때 정한 페이지 크기를 계속 씁니다.

### formatVersion

```dart
int get formatVersion;
```

파일에 기록된 파일 형식 버전입니다. 이 빌드가 여는 파일이면 최상위 [formatVersion](../../types/dart/constants.md)과 같습니다.

### isEncrypted

```dart
bool get isEncrypted;
```

파일이 암호화돼 있는지 나타냅니다.

### schemaVersion

```dart
int? get schemaVersion;
```

이 핸들이 파일을 열 때 파일에 있던 스키마의 버전입니다. 스키마 없이 열었으면 `null`입니다.

## 메서드

### prepare

```dart
Prepared<T> prepare<T, Q extends QueryBuilder<T>, K extends Object>(
  CollectionSchema<T, Q, K> collection,
  String text,
);
```

`text`를 `collection`에서 실행할 쿼리로 한 번 해석해 둡니다. `text`는 바뀌는 값 자리에 `$0`, `$1` 같은 매개변수를 쓴 쿼리 언어 문자열입니다. 준비한 쿼리인 [Prepared](../../types/dart/prepared.md)는 트랜잭션에 묶이지 않으므로, 그 컬렉션이라면 동기든 비동기든 어느 트랜잭션에서나 `findPrepared`, `findOnePrepared`, `countPrepared`로 실행할 수 있습니다. 해석할 수 없는 문자열이면 `INVALID_QUERY`로 실패합니다.

```dart
final byEmail = db.prepare(userSchema, r'email == $0');
final alice = db.read(
  (txn) => txn.collection(userSchema).findOnePrepared(byEmail, ['alice@example.com']),
);
```

### read

```dart
R read<R>(R Function(ReadTransaction txn) fn);
```

`fn`을 [읽기 트랜잭션](./read-transaction.md) 안에서 실행하고 `fn`이 반환한 값을 돌려줍니다. 트랜잭션은 `fn`이 도는 동안 커밋 하나를 보며, 시작할 때 쓰기를 기다리지 않습니다. `fn`은 동기 함수여야 합니다. `Future`를 돌려주면 `INVALID_ARGUMENT`로 실패합니다.

### readAsync

```dart
Future<R> readAsync<R>(FutureOr<R> Function(AsyncReadTransaction txn) fn);
```

`Future` API로 실행하는 `read`입니다. `fn`은 비동기여도 되고, 컬렉션의 호출은 네이티브 라이브러리의 스레드에서 실행되며, 트랜잭션은 `fn`이 끝날 때까지 커밋 하나를 봅니다. 읽기를 시작할 때는 쓰기를 기다리지 않으므로 트랜잭션은 부른 isolate에서 시작합니다.

### write

```dart
R write<R>(
  R Function(WriteTransaction txn) fn, {
  Durability durability = Durability.sync,
});
```

`fn`을 [쓰기 트랜잭션](./write-transaction.md) 안에서 실행합니다. `fn`이 반환하면 커밋하고, 예외를 던지면 취소하며, `fn`이 반환한 값을 돌려줍니다. 기본으로는 커밋이 디스크에 기록된 뒤 반환하고, `Durability.deferred`를 주면 디스크를 기다리지 않습니다. 자세한 내용은 [Durability](../../types/dart/durability.md)에 있습니다.

- `BUSY`: 다른 프로세스의 쓰기가 `busyTimeout`보다 오래 파일을 붙잡고 있었습니다.
- `INVALID_ARGUMENT`: `fn`이 `Future`를 돌려줘서 트랜잭션을 취소했습니다.
- `INVALID_ARGUMENT`: 같은 파일에 대한 쓰기 트랜잭션의 함수 안에서 불렀거나, 이 isolate의 비동기 쓰기가 파일을 쥐고 있는 동안 불렀습니다. 쓰기 트랜잭션은 겹칠 수 없고, 여기서 기다리면 다른 쓰기가 끝나는 데 필요한 isolate를 붙잡게 됩니다.
- `SYNC_FAILED`: 디스크 동기화가 실패해서 커밋이 반영됐는지 알 수 없습니다. 데이터베이스를 닫고 다시 열어야 합니다.

### writeAsync

```dart
Future<R> writeAsync<R>(
  FutureOr<R> Function(AsyncWriteTransaction txn) fn, {
  Durability durability = Durability.sync,
});
```

`Future` API로 실행하는 `write`입니다. `fn`은 비동기여도 됩니다. 트랜잭션은 `fn`이 완료되고 `fn`이 부른 호출이 모두 끝나면 커밋하고, `fn`이 실패하면 취소합니다. 이 isolate가 한 파일에 하는 비동기 쓰기는 `Database` 객체가 몇 개든 Dart 안에서 차례를 기다렸다가 하나씩 네이티브 라이브러리로 갑니다. 같은 파일에 대한 비동기 쓰기의 함수 안에서 부르면 자기 자신을 기다리는 대신 `INVALID_ARGUMENT`로 실패합니다.

### check

```dart
CheckReport check();
```

게시된 커밋을 빠짐없이 검사합니다. 모든 페이지가 검사값과 맞는지, 키가 순서대로 있는지, 항목 수가 맞는지, 모든 페이지가 사용 중이거나 비었거나 보류 중인 상태 중 정확히 하나인지, 모든 객체가 인덱스와 맞는지 봅니다. 예외를 던지지 않고 찾은 문제를 모두 [CheckReport](../../types/dart/check-report.md)에 담아 돌려주며, 다른 핸들과 프로세스가 쓰는 동안에도 읽습니다.

### checkAsync

```dart
Future<CheckReport> checkAsync();
```

네이티브 라이브러리의 스레드에서 실행하는 `check`입니다. 쓰기를 기다리지 않습니다.

### backup

```dart
BackupReport backup(
  String path, {
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

다른 핸들과 프로세스가 쓰는 동안에도 게시된 커밋의 사본을 `path`에 새 파일로 씁니다. 사본에는 빈 공간이 없고, 페이지 크기는 원본과 같으며, 같은 키나 비밀번호로 열립니다. 파일을 덮어쓰지 않으므로 이미 있는 경로나 빈 경로는 `INVALID_ARGUMENT`로 실패합니다. 결과는 [BackupReport](../../types/dart/backup-report.md)에 있습니다.

32바이트 `key`나 `password`를 주면 사본은 무작위로 만든 새 데이터 키로 암호화되고, 그 키나 비밀번호가 새 데이터 키를 감쌉니다. 비밀번호를 해시하는 비용은 [`passwordHashing`](../../types/dart/password-hashing.md)이 정하고, 사본은 그것으로만 열립니다. 파일의 키나 비밀번호를 바꾸면 데이터 키를 다시 감쌀 뿐 데이터 키 자체는 그대로이므로, 노출됐을 수 있는 데이터 키를 버리려면 새 키로 백업한 뒤 사본을 원래 파일 자리에 두면 됩니다. 평문 데이터베이스의 사본도 같은 방식으로 암호화됩니다. 32바이트가 아닌 키, 빈 비밀번호, 함께 준 키와 비밀번호는 아무것도 쓰기 전에 `INVALID_ARGUMENT`로 실패하며, 패키지는 호출하는 순간 이 값을 복사합니다.

### backupAsync

```dart
Future<BackupReport> backupAsync(
  String path, {
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
});
```

네이티브 라이브러리의 스레드에서 실행하는 `backup`입니다.

### compact

```dart
CompactReport compact();
```

파일을 그 자리에서 줄입니다. 파일 끝의 페이지를 앞쪽의 빈 페이지로 옮기고, 끝부분은 파일 시스템에 돌려줍니다. 자기 쓰기 트랜잭션 안에서 일하므로 쓰기처럼 쓰기 차례를 기다리고, `write`가 거부되는 곳에서는 똑같이 `INVALID_ARGUMENT`로 거부됩니다. 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 옮기지 않습니다. 결과는 [CompactReport](../../types/dart/compact-report.md)에 있습니다.

### compactAsync

```dart
Future<CompactReport> compactAsync();
```

네이티브 라이브러리의 스레드에서 실행하는 `compact`입니다. 이 isolate가 같은 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다.

### setKey

```dart
void setKey(Uint8List key);
```

암호화한 데이터베이스의 키를 32바이트 `key`로 바꿉니다. 페이지를 다시 암호화하지 않으며, 반환된 뒤로는 예전 키나 비밀번호로 파일을 열 수 없습니다. 커밋을 하므로 `write`가 거부되는 곳에서는 똑같이 `INVALID_ARGUMENT`로 거부됩니다. 평문 데이터베이스이거나 키가 32바이트가 아니면 `INVALID_ARGUMENT`로 실패합니다. 키와 비밀번호는 [암호화](../../guide/encryption.md)에서 설명합니다.

### setKeyAsync

```dart
Future<void> setKeyAsync(Uint8List key);
```

네이티브 라이브러리의 스레드에서 실행하는 `setKey`입니다. 이 isolate가 같은 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다. 키는 부르는 순간 복사하므로, 넘긴 리스트는 바로 지워도 됩니다.

### setPassword

```dart
void setPassword(String password);
```

암호화한 데이터베이스의 키를 `password`에서 Argon2id로 만든 키로 바꿉니다. 비용은 파일을 열 때 준 `passwordHashing`이고, 없으면 기본 비용입니다. 거부되거나 실패하는 경우는 `setKey`와 같습니다.

### setPasswordAsync

```dart
Future<void> setPasswordAsync(String password);
```

네이티브 라이브러리의 스레드에서 실행하는 `setPassword`입니다. 이 isolate가 같은 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다.

### sync

```dart
void sync();
```

어느 핸들이나 프로세스가 했든 지연 커밋을 포함한 모든 커밋을 디스크에 기록합니다. 쓰기를 기다릴 수 있으므로 `write`가 거부되는 곳에서는 똑같이 `INVALID_ARGUMENT`로 거부됩니다.

### syncAsync

```dart
Future<void> syncAsync();
```

네이티브 라이브러리의 스레드에서 실행하는 `sync`입니다. 이 isolate가 같은 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다.

### close

```dart
void close();
```

지연 커밋을 디스크에 기록하고 데이터베이스를 닫습니다. 이미 닫힌 데이터베이스를 닫으면 아무 일도 하지 않습니다. 쓰기를 기다릴 수 있으므로 `write`가 거부되는 곳에서는 똑같이 `INVALID_ARGUMENT`로 거부되고, 거부된 `close`는 데이터베이스를 열어 둔 채로 둡니다.

### closeAsync

```dart
Future<void> closeAsync();
```

네이티브 라이브러리의 스레드에서 실행하는 `close`입니다. 이 isolate가 같은 파일에 먼저 시작한 비동기 쓰기가 끝난 뒤에 실행됩니다. 같은 파일에 대한 비동기 쓰기의 함수 안에서 부르면 `INVALID_ARGUMENT`로 실패하고, 데이터베이스는 열린 채로 남습니다.
