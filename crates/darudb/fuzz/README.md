# Fuzz targets

Coverage-guided fuzzing of everything the engine reads from outside itself: a database file is untrusted input, and so is what a language binding hands the engine. A target fails when the engine panics, overflows in arithmetic, or asks for more memory than the limit; an error the engine returns is the right answer to an input that makes no sense.

| Target | What it feeds the engine |
| --- | --- |
| `query_text` | Text in the query language, which is parsed, prepared, and run when it parses |
| `query_ir` | The IR a binding encodes, which is decoded, planned and run on the collection it names |
| `schema` | A schema as a binding encodes it |
| `record` | Records and changes as a binding encodes them, stored in a write transaction that is thrown away |
| `file` | A small database with bytes changed anywhere, which is opened, checked, read, written, compacted and salvaged |

## Running one

The targets need a nightly compiler and `cargo-fuzz`, which the rest of the repository does not:

```bash
rustup toolchain install nightly --profile minimal
```

```bash
cargo install cargo-fuzz --locked
```

Then, from `crates/darudb`:

```bash
cargo +nightly fuzz run file -- -max_total_time=600 -max_len=256 -len_control=0
```

`query_text` has seeds in `seeds/query_text`, given as a second corpus folder: `cargo +nightly fuzz run query_text fuzz/corpus/query_text fuzz/seeds/query_text`. An input that fails is written to `artifacts/<target>/`, and `cargo +nightly fuzz run <target> <that file>` runs it alone. Fix what it shows, and add a test beside the code for it: the corpus a run gathers is not kept in the repository.

## What a fuzzing build changes

`cargo fuzz` builds with `--cfg fuzzing`, and the engine does three things differently under it, each marked where it is:

- **Every check is the same value** (`Check::of` in `format/check.rs`). The file a fuzzer changes would otherwise fail the check of the first page it touched, and nothing past the checks would be reached. With one check for everything, a changed page still passes for the page its parent names, which is what a file whose author worked the hashes out again does to a plain file.
- **Nothing is found in the page cache, or among the field marks** (`storage/cache.rs`, `btree/load.rs`), since a check no longer tells a page from an older one of its number.
- **No flush waits for the disk** (`storage/file.rs`), since a file lives for one input.

So a fuzzing build is for these targets alone, and its files are not ones another build reads.

`file` builds its database once for each process, in one transaction, and reads each input as changes to it: every four bytes set one byte of the file, three saying where and the fourth what.
