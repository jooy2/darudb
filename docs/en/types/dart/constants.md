---
title: Constants
order: 15
---

# Constants

The package exports two top-level getters besides its API: the newest file format version this build reads and writes, and the version of the engine inside it.

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

The newest file format version this build of the engine reads and writes, 6, which a new file gets. Every file records the version it was written in, which [`Database.formatVersion`](../../api/dart/database.md#formatversion) reads, and opening a file in a version this build does not read fails with `UNSUPPORTED_FORMAT_VERSION`. Version 5 is the format of the first release, and version 6 writes the lengths in a leaf's entries in fewer bytes. This build reads and writes both, and raises a file of version 5 to version 6 when it opens it, unless `upgradeFormat` is `false`; a file a development build wrote before version 5 does not open. [File format](../../engine/file-format.md#format-versions) describes what the version covers.

## engineVersion

```dart
String get engineVersion;
```

The version of the DaruDB engine inside the package, such as `'0.1.0'`. The engine and the Dart package are versioned separately, so it can differ from the version in the package's `pubspec.yaml`.
