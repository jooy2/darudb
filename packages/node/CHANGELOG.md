# Changelog

> This package's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- `Database.open` opens a database file, creating it when nothing exists at the path, with `create` and `pageSize` options. `path`, `isOpen`, `pageSize` and `formatVersion` describe the open database, and `close` flushes and closes it.
- Every error thrown by the package carries the engine's stable `code`, such as `NOT_FOUND`, `NOT_A_DATABASE` or `CLOSED`.
- `engineVersion()` and `FORMAT_VERSION` report the engine inside the package and the file format version it reads and writes.
