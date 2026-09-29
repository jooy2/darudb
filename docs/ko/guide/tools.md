---
title: 도구
order: 5
---

# 도구

DaruDB에는 파일을 검사하고, 복사하고, 줄이고, 되살릴 때 쓰는 도구가 들어 있습니다. 검사와 백업과 압축은 다른 핸들과 프로세스가 파일을 읽고 쓰는 동안에도 동작합니다. 되살리기는 아무도 쓸 수 없게 된 파일을 위한 도구라서 파일을 혼자 씁니다.

## 파일 검사하기

무결성 검사는 게시된 커밋이 닿는 모든 것을 읽어 확인합니다. 모든 페이지가 부모가 기록한 검사값과 맞는지, 키가 순서대로 있는지, 트리마다 항목 수가 맞는지, 파일의 모든 페이지가 사용 중이거나 비었거나 보류 중인 상태 중 정확히 하나인지 봅니다. 스키마가 있는 파일에서는 모든 객체가 인덱스와 맞는지도 봅니다. 첫 문제에서 멈추지 않고 찾은 문제를 모두 보고합니다. 예외는 데이터베이스가 닫혀 있을 때처럼 검사를 시작할 수 없을 때만 던집니다.

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

```ts
const report = db.check(); // 또는 `await db.checkAsync()`

if (!report.ok) {
  for (const { page, tree, message } of report.problems) {
    console.error(page, tree, message);
  }
}
```

- 문제마다 그 문제가 있는 페이지와, 문제를 찾은 트리나 컬렉션이 적혀 있습니다.
- 검사는 파일 전체를 읽으므로 모든 객체를 읽는 것과 비슷하게 걸립니다. 메모리는 페이지마다 1비트만 쓰고, 객체 수에 따라 늘지 않습니다.
- 읽을 수 없는 페이지가 있으면 그 아래 페이지는 읽지 못합니다. 이런 페이지는 하나씩 누수로 보고하지 않고 문제 하나로 묶어 셉니다.

## 파일 백업하기

백업은 게시된 커밋을 새 파일로 복사합니다. 그동안 다른 핸들과 프로세스는 계속 읽고 씁니다. 사본에는 빈 공간이 없고, 페이지 크기는 원본과 같으며, 원본이 암호화돼 있으면 같은 키나 비밀번호로 열립니다.

```rust
use darudb::Database;

fn back_up(db: &Database) -> Result<(), darudb::Error> {
    let report = db.backup("backups/app.darudb")?;

    println!("{} entries of commit {}", report.entries, report.commit_id);
    Ok(())
}
```

```ts
const report = await db.backupAsync('backups/app.darudb'); // 또는 `db.backup(path)`
```

- 사본은 대상 경로 옆에 임시 이름으로 쓰고, 온전하고 디스크에 기록된 뒤에야 대상 경로로 옮깁니다. 백업은 이미 있는 파일을 덮어쓰지 않습니다. 경로에 파일이 있으면 `INVALID_ARGUMENT`로 실패합니다.
- 백업은 도는 동안 복사하는 커밋을 읽기 트랜잭션처럼 붙잡고 있습니다. 그래서 그동안 다른 쪽이 쓰면 파일이 커질 수 있습니다.

## 파일 압축하기

파일은 한때 필요했던 페이지를 계속 갖고 있습니다. 객체를 지우면 파일 안에 빈 페이지가 생기고 이후 쓰기가 그 페이지를 다시 쓰지만, 파일이 저절로 줄지는 않습니다. 압축은 파일 끝쪽 페이지를 앞쪽 빈 페이지로 옮기고, 비게 된 끝을 파일 시스템에 돌려줍니다. 파일을 그 자리에서 줄이므로 다른 핸들과 프로세스는 그동안 계속 읽고 씁니다.

```rust
use darudb::Database;

fn compact(db: &Database) -> Result<(), darudb::Error> {
    let report = db.compact()?;

    println!("{} bytes, then {}", report.bytes_before, report.bytes_after);
    Ok(())
}
```

```ts
const report = await db.compactAsync(); // 또는 `db.compact()`
```

- 압축은 평범한 쓰기 트랜잭션으로 이뤄집니다. 그래서 다른 쓰기처럼 쓰기 잠금을 기다리고, 도중에 멈추면 파일은 압축의 커밋 중 하나에 남습니다.
- 읽기 트랜잭션이 아직 닿을 수 있는 페이지는 그 트랜잭션이 끝날 때까지 옮기지 못합니다. 그래서 오래 도는 읽기가 있으면 덜 줄어듭니다. 나머지는 다음 압축이 처리합니다.
- 파일을 건드리지 않고 압축된 사본을 얻으려면 백업을 쓰세요.

## 손상된 파일 되살리기

파일이 열리지 않거나 검사에서 손상이 나오면, 되살리기로 건질 수 있는 것을 새 파일에 옮깁니다. 옛 파일을 여는 대신 페이지 단위로 읽고, 파일에 기록된 가장 새 커밋에서 시작합니다. 그 커밋의 페이지를 읽을 수 없는 곳은 파일에 아직 남은 같은 페이지의 옛 버전에서 같은 키를 가져옵니다. 그다음 객체로 모든 인덱스를 다시 만들어서, 새 파일은 검사를 통과합니다.

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

- 새 파일이 가장 새 커밋을 그대로 담으면 `is_whole()`이 참입니다. 그렇지 않으면 읽지 못한 곳을 옛 버전으로 채운 것입니다. 항목이 옛 값을 가질 수 있고, 잃어버린 페이지에서 지워졌던 항목이 되살아날 수도 있습니다.
- 레코드를 읽을 수 없는 객체와, 고유 인덱스 값을 다른 객체가 이미 차지한 객체는 빼고 그 수를 셉니다.
- 되살리기는 파일을 혼자 써야 합니다. 어느 프로세스든 파일을 열고 있으면 `BUSY`로 실패하고, 되살리는 동안 파일을 열어도 `BUSY`로 실패합니다.
- 새 파일의 페이지 크기는 옛 파일과 같습니다. Rust에서는 암호화한 파일을 옵션에 키나 비밀번호를 넣어 되살리고, 그 키나 비밀번호로 새 파일도 열립니다. 백업처럼 경로에 이미 있는 파일은 덮어쓰지 않습니다.
- 파일 전체를 두 번쯤 읽고, 항목이 든 페이지마다 첫 키와 마지막 키를 메모리에 둡니다. 그래서 파일이 크면 그만큼 메모리가 듭니다.
