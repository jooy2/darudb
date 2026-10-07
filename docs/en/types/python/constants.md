---
title: Constants
order: 12
---

# Constants

The package exports three values besides its API: the file format version this build reads and writes, the version of the engine inside it, and the version of the package itself.

```python
import darudb

print(f"darudb {darudb.__version__}, engine {darudb.ENGINE_VERSION}, file format {darudb.FORMAT_VERSION}")
```

## FORMAT_VERSION

```python
FORMAT_VERSION: int
```

The file format version this build of the engine reads and writes. Every file records the version it was written in, which [`Database.format_version`](../../api/python/database.md#format-version) reads, and opening a file with another version fails with `UNSUPPORTED_FORMAT_VERSION`. Version 5 is the format of the first release: a file a development build wrote before it fails this way, and every later format version will come with a migration from the one before. [File format](../../engine/file-format.md) describes what the version covers.

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
