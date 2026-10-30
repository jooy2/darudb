# Work in progress

What is left of the work on query and schema features that started on 2026-10-30, written so that it can be picked up on another machine from this file and the repository alone. Delete this file in the commit that finishes the last item.

## Where it stands

The maintainer asked for ten features in the engine and in every package (Rust API, Node.js, Dart, Python), each with tests and documentation in both locales. Nine are on `main`, one commit per package and feature, every suite green at each commit:

1. Aggregates: `sum`, `min`, `max`, `avg`, IR field 7.
1. Update and delete by query: `update_where`, `delete_where`, `change_where`, all or nothing for updates.
1. Case-insensitive comparison: Unicode simple case folding, IR expression field 5, `IGNORE CASE` in the query language.
1. Filters on the objects that link to an object: operator 16 (`LINKED`), `ANY posts.author (...)` in the query language.
1. A typed query builder for Rust: `FieldOf` methods, `PathOf`, `IntoPath`.
1. The date type, which brought object format 2.
1. Lists of embedded objects.
1. Indexes on several fields and on fields inside embedded objects (features 8 and 9), planned by their prefix of equalities and a range.

Nothing has been released: every change waits under `## vNext` in each package's changelog, and no version was raised. The maintainer's decisions that still apply:

- Commit on local `main`, one commit per unit of work and package, no branch and no pull request. Push `main` once the work is done.
- Rust takes the breaking changes a 2.0 would, but its version stays 1.x; each changelog lists them under "Breaking changes".
- An object format 2 construct on a format 1 file follows `OpenOptions::upgrade_format`: with it off, opening fails with `INVALID_ARGUMENT` naming the option.
- A cascade reads the link field's index when it has one, and scans the linking collection when it does not.

## 1. Cascading deletes (feature 10, not started)

Deleting an object deletes the objects whose link field marked "cascade" names it, and so on down the chain. Object format 2, since a build without cascades would write a file without honouring them.

### Engine (`[core]`)

- **On disk.** The field record of the stored schema (`encode_fields` and `decode_fields` in `crates/darudb/src/format/object/schema.rs`) gains field 6, a `bool`, on a top-level to-one link field only (`Kind::Link`, not a list of links, not inside an embedded object). `StoredSchema::check` refuses it anywhere else as `CORRUPTED`, `StoredSchema::needs` makes it require format 2, and `FieldDef` gains `cascade: bool`. Write it only when true, so that format 1 files stay byte for byte as they are. Specify it in `design/objects.md`, "The stored schema".
- **Declaring.** `Collection::cascade("author")` in `crates/darudb/src/schema/declare.rs`, validated there (`INVALID_ARGUMENT` for a field that is not a top-level to-one link). `Field` gains `is_cascade()` for reading it back. `resolve.rs` carries it into the stored schema and treats adding or removing it as an additive change in a migration (no records change).
- **Working out the cascades once.** In `OpenSchema::new` (`format/object/schema.rs`, around `impl OpenSchema`), build for each collection the list of inbound cascades: `(linking collection position, link field id, the index on that field if any)`. A delete in a collection nothing cascades into must cost nothing more than today; check with `examples/object_bench.rs` ("delete, in one transaction").
- **Running them.** Every delete ends in `CollectionWriter::delete` (`crates/darudb/src/schema/objects.rs`, `pub fn delete`), which `delete_where` (`query/change.rs`), the typed writer (`schema/typed.rs`, `pub fn delete`) and every binding call (Node `packages/node/src/lib.rs`, the three `delete` call sites; Dart `packages/dart/darudb/native/src/ops.rs`; Python `packages/python/src/transaction.rs`) go through. After the object is deleted and its `Spare` buffers given back, push its key on a work list of copied keys, and work the list: for each inbound cascade, find the linking objects, by the link field's index when it has one, otherwise by scanning the linking collection once per round with the round's deleted keys in a key set (as `LINKED` builds one in `query/run.rs`), never once per object. Delete each found object the same way, which pushes its key. An object already gone is not visited again, so cycles and self-links end.
- **Return value.** `delete` keeps returning whether the named object existed; `delete_where` keeps returning how many objects its query found, not counting the cascade. Document that.
- **Failure.** A delete cannot be refused, so the cascade needs no undo log; a `CORRUPTED` or I/O failure part-way leaves the uncommitted transaction holding what was deleted, as any engine failure does, and the documentation says to abort.
- **Migrations.** A migration function that deletes cascades too (`Migrating` in `schema/`), with the schema the migration is moving to.
- **Derive.** `#[darudb(cascade)]` on a `Link<T>` field in `crates/darudb-derive/src/lib.rs`, refused at compile time on any other field type.
- **Tests.** In `schema/tests.rs` and `query/tests.rs`: chains of three collections, a cycle of two, a self-link, with and without an index on the link field, a cascade inside a large `delete_where`, inside a migration function, a dangling link (nothing to delete), format 1 to 2 raise and the `upgrade_format(false)` refusal, `StoredSchema::check` refusing field 6 on a non-link. In `src/crash.rs`, give one object-run collection a cascading link and check after every cut that no object links to a deleted one through it.

### Bindings

- **Node.js (`[node]`).** `t.link('users').cascade()` in `lib/schema.ts` (refused on anything but a top-level to-one link), `cascade` in the stored schema's field encoding in `lib/codec.ts` (field 6), `StoredField.cascade` in `lib/types.ts` and `describeSchema` in `lib/shared.ts`, `types/check.ts` lines, tests in `test/`.
- **Dart (`[dart]`).** `@Cascade()` in `lib/src/annotations.dart`, `FieldSpec.cascade` in `lib/src/schema.dart` with field 6 in its encoder, the stored schema reader in `lib/src/stored.dart`, and the generator in `packages/dart/darudb_generator` (refuse it on anything but a `Link<T>`, test the output). Regenerate `example/main.g.dart` if the output changes.
- **Python (`[python]`).** `field(link=User, cascade=True)` in `python/darudb/_schema.py`, carried through `FieldSpec` in `src/schema.rs` to `Collection::cascade`, `StoredField.cascade`, tests in `tests/`, lines in `tests/typing/check_types.py`.

### Documentation (`[docs]`)

Both locales: the guide's objects page (links, deleting), each language's collection page and the attribute or modifier, the stored schema type pages, and `engine/bindings.md` (field 6). The comparison table in `docs/.vitepress/theme/comparison.ts` has no row for cascades; adding one means checking every other column against each project's own documentation, so ask the maintainer first. The migration guide whose table maps a foreign key to a link (under `docs/*/migration/`) can add that `ON DELETE CASCADE` maps to a cascading link.

## 2. Verification left

- **Benchmarks.** Run `cargo run -p darudb --release --example object_bench` on the commit before this work (`37a9eb7`) and on the last one, on one machine, and compare. Measured after feature 8, every workload was within noise except "count age >= 40 through the index", which ran 12 to 13% fewer counts a second in the new build. Its planning cost and its profile were unchanged, and builds that kept symbols measured equal, so the difference is where the code landed, not the change; it needs no fix. Measure again after feature 10, especially "delete, in one transaction", which the cascade must not slow.
- **Long suites in release.** `DARUDB_CRASH_SEEDS=2000`, `DARUDB_KILL_ROUNDS=500` and `DARUDB_PROCESS_KILLS=300`, the commands in [CONTRIBUTING.md](CONTRIBUTING.md#running-the-checks). After feature 9 (commit `b92baaf`), on macOS, all three passed: the crash and multi-process suites in 74 seconds, the process-kill suite in 69. Run all three again after feature 10, before the last commit.
- **Samples.** Not run yet for any of the ten features. `npm install` and `npm test` in `samples/node` (it builds its screens and runs Playwright against the web page and Electron) after `npm run build` in `packages/node`, and `flutter test integration_test` in `samples/flutter` where Flutter is installed.

## 3. `CLAUDE.md` (`[common]`)

It describes features 1 to 9 already. After feature 10, add cascades to "v1 queries" and a line to "Things that surprise" on where they are worked out (`OpenSchema::new`) and how they run, and delete this file in the same commit.

## 4. Proposals to put to the maintainer

- The count through an index measured 12% slower only because of where its code landed. Aligning functions as well as loops (`-C llvm-args=-align-all-functions=6` beside the loop alignment in `.cargo/config.toml`) might keep such shifts out of A/B comparisons; it costs binary size, and needs measuring before anything changes.
- Planned deviations, already in the changelogs: `FieldOf::new` takes the slot and the stored name; Dart declares indexes on several fields with a class annotation, `@IndexOn([...])`, rather than an argument of `@Collection`, because a Dart constructor cannot mix optional positional and named parameters; the engine gained `Error::UnsupportedObjectFormat`, whose code is `UNSUPPORTED_FORMAT_VERSION`.
