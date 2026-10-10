---
title: Durability
order: 4
group: queries
counterpart: /types/node/write-options
pageClass: reference-page
---

# Durability

`Durability`는 쓰기 트랜잭션이 어떻게 커밋할지, 즉 커밋이 디스크를 기다릴지 정합니다.

```python
Durability: TypeAlias = Literal["sync", "deferred"]
```

[`write`와 `write_async`](../../api/python/database.md#write)가 키워드 인자 `durability`로 받으며, 주지 않으면 `"sync"`입니다. 그 밖의 값은 `write`나 `write_async`를 부를 때 `INVALID_ARGUMENT`로 실패합니다. 두 가지 커밋은 [트랜잭션](../../guide/transactions.md)에서 자세히 설명합니다.

```python
with db.write(durability="deferred") as txn:
    txn.collection(User).insert(User(name="Alice"))

# 나중에, 사용자가 정전에도 남아야 할 때:
db.sync()
```

## 값

### sync

커밋이 디스크에 기록된 뒤에 반환하므로, 그 뒤에 전원이 나가도 커밋은 사라지지 않습니다. 커밋마다 파일을 한 번 동기화합니다.

### deferred

디스크를 기다리지 않고 반환합니다. 커밋은 모든 프로세스의 읽기에 곧바로 보이고, 이 프로세스가 비정상 종료돼도 잃지 않습니다. 디스크에는 다음 동기 커밋이나 `sync`, `close`, 이들의 `_async` 짝을 부를 때 기록됩니다. 마지막 동기화 뒤의 지연 커밋이 1초를 기다렸거나 16,384페이지를 썼을 때도 기록됩니다. 이 한도는 엔진의 기본값이고, 패키지는 바꾸지 않습니다. 그 전에 전원이 나가면 지연 커밋은 가장 새것부터 거꾸로 사라집니다. 그래서 파일은 커밋이 뒤섞인 상태가 아니라 앞선 어느 커밋의 상태로 돌아옵니다.

지연 커밋은 로그처럼 작은 쓰기가 많고, 전원이 나갔을 때 마지막 1초쯤은 잃어도 되는 경우에 맞습니다. 블록이 끝난 뒤에는 잃으면 안 되는 쓰기에는 동기 커밋이 맞습니다.
