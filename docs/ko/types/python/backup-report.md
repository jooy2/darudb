---
title: BackupReport
order: 9
group: tools
pageClass: reference-page
---

# BackupReport

`BackupReport`는 백업의 결과로, 복사한 커밋과 복사한 양, 사본의 크기를 담습니다.

```python
@dataclasses.dataclass(frozen=True)
class BackupReport:
    commit_id: int
    trees: int
    entries: int
    bytes: int
```

[`Database.backup`과 `backup_async`](../../api/python/database.md#backup)가 사본이 온전하게 디스크에 기록돼 제 경로에 놓인 뒤 돌려줍니다. 사본에는 백업을 시작할 때 게시돼 있던 커밋이 들어 있고, 그동안 다른 핸들과 프로세스는 계속 썼을 수 있습니다. 백업을 만드는 방식은 [도구](../../guide/tools.md)에 있습니다.

```python
report = db.backup("backups/app.darudb")  # 또는 `await db.backup_async(path)`

print(f"{report.entries} entries of commit {report.commit_id}, {report.bytes} bytes")
```

## 필드

| 필드        | 타입  | 설명                                                               |
| ----------- | ----- | ------------------------------------------------------------------ |
| `commit_id` | `int` | 복사한 커밋의 트랜잭션 id. 백업을 시작할 때 게시돼 있던 커밋입니다 |
| `trees`     | `int` | 복사한 트리 수. 엔진 자체의 트리도 포함합니다                      |
| `entries`   | `int` | 복사한 항목 수. 인덱스의 항목도 포함합니다                         |
| `bytes`     | `int` | 새 파일의 크기. 단위는 바이트입니다                                |
