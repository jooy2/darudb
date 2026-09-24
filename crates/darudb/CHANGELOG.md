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
- `WriteTransaction::commit_deferred` publishes a commit without waiting for a barrier. Readers see it at once; it becomes durable at the next `commit`, at `Database::sync`, when the database is closed, or when `OpenOptions::max_unsynced_time` (one second by default) or `OpenOptions::max_unsynced_pages` runs out. A crash of the process loses no deferred commit, and a power cut undoes them only from the newest backwards.
- `Database::close` makes deferred commits durable first, and reports whether a barrier failed on any handle to the file.
- `OpenOptions::key` and `OpenOptions::password` create an encrypted database, or open one. Every page is encrypted and authenticated under a random data key, which the key, or the Argon2id hash of the password, wraps: with XAES-256-GCM on processors with AES instructions, and XChaCha20-Poly1305 elsewhere. `OpenOptions::password_hashing` sets the Argon2id cost, 19 MiB, 2 iterations and 1 lane by default.
- Each commit record of an encrypted database carries a MAC under a key derived from the data key, so a record put together from the file's own pages by someone without the key is refused. The file format version is 3.
- `Database::set_key` and `Database::set_password` change the key of an encrypted database without encrypting any page again, and `Database::is_encrypted` tells the two kinds of database apart.
- The error codes `KEY_REQUIRED`, for an encrypted database opened without a key or password, and `WRONG_KEY`, for one that does not open it.
- Several processes can open one database file at once, coordinated through the operating system's byte-range locks and nothing else. One process writes at a time and `begin_write` waits for another process's writer as it does for its own, readers never wait for a writer, and a process that dies releases its locks with nothing left for the others to clean up. The first process to open a file recovers it, and one that opens it meanwhile waits up to the busy timeout. `Database::sync` makes deferred commits durable whichever process made them.
- The error code `UNSUPPORTED_FILE_SYSTEM`, for a database on a network file system such as NFS or SMB, or on one whose file locks do not work. Network file systems are recognised on Linux, Android, macOS, iOS and Windows, on a best-effort basis, and a new database is refused before its file is created.
