---
title: 도구
order: 11
---

# 도구

DaruDB에는 파일을 검사하고, 복사하고, 줄이고, 되살릴 때 쓰는 도구가 들어 있습니다. 검사와 백업과 압축은 다른 핸들과 프로세스가 파일을 읽고 쓰는 동안에도 동작합니다. 되살리기는 아무도 쓸 수 없게 된 파일을 위한 도구라서 파일을 혼자 씁니다.

## 파일 검사하기

무결성 검사는 게시된 커밋이 닿는 모든 것을 읽어 확인합니다. 모든 페이지가 부모가 기록한 검사값과 맞는지, 키가 순서대로 있는지, 트리마다 항목 수가 맞는지, 파일의 모든 페이지가 사용 중이거나 비었거나 보류 중인 상태 중 정확히 하나인지 봅니다. 스키마가 있는 파일에서는 모든 객체가 인덱스와 맞는지도 봅니다. 첫 문제에서 멈추지 않고 찾은 문제를 모두 보고합니다. 실패하는 것은 데이터베이스가 닫혀 있을 때처럼 검사를 시작할 수 없을 때뿐입니다.

::: lang rust

```rust
use darudb::Database;

fn check(db: &Database) -> Result<(), darudb::Error> {
    let report = db.check()?;

    if !report.is_ok() {
        for problem in &report.problems {
            eprintln!("{problem}");
        }
    }

    Ok(())
}
```

:::

::: lang node

```ts
const report = db.check(); // 또는 `await db.checkAsync()`

if (!report.ok) {
  for (const { page, tree, message } of report.problems) {
    console.error(page, tree, message);
  }
}
```

:::

::: lang dart

```dart
final report = db.check(); // 또는 `await db.checkAsync()`

if (!report.ok) {
  for (final problem in report.problems) {
    print('${problem.page} ${problem.tree} ${problem.message}');
  }
}
```

:::

::: lang python

```python
report = db.check()  # 또는 `await db.check_async()`

if not report.ok:
    for problem in report.problems:
        print(problem.page, problem.tree, problem.message)
```

:::

- 문제마다 그 문제가 있는 페이지와, 문제를 찾은 트리나 컬렉션이 적혀 있습니다.
- 검사는 파일 전체를 읽으므로 모든 객체를 읽는 것과 비슷하게 걸립니다. 메모리는 페이지마다 1비트만 쓰고, 객체 수에 따라 늘지 않습니다.
- 읽을 수 없는 페이지가 있으면 그 아래 페이지는 읽지 못합니다. 이런 페이지는 하나씩 누수로 보고하지 않고 문제 하나로 묶어 셉니다.

## 파일 백업하기

백업은 게시된 커밋을 새 파일로 복사합니다. 그동안 다른 핸들과 프로세스는 계속 읽고 씁니다. 사본에는 빈 공간이 없고, 페이지 크기는 원본과 같으며, 원본이 암호화돼 있으면 같은 키나 비밀번호로 열립니다.

::: lang rust

```rust
use darudb::Database;

fn back_up(db: &Database) -> Result<(), darudb::Error> {
    let report = db.backup("backups/app.darudb")?;

    println!("{} entries of commit {}", report.entries, report.commit_id);
    Ok(())
}
```

:::

::: lang node

```ts
const report = await db.backupAsync('backups/app.darudb'); // 또는 `db.backup(path)`
console.log(`${report.entries} entries of commit ${report.commitId}`);
```

:::

::: lang dart

```dart
final report = await db.backupAsync('backups/app.darudb'); // 또는 `db.backup(path)`

print('${report.entries} entries of commit ${report.commitId}');
```

:::

::: lang python

```python
report = db.backup("backups/app.darudb")  # 또는 `await db.backup_async(path)`

print(f"{report.entries} entries of commit {report.commit_id}")
```

:::

- 사본은 대상 경로 옆에 임시 이름으로 쓰고, 온전하고 디스크에 기록된 뒤에야 대상 경로로 옮깁니다. 백업은 이미 있는 파일을 덮어쓰지 않습니다. 경로에 파일이 있으면 `INVALID_ARGUMENT`로 실패합니다.
- 백업은 도는 동안 복사하는 커밋을 읽기 트랜잭션처럼 붙잡고 있습니다. 그래서 그동안 다른 쪽이 쓰면 파일이 커질 수 있습니다.

### 새 키로 백업하기

암호화한 파일의 키나 비밀번호를 바꾸면 데이터 키를 다시 감쌀 뿐, 모든 페이지를 암호화한 데이터 키 자체는 그대로입니다. 백업에 키나 비밀번호를 따로 주면 사본을 무작위로 만든 새 데이터 키로 씁니다. 그래서 노출됐을 수 있는 데이터 키는 옛 파일과 함께 남습니다. 옛 파일의 핸들을 모두 닫은 뒤 사본을 옛 파일 자리에 두세요. 평문 데이터베이스의 사본도 같은 방식으로 암호화되므로, 평문 데이터베이스를 암호화할 때도 이 방법을 씁니다.

::: lang rust

```rust
use darudb::{BackupOptions, Database};

fn rekey(db: &Database) -> Result<(), darudb::Error> {
    db.backup_with("app.rekeyed.darudb", BackupOptions::new().password("a new password"))?;
    Ok(())
}
```

:::

::: lang node

```ts
await db.backupAsync('app.rekeyed.darudb', { password: 'a new password' }); // 또는 `db.backup(path, options)`
```

:::

::: lang dart

```dart
await db.backupAsync('app.rekeyed.darudb', password: 'a new password'); // 또는 `db.backup(path, ...)`
```

:::

::: lang python

```python
db.backup("app.rekeyed.darudb", password="a new password")  # 또는 `await db.backup_async(...)`
```

:::

## 파일 압축하기

파일은 한때 필요했던 페이지를 계속 갖고 있습니다. 객체를 지우면 파일 안에 빈 페이지가 생기고 이후 쓰기가 그 페이지를 다시 쓰지만, 파일이 저절로 줄지는 않습니다. 압축은 파일 끝쪽 페이지를 앞쪽 빈 페이지로 옮기고, 비게 된 끝을 파일 시스템에 돌려줍니다. 그 전에, 삽입으로 페이지가 덜 찬 트리를 꽉 채워 다시 씁니다. 키가 정해진 순서 없이 들어오면 트리의 페이지는 3분의 2쯤만 차므로, 이 과정으로 파일이 백업 사본과 비슷한 크기까지 줄어듭니다. 파일을 그 자리에서 줄이므로 다른 핸들과 프로세스는 그동안 계속 읽고 씁니다.

::: lang rust

```rust
use darudb::Database;

fn compact(db: &Database) -> Result<(), darudb::Error> {
    let report = db.compact()?;

    println!("{} bytes, then {}", report.bytes_before, report.bytes_after);
    Ok(())
}
```

:::

::: lang node

```ts
const report = await db.compactAsync(); // 또는 `db.compact()`
console.log(`${report.bytesBefore} bytes, then ${report.bytesAfter}`);
```

:::

::: lang dart

```dart
final report = await db.compactAsync(); // 또는 `db.compact()`

print('${report.bytesBefore} bytes, then ${report.bytesAfter}');
```

:::

::: lang python

```python
report = db.compact()  # 또는 `await db.compact_async()`

print(f"{report.bytes_before} bytes, then {report.bytes_after}")
```

:::

- 압축은 평범한 쓰기 트랜잭션으로 이뤄집니다. 그래서 다른 쓰기처럼 쓰기 잠금을 기다리고, 도중에 멈추면 파일은 압축의 커밋 중 하나에 남습니다.
- 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 그 트랜잭션이 끝날 때까지 옮기지 못합니다. 그래서 오래 도는 읽기가 있으면 덜 줄어듭니다. 나머지는 다음 압축이 처리합니다.
- 형식 버전 5에서 올린 파일이라면, 압축은 형식 6의 작은 항목으로 페이지가 눈에 띄게 줄어드는 트리도 다시 씁니다([파일 형식](../engine/file-format.md#형식-버전)).
- 파일을 건드리지 않고 압축된 사본을 얻으려면 백업을 쓰세요.

## 손상된 파일 되살리기

파일이 열리지 않거나 검사에서 손상이 나오면, 되살리기로 건질 수 있는 것을 새 파일에 옮깁니다. 옛 파일을 여는 대신 페이지 단위로 읽고, 파일에 기록된 가장 새 커밋에서 시작합니다. 그 커밋의 페이지를 읽을 수 없는 곳은 파일에 아직 남은 같은 페이지의 옛 버전에서 같은 키를 가져옵니다. 그다음 객체로 모든 인덱스를 다시 만들어서, 새 파일은 검사를 통과합니다.

::: lang rust

```rust
use darudb::OpenOptions;

fn rescue() -> Result<(), darudb::Error> {
    let report = OpenOptions::new().salvage("app.darudb", "rescued.darudb")?;

    if !report.is_whole() {
        eprintln!(
            "{} entries from older pages, {} values lost, {} objects dropped",
            report.entries_recovered, report.values_lost, report.objects_dropped
        );
    }

    Ok(())
}
```

암호화한 파일은 `OpenOptions`에 키나 비밀번호를 정해 되살립니다.

:::

::: lang node

```ts
import { Database } from 'darudb';

const report = await Database.salvageAsync('app.darudb', 'rescued.darudb'); // 또는 `Database.salvage`

if (!report.whole) {
  console.warn(report.entriesRecovered, report.valuesLost, report.objectsDropped);
}
```

암호화한 파일은 세 번째 인자인 옵션에 `key`나 `password`를 넣어 되살립니다.

:::

::: lang dart

```dart
final report = await Database.salvageAsync('app.darudb', 'rescued.darudb'); // 또는 `Database.salvage`

if (!report.whole) {
  print('${report.entriesRecovered} ${report.valuesLost} ${report.objectsDropped}');
}
```

암호화한 파일은 이름 있는 인자에 `key`나 `password`를 넣어 되살립니다.

:::

::: lang python

```python
import darudb

report = darudb.Database.salvage("app.darudb", "rescued.darudb")  # 또는 `Database.salvage_async`

if not report.whole:
    print(report.entries_recovered, report.values_lost, report.objects_dropped)
```

암호화한 파일은 키워드 인자에 `key`나 `password`를 넣어 되살립니다.

:::

- 새 파일이 가장 새 커밋을 그대로 담으면 <LangCode rust="is_whole()" node="whole" dart="whole" python="whole" />이 참입니다. 그렇지 않으면 읽지 못한 곳을 옛 버전으로 채운 것입니다. 항목이 옛 값을 가질 수 있고, 잃어버린 페이지에서 지워졌던 항목이 되살아날 수도 있습니다.
- 레코드를 읽을 수 없는 객체와, 고유 인덱스 값을 다른 객체가 이미 차지한 객체는 빼고 그 수를 셉니다.
- 되살리기는 파일을 혼자 써야 합니다. 어느 프로세스든 파일을 열고 있으면 `BUSY`로 실패하고, 되살리는 동안 파일을 열어도 `BUSY`로 실패합니다.
- 새 파일의 페이지 크기는 옛 파일과 같고, 암호화한 파일의 키나 비밀번호로 새 파일도 열립니다. 백업처럼 경로에 이미 있는 파일은 덮어쓰지 않습니다.
- 파일 전체를 두 번쯤 읽고, 항목이 든 페이지마다 첫 키와 마지막 키를 메모리에 둡니다. 그래서 파일이 크면 그만큼 메모리가 듭니다.
