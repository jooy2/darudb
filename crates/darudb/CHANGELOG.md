# Changelog

> This crate's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- `Database::open` and `OpenOptions` open a database file, creating it when nothing exists at the path. A new file appears whole or not at all, even when two processes create it at once. Its header records the file format version and the page size, a power of two from 4096 to 65536, and it is validated every time the file is opened again.
- `Error` reports every failure with a stable `code`: `IO`, `NOT_FOUND`, `NOT_A_DATABASE`, `UNSUPPORTED_FORMAT_VERSION`, `CORRUPTED`, `INVALID_ARGUMENT` and `CLOSED`, the last for a language binding's handle used after it was closed.
- `Database::close` flushes the file to the storage device and reports a failure to do so.
