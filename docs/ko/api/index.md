---
title: API
order: 1
---

# API

사이드바에서 고른 언어의 패키지가 제공하는 클래스와 함수를 하나씩 따로 정리했습니다.

::: lang rust

`darudb` 크레이트는 모든 것을 최상위에서 내보냅니다. `use darudb::{Database, OpenOptions};`처럼 가져오면 됩니다. 이 호출들이 받고 돌려주는 타입은 [타입](../types/index.md)에 있습니다.

:::

::: lang node

`darudb` 패키지는 모든 것을 최상위에서 내보냅니다. `import { Database, schema } from 'darudb';`처럼 가져오면 됩니다. 이 호출들이 받고 돌려주는 타입은 [타입](../types/index.md)에 있습니다.

:::

::: lang dart

`darudb` 패키지는 모든 것을 라이브러리 하나에서 내보냅니다. `import 'package:darudb/darudb.dart';`로 가져오면 됩니다. 생성기 `darudb_generator`는 어노테이션을 붙인 클래스마다 스키마 상수와 쿼리 빌더를 씁니다. 이 호출들이 받고 돌려주는 타입은 [타입](../types/index.md)에 있습니다.

:::

<PageList section="api" />
