# Changelog

> This crate's history. DaruDB keeps a separate changelog for each package it ships, beside that package's own manifest, because the packages version independently.

## vNext (2026--)

### Added

- `Database::open` and `OpenOptions` open a database file, creating it when nothing exists at the path. A new file appears whole or not at all, even when two processes create it at once. Its page size is a power of two from 4096 to 65536, and the file's header is validated every time it is opened again.
- `Database::begin_read` and `Database::begin_write` start transactions over named trees of byte keys and byte values. `ReadTransaction` sees one commit for as long as it lives, whatever is committed after it began. `WriteTransaction` adds `insert`, `remove` and `delete_tree`, sees its own changes, and makes all of them visible and durable at once with `commit`; dropping it aborts it. Both offer `get`, `range`, `iter`, `len` and `tree_names`.
- A commit is durable when `commit` returns: a crash or a power cut at any moment leaves the file at a commit no older than the last one that returned. Opening a file after a crash recovers it without a separate step.
- Every page is verified against the check its parent recorded before it is used, so a damaged or stale page is reported as `CORRUPTED` rather than read.
- A value too large to keep in a tree's pages is stored in pages of its own, up to 4 GiB.
- `Database` handles to one file in a process share one instance, so they see the same commits and take turns writing. `OpenOptions::busy_timeout` sets how long `begin_write` waits for another writer.
- `Error` reports every failure with a stable `code`: `IO`, `NOT_FOUND`, `NOT_A_DATABASE`, `UNSUPPORTED_FORMAT_VERSION`, `CORRUPTED`, `INVALID_ARGUMENT`, `CLOSED`, `BUSY`, `SYNC_FAILED` and `INTERNAL`. `CLOSED` is for a language binding's handle used after it was closed; `SYNC_FAILED` means a barrier failed, the last commit's outcome is unknown, and the file has to be opened again.
- `Database::close` reports whether a barrier failed on any handle to the file.
