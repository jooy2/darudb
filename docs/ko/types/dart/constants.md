---
title: 상수
order: 15
---

# 상수

패키지는 API 말고도 최상위 getter 두 개를 내보냅니다. 이 빌드가 읽고 쓰는 파일 포맷 버전과, 패키지에 든 엔진의 버전입니다.

```dart
import 'package:darudb/darudb.dart';

void main() {
  print('DaruDB engine $engineVersion, file format $formatVersion');
}
```

## formatVersion

```dart
int get formatVersion;
```

이 빌드의 엔진이 읽고 쓰는 파일 포맷 버전입니다. 파일마다 자신을 쓴 포맷 버전을 기록하고, 그 값은 [`Database.formatVersion`](../../api/dart/database.md#formatversion)으로 읽습니다. 버전이 다른 파일을 열면 `UNSUPPORTED_FORMAT_VERSION`으로 실패합니다. 첫 릴리스 전까지는 포맷이 고정되지 않았으므로, 예전 빌드가 쓴 파일이 업그레이드 뒤에 이렇게 실패할 수 있습니다. 이 버전이 무엇을 정하는지는 [파일 포맷](../../engine/file-format.md)에 있습니다.

## engineVersion

```dart
String get engineVersion;
```

패키지에 든 DaruDB 엔진의 버전으로, `'0.1.0'` 같은 문자열입니다. 엔진과 Dart 패키지는 버전을 따로 매기므로, 패키지의 `pubspec.yaml`에 적힌 버전과 다를 수 있습니다.
