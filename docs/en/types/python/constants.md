---
title: Constants
order: 12
---

# Constants

The package exports three values besides its API: the newest file format version this build reads and writes, the version of the engine inside it, and the version of the package itself.

```python
import darudb

print(f"darudb {darudb.__version__}, engine {darudb.ENGINE_VERSION}, file format {darudb.FORMAT_VERSION}")
```

## FORMAT_VERSION

```python
FORMAT_VERSION: int
```

The newest file format version this build of the engine reads and writes, 6, which a new file gets. Every file records the version it was written in, which [`Database.format_version`](../../api/python/database.md#format-version) reads, and opening a file in a version this build does not read fails with `UNSUPPORTED_FORMAT_VERSION`. Version 5 is the format of the first release, and version 6 writes the lengths in a leaf's entries in fewer bytes. This build reads and writes both, and raises a file of version 5 to version 6 when it opens it, unless `upgrade_format` is `False`; a file a development build wrote before version 5 does not open. [File format](../../engine/file-format.md#format-versions) describes what the version covers.

## ENGINE_VERSION

```python
ENGINE_VERSION: str
```

The version of the DaruDB engine inside the package, such as `"1.0.0"`. The engine and the Python package are versioned separately, so it can differ from `__version__`.

## \_\_version\_\_

```python
__version__: str
```

The version of the package, the one `pip` installed, such as `"1.0.0"`.
