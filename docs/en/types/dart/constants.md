---
title: Constants
order: 15
---

# Constants

The package exports two top-level getters besides its API: the file format version this build reads and writes, and the version of the engine inside it.

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

The file format version this build of the engine reads and writes. Every file records the version it was written in, which [`Database.formatVersion`](../../api/dart/database.md#formatversion) reads, and opening a file with another version fails with `UNSUPPORTED_FORMAT_VERSION`. The format is not stable before the first release, so a file that an older build wrote can fail this way after an upgrade. [File format](../../engine/file-format.md) describes what the version covers.

## engineVersion

```dart
String get engineVersion;
```

The version of the DaruDB engine inside the package, such as `'0.1.0'`. The engine and the Dart package are versioned separately, so it can differ from the version in the package's `pubspec.yaml`.
