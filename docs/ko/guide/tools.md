---
title: 도구
order: 5
---

# 도구

DaruDB에는 파일을 검사하거나 되살려야 할 때 쓰는 도구가 들어 있습니다. 모든 도구는 다른 핸들과 프로세스가 파일을 읽고 쓰는 동안에도 동작합니다.

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

- 사본은 대상 경로 옆에 임시 이름으로 쓰고, 온전하고 디스크에 기록된 뒤에야 대상 경로로 옮깁니다. 백업은 이미 있는 파일을 덮어쓰지 않습니다. 경로에 파일이 있으면 `INVALID_ARGUMENT`로 실패합니다.
- 백업은 도는 동안 복사하는 커밋을 읽기 트랜잭션처럼 붙잡고 있습니다. 그래서 그동안 다른 쪽이 쓰면 파일이 커질 수 있습니다.
