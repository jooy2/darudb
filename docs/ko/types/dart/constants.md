---
title: 상수
order: 15
---

# 상수

패키지는 API 말고도 최상위 getter 두 개를 내보냅니다. 이 빌드가 읽고 쓰는 가장 새 파일 형식 버전과, 패키지에 든 엔진의 버전입니다.

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

이 빌드의 엔진이 읽고 쓰는 가장 새 파일 형식 버전인 6이며, 새 파일은 이 버전으로 만듭니다. 파일마다 자신을 쓴 형식 버전을 기록하고, 그 값은 [`Database.formatVersion`](../../api/dart/database.md#formatversion)으로 읽습니다. 이 빌드가 읽지 않는 버전의 파일을 열면 `UNSUPPORTED_FORMAT_VERSION`으로 실패합니다. 첫 릴리스의 형식은 버전 5이고, 버전 6은 리프 항목의 길이를 더 적은 바이트로 적습니다. 이 빌드는 두 버전을 모두 읽고 쓰며, 버전 5 파일을 열면 `upgradeFormat`이 `false`가 아닌 한 버전 6으로 올립니다. 버전 5보다 앞선 개발 빌드가 쓴 파일은 열리지 않습니다. 이 버전이 무엇을 정하는지는 [파일 형식](../../engine/file-format.md#형식-버전)에 있습니다.

## engineVersion

```dart
String get engineVersion;
```

패키지에 든 DaruDB 엔진의 버전으로, `'0.1.0'` 같은 문자열입니다. 엔진과 Dart 패키지는 버전을 따로 매기므로, 패키지의 `pubspec.yaml`에 적힌 버전과 다를 수 있습니다.
