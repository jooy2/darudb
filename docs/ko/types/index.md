---
title: 타입
order: 1
pageClass: reference-page
---

# 타입

API가 받고 돌려주는 타입을 사이드바에서 고른 언어 기준으로 하나씩 따로 정리했습니다.

::: lang rust

모든 타입은 크레이트 최상위에서 `darudb::Value`처럼 가져옵니다. 이 타입을 받고 돌려주는 호출은 [API](../api/index.md)에 있습니다.

:::

::: lang node

TypeScript용 타입은 패키지 최상위에서 `import type { OpenOptions } from 'darudb';`처럼 가져옵니다. 이 타입을 받고 돌려주는 호출은 [API](../api/index.md)에 있습니다.

:::

::: lang dart

타입은 패키지의 라이브러리 `package:darudb/darudb.dart`에서 내보냅니다. 이 타입을 받고 돌려주는 호출은 [API](../api/index.md)에 있습니다.

:::

::: lang python

타입은 패키지 최상위에서 `darudb.Key`처럼 쓰거나 `from darudb import CheckReport`처럼 가져옵니다. 패키지에 타입 힌트가 함께 들어 있습니다. 이 타입을 받고 돌려주는 호출은 [API](../api/index.md)에 있습니다.

:::

<PageList section="types" grouped />

## 언어별 필드 타입

스키마의 필드에는 아래 가운데 하나가 담깁니다. 패키지마다 선언하는 방법과 읽어 오는 타입은 다르지만, 한 언어에서 쓴 파일을 다른 언어에서 읽어도 똑같이 읽힙니다.

| 필드에 담기는 것 | Rust | Node.js | Dart | Python |
| --- | --- | --- | --- | --- |
| 불리언 | `Type::Bool`, `bool` | `t.bool()`, `boolean` | `bool` | `bool` |
| 64비트 정수 | `Type::Int`, `i64` | `t.int()`, `number`, 또는 `t.bigint()`, `bigint` | `int` | `int` |
| 64비트 실수 | `Type::Float`, `f64` | `t.float()`, `number` | `double` | `float` |
| 문자열 | `Type::String`, `String` | `t.string()`, `string` | `String` | `str` |
| 바이트 | `Type::Bytes`, `Vec<u8>` | `t.bytes()`, `Uint8Array` | `Uint8List` | `bytes` |
| 목록 | `Type::list(...)`, `Vec<T>` | `t.list(...)`, 배열 | `List<E>` | `list[E]` |
| 다른 컬렉션으로 가는 링크 | `Type::link(...)`, `Link<T>` | `t.link(...)`, 키 | `Link<T>` | 키 타입과 `field(link=...)` |
| 내장 객체 | `Type::object(...)`, `#[derive(Embedded)]`를 붙인 구조체 | `t.object({...})`, 객체 | `@Embedded()`를 붙인 클래스 | `@darudb.embedded`를 붙인 클래스 |
| 선택 필드의 빈 값 | `optional(...)`, `Option<T>` | `.optional()`, `null` | 널 허용 타입, `null` | `X \| None`, `None` |

Rust에서 짝의 앞쪽은 [`Schema`](../api/rust/schema.md)에서 필드를 선언하는 방법이고, 뒤쪽은 [`#[derive(Object)]`](../api/rust/derive.md)로 컬렉션으로 만든 구조체에서 쓰는 타입입니다. Node.js에서 앞쪽은 [`t`](../api/node/t.md) 빌더이고, 뒤쪽은 객체에 담기는 값입니다. Dart와 Python은 클래스에 적은 타입으로 필드를 선언합니다. 자세한 내용은 언어별 필드 타입 페이지에 있습니다.
