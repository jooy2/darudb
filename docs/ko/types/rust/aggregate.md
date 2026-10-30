---
title: Aggregate
order: 12.5
group: queries
pageClass: reference-page
---

# Aggregate

`Aggregate`는 쿼리가 찾은 객체의 한 필드 값에서 무엇을 계산할지 나타냅니다.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Aggregate {
    Sum,
    Min,
    Max,
    Avg,
}
```

실행 중에 함수를 고르는 프로그램은 [`CollectionReader::aggregate`](../../api/rust/collection-reader.md#aggregate)에 이 값을 넘깁니다. [`sum`](../../api/rust/collection-reader.md#sum), [`min`](../../api/rust/collection-reader.md#min), [`max`](../../api/rust/collection-reader.md#max), [`avg`](../../api/rust/collection-reader.md#avg)는 이름이 곧 함수입니다. null 값과 값에 닿지 않는 경로는 빼고 계산합니다.

| 변형 | 읽는 타입 | 결과 |
| --- | --- | --- |
| `Sum` | `int`, `float` | 정확한 합계. 정수 필드는 정수, 실수 필드는 실수이며 값이 없으면 0 |
| `Min` | `int`, `float`, `string`, `bytes` | 정렬과 같은 순서의 최솟값, 없으면 null |
| `Max` | `int`, `float`, `string`, `bytes` | 최댓값, 없으면 null |
| `Avg` | `int`, `float` | 정확한 합계를 개수로 나눠 한 번 반올림한 실수 평균, 없으면 null |

## 메서드

### code

```rust
pub fn code(self) -> i64
```

IR에서 이 함수를 나타내는 코드입니다. `Sum`은 1, `Min`은 2, `Max`는 3, `Avg`는 4입니다.

### from_code

```rust
pub fn from_code(code: i64) -> Option<Self>
```

IR 코드가 `code`인 함수를 돌려줍니다. 그런 함수가 없으면 `None`입니다.
