---
title: 타입
order: 1
---

# 타입

API가 받고 돌려주는 타입을 사이드바에서 고른 언어 기준으로 하나씩 따로 정리했습니다.

::: lang rust

모든 타입은 크레이트 최상위에서 `darudb::Value`처럼 가져옵니다. 이 타입을 받고 돌려주는 호출은 [API](../api/index.md)에 있습니다.

:::

::: lang node

TypeScript용 타입은 패키지 최상위에서 `import type { OpenOptions } from 'darudb';`처럼 가져옵니다. 이 타입을 받고 돌려주는 호출은 [API](../api/index.md)에 있습니다.

:::

<PageList section="types" />
