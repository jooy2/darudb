# Contributing to DaruDB

Thank you for contributing. Bug reports, fixes, tests and documentation changes are all welcome.

This project adheres to the [Contributor Covenant code of conduct](CODE_OF_CONDUCT.md). Contributing means you have read and agree to it. The maintainers will warn or restrict any behaviour that breaks it.

## Issues

Issues can be created on the following page: https://github.com/jooy2/darudb/issues

Alternatively, you can email the package maintainer. However, we prefer to track progress via GitHub Issues.

When creating an issue, keep the following in mind:

- Please specify the correct category selection based on the format of the issue (e.g., bug report, feature request).
- Check to see if there are duplicate issues.
- Describe in detail what is happening and what needs to be fixed. You may need additional materials such as logs or a damaged database file.
- Use appropriate keyword titles to make it easy for others to search and understand.
- Please use English in all content.
- You may need to describe the environment in which the issue occurs, including the file system.

**Do not open an issue for a security problem.** [SECURITY.md](SECURITY.md) has the private route, which keeps a vulnerability out of public view until there is a version to upgrade to.

## Where things live

The repository holds one database engine, written in Rust, the bindings that ship it to each language, and one documentation site shared by all of them:

| Path                             | What it is                                                      | How it is run                                                          |
| -------------------------------- | --------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `crates/darudb`                  | The engine and the Rust API, the crate `darudb`                 | `cargo test -p darudb` from the repository root                        |
| `crates/darudb-derive`           | `#[derive(Object)]` and `#[derive(Embedded)]` for the crate     | `cargo test -p darudb -p darudb-derive` from the repository root       |
| `packages/node`                  | The Node.js binding, the npm package `darudb`                   | `cd packages/node && npm install`, then `npm run build` and `npm test` |
| `packages/dart/darudb`           | The Dart package `darudb`, with its native library in `native/` | `dart pub get`, `dart run build_runner build`, then `dart test`        |
| `packages/dart/darudb_generator` | The Dart code generator `darudb_generator`                      | `dart pub get`, then `dart test`                                       |
| `packages/python`                | The Python binding, the PyPI package `darudb`                   | `cd packages/python`, then `maturin develop` and `pytest` in a venv    |
| `docs`                           | The documentation site, shared by every language                | `cd docs && npm install`, then `npm run dev`                           |
| `design`                         | The engine's specifications, in English only                    | Read before changing the file format, commits, recovery or locking     |

The repository root holds the Cargo workspace (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`) and nothing for JavaScript. There is no root `package.json` and no npm workspace: each JavaScript folder is entered and installed on its own.

The Dart package builds its native library from `packages/dart/darudb/native`, a member of the Cargo workspace, in its build hook, which `dart test` runs; the first build compiles the engine and takes a minute or two. The tests' models are annotated classes whose `.g.dart` parts `build_runner` writes and git ignores, so `dart run build_runner build` comes before `dart test`.

The Python package builds its native module, `darudb._native`, from `packages/python/src`, a member of the Cargo workspace, with maturin. `maturin develop` compiles it into `packages/python/python/darudb`, beside the Python code that wraps it, where git ignores it, and installs the package into the virtual environment it runs in, so a virtual environment comes first. The tests import the package that is installed, so build again after every change to Rust code, in the engine as well as in the binding.

`design/` specifies the file format, the commit and recovery protocol and the locking protocol. A change to any of them updates the matching document in the same commit.

## One engine, thin bindings

Everything a database does is decided in `crates/darudb`: how a file is laid out, what a query means, which error a failure is. A binding translates between its language and the engine and decides nothing of its own. That is what keeps a program in one language and a program in another reading the same file the same way.

In practice:

- **A new behaviour starts in the engine**, with its tests there, and then reaches each binding. A binding that grows logic the engine does not have is a second implementation that will drift from the first.
- **Error codes are shared.** `Error::code` in `crates/darudb/src/error.rs` is the stable, machine-readable name of a failure, and every binding hands it to its language unchanged (`error.code` in JavaScript). A new error gets a new code there, and the bindings pick it up.
- **Crossing the language boundary costs more than the engine's own work** for small operations. A binding passes whole records and whole batches across in one call rather than one field at a time.

A few notes that are easy to trip over:

- **Each package keeps its own `CHANGELOG.md`**, beside its manifest, where its registry and a reader browsing that package expect to find it. The documentation site's copy is generated from them by `docs/scripts/copy-changelog.mjs` and is git-ignored, so edit the package's file and never the one under `docs/`.
- **A change usually means a change to the docs in _both_ languages.** `docs/en` and `docs/ko` mirror each other page for page. If you cannot write the Korean, write the English and say so in the pull request, and a maintainer will follow up rather than let the two drift.
- **`packages/node/native.js` and `native.d.ts` are generated**, together with the native addon, by `npm run build` from the `#[napi]` items in `packages/node/src/lib.rs`. They are git-ignored, and an edit to either is lost on the next build. The package's API is `lib/`, in TypeScript, written by hand over them, with its types in `lib/types.ts`; `tsc` compiles `lib/` into `dist/`, declarations included, which is what the package ships and what the tests load.
- **The Rust version is pinned** in `rust-toolchain.toml`. `rustup` installs that version on the first `cargo` command in the repository, so every machine and every CI runner compiles with the same compiler.
- **Tests never touch a fixed path.** Every test that writes a database writes it into a temporary directory of its own, so the suites can run in parallel and leave nothing behind.

## Running the checks

Everything CI runs, you can run. From the repository root, for the engine and everything else written in Rust:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The crash suites run a quick version by default. Before a change to the storage engine is merged, run them long, in release mode so that they finish in minutes:

```bash
DARUDB_CRASH_SEEDS=2000 cargo test -p darudb --release --lib crash
DARUDB_KILL_ROUNDS=500 cargo test -p darudb --release --test process_kill
DARUDB_PROCESS_KILLS=300 cargo test -p darudb --release --lib processes
```

A change meant to make the engine faster comes with numbers from before and after it, on the same machine:

```bash
cargo run -p darudb --release --example kernel_bench
```

For the Node.js binding:

```bash
cd packages/node
npm ci
npm run build        # the native addon, `native.js`, `native.d.ts`, then `dist/`
npm run lint
npx prettier . --check
npm test             # `dist/` again, node:test against it, then the types
npm run bench        # optional: the object workloads, after a release build
```

`npm test` compiles `lib/` into `dist/` first, but it loads the addon that `npm run build` left in the folder, so build first after every change to Rust code, in the engine as well as in the binding. `npm run build:ts` compiles `lib/` alone.

The package also runs in Electron's main process, which has a test of its own in `packages/node/electron`, since Electron is a large install the other tests do not need. It loads the package's `dist/` and the addon beside it, so build first:

```bash
cd packages/node/electron
npm ci
node node_modules/electron/install.js   # when npm skipped the install script that unpacks Electron
npm test
```

For the Python binding, in a virtual environment of its own:

```bash
cd packages/python
python -m venv .venv
source .venv/bin/activate          # .venv\Scripts\activate on Windows
pip install maturin pytest ruff mypy
maturin develop                    # the native module, debug build, into python/darudb
pytest
ruff check python tests tool
ruff format --check python tests tool
mypy                               # the package, strict, and tests/typing
```

`maturin develop --release` builds the module as a wheel is built, which is what to measure speed with. `tests/typing/check_types.py` is never run: mypy checks it, and fails when what a type checker infers through the API changes.

For the documentation site:

```bash
cd docs
npm ci
npm run dev          # local preview
npm run build        # what the deploy workflow runs
npm run lint
npm run typecheck
```

## Third-party dependencies

The engine is the part of an application that holds its data, so what it depends on is held to a high standard. Every crate it pulls in is code that runs with access to that data and a line in the application's own licence review.

`crates/darudb` depends at run time only on what would be worse to write ourselves: the XXH3 hash, the operating system's random numbers, the ciphers, Argon2id and BLAKE2b, key wiping, the C library and Windows API calls for file locks, and a positional write of several buffers at once, which the standard library offers only on nightly. `crates/darudb/Cargo.toml` says beside each one what it does, what licence it has and what it brings along. A cryptographic primitive is the standing example of a dependency worth having, because a hand-written cipher is a vulnerability waiting for a reviewer. Every one has to be permissively licensed.

A pull request that adds a runtime dependency to the engine or to a binding should say, in the description:

- **What it does that we would otherwise write.**
- **Its licence.** MIT, ISC, BSD, Apache-2.0 and Zlib are fine. Copyleft licences (GPL, LGPL, AGPL, MPL) are not, because they would reach into the applications that embed this one.
- **Its own dependency tree and its size.** A small crate that brings twelve more is not a small crate.
- **Whether it builds for every target the packages ship to**, the oldest operating systems included.

Development dependencies are held to a much looser standard, because they never reach a consumer.

The prebuilt Node.js addon links these crates, the binding's own and what they bring, so each platform package ships their notices in `THIRD_PARTY_NOTICES.txt`, which `packages/node/scripts/notices.mjs` writes from `cargo metadata` and the licence files each crate ships. `npm run notices -- --print` in `packages/node` shows them. A new dependency that ships no licence file needs its text in `packages/node/scripts/licenses`, or the release stops. Every Python wheel ships the notices of the crates its native module links too, in `darudb/THIRD_PARTY_NOTICES.txt`, which `packages/python/tool/notices.py` writes the same way before the release workflow builds the wheels; `python tool/notices.py --print` in `packages/python` shows them.

## Releasing a package

The crate, the npm package, the Dart package and its generator, and the Python package version independently, and each is released on its own by the `release` workflow, which is started by hand from the Actions tab, or for a Dart package after its first version by pushing its tag. Without **publish**, it publishes nothing: it builds and tests what would be published, for every platform the npm and Dart packages ship to, and lists what each package would contain. Make that run before every release.

Nothing raises a version or releases a package by itself. A push to `main` runs the tests and deploys the documentation site, and the changes wait under `## vNext` in each package's changelog until a maintainer releases that package. Start one run of the workflow at a time: it keeps one run waiting behind the one that is running, and starting another cancels the waiting one.

1. Raise the version. For the crate, `version` in `crates/darudb/Cargo.toml` and `crates/darudb-derive/Cargo.toml`, and the `darudb` and `darudb-derive` entries of `[workspace.dependencies]` in the root `Cargo.toml`: the two crates release together, at one version, since the code the macros generate calls the engine's crate, which depends on exactly that version of them. For the npm package, `npm version <version> --no-git-tag-version` in `packages/node`; the platform packages take the version when the workflow makes them. For the Dart package, `version` in `packages/dart/darudb/pubspec.yaml`, and for the generator, `version` in `packages/dart/darudb_generator/pubspec.yaml`. For the Python package, `version` in `packages/python/pyproject.toml` and `__version__` in `packages/python/python/darudb/__init__.py`, which a test holds to the same number.
1. Rename the changelog's `## vNext (<year>--)` section, in both crates' changelogs for the crate, to `## v<version> (<date>)`. The workflow refuses to publish until the changelog names the version.
1. Commit that as `[core] chore: release v<version>`, `[node] chore: release v<version>`, `[dart] chore: release v<version>` or `[python] chore: release v<version>`, and push it.
1. Run the workflow on that commit without **publish**, and read what it lists.
1. Run it again with **publish**. The crates and the npm packages publish through their registries' trusted publishing: crates.io and npm trust `release.yml` in this repository and take the run's OIDC token in place of a secret, so the repository keeps none. It publishes from a job of its own that runs no third-party code. Cargo publishes `darudb-derive` first, then `darudb`. npm publishes every `@darudb/darudb-<platform>` package first, then `darudb` with them as optional dependencies, each with a provenance statement, and skips a version npm has already, so a run that failed part of the way can run again. PyPI takes the Python package's wheels and its source distribution from a job that runs in the `pypi` environment, with an attestation for each file, and skips a file it has already.
1. A package a registry does not have yet, such as a new crate or the npm package of a new platform, cannot trust the workflow until it exists. Publish its first version by hand, or from the workflow with an API token, then set its trusted publisher: on crates.io, in the crate's settings, with the repository `jooy2/darudb` and the workflow `release.yml`; on npm, with `npm trust github <package> --file release.yml --repo jooy2/darudb --allow-publish`. PyPI is the exception: a pending publisher creates the project on its first publish. Add one on PyPI, under the account's publishing settings, for the project `darudb`, the owner `jooy2`, the repository `darudb`, the workflow `release.yml` and the environment `pypi`, and the first run with **publish** creates the project.
1. Once it has published, the workflow tags the commit `darudb-v<version>` for the crate, `node-v<version>` for the npm package, `dart-v<version>` for the Dart package or `python-v<version>` for the Python package, and makes a GitHub release whose notes are the changelog's section for the version. A separate job does that, the only one that may write to the repository.
1. Check the release where an application meets it: in an empty project, `cargo add darudb`, `npm install darudb`, `pip install darudb` or a dependency on the Dart package at the new version, then open a database.
1. The Dart packages are the exception. For `darudb`, the workflow builds its native library for every target, tests three of them through the build hook, and attaches them all to the GitHub release; for `darudb_generator`, it tests the generator. pub.dev accepts automated publishing only from a workflow that a tag push started, and only for a package it already has, so a run started by hand stops there. Never publish from `packages/dart/darudb` itself: its manifest lists no library. Manual publishing is turned off for both packages on pub.dev to stop that, rather than with `publish_to: none`, which would fail pub.dev's check that the repository holds the package.

Each Dart package's first version is published by hand, and every later one by pushing its tag:

1. **The first version.** Run the workflow with **publish** for `dart` or `dart-generator`, download the run's `dart-package` or `dart-generator-package` artifact, whose `hook/prebuilt.json` names the release's libraries and their hashes for `darudb`, and run `dart pub publish` in it.
1. **Once, on pub.dev.** On the package's admin page, enable publishing from GitHub Actions, with the repository `jooy2/darudb`, the tag pattern `dart-v{{version}}` for `darudb` or `dart-generator-v{{version}}` for `darudb_generator`, and the environment `pub.dev` required, and turn manual publishing off. The workflow's publishing job runs in that environment, so required reviewers set on it in the repository's settings hold every publish until they approve.
1. **Every later version.** Raise the version, rename the changelog's section and push the commit as above, run the workflow on it without **publish**, and then push the tag: `git tag dart-v<version>` or `git tag dart-generator-v<version>` on that commit, then `git push origin <tag>`. The run it starts checks that the tag names the package's version, builds and tests the package, makes the GitHub release, and publishes the package to pub.dev with a token GitHub signs for the run, so the repository keeps no pub.dev secret. Pushing the tag is the request to publish.

The Python package builds its wheels for Linux in the PyPA's manylinux2014 and musllinux_1_2 containers, for macOS and Windows on runners of each architecture, and tests each where it was built, Alpine's included. Every platform gets two wheels, the stable ABI's for CPython 3.11 and later and the free-threaded 3.14's, and 32-bit Windows the first alone; the packaging job refuses a release with any of them missing.

The Node.js addon and the Python module have the engine compiled in, so a fix to the engine reaches Node.js or Python only with a release of that package.

## How to contribute (Pull Requests)

### Write the code you want to change

1. Clone the project, or rebase onto the latest commit on the main branch.
2. Install the dependencies of the folder you are working in.
3. Set up the linter and formatter in your IDE and install the matching plugins. The commands are listed under [Running the checks](#running-the-checks).
4. Write the code.
5. Update the documentation, or add a page where none exists. The site is published in both English and Korean, so update both. Write the content in your own language rather than leaving it out; a maintainer will follow up on the translation.
6. Add or change tests as the change warrants, and confirm that the existing tests pass.
7. Add an entry under `## vNext` in the changelog of the package you changed, unless the change is invisible to a consumer.

### Write a commit message

There are no strict rules for commit messages, but follow these where you can:

- Write in English.
- Wrap function, variable, folder and file names in backticks.
- Use the format `[scope] tag: message (fixes #1)`. The part in parentheses is optional.
- Summarize what was modified.
- Split unrelated modifications into separate commits.

The scope names the part of the repository that changed, since one repository holds several codebases:

- `[core]`: `crates/darudb` and `crates/darudb-derive`, the engine, the Rust API and its derive macros
- `[node]`: `packages/node`, the Node.js binding
- `[dart]`: `packages/dart`, the Dart package, its native library and its code generator
- `[python]`: `packages/python`, the Python package and its native module
- `[docs]`: `docs`, the documentation site
- `[common]`: anything shared by all of them, such as the repository files, the Cargo workspace or CI

Then the tag, followed by `: ` and the summary. The tags follow the [Udacity Git Commit Message Style Guide](https://udacity.github.io/git-styleguide). You may use a tag outside this list where none of them fits.

- `feat`: A new feature
- `fix`: A bug fix
- `docs`: Changes to documentation
- `style`: Formatting, missing semicolons, etc.; no code change
- `refactor`: Refactoring production code
- `test`: Adding tests, refactoring test; no production code change
- `chore`: Updating build tasks, package manager configs, etc.; no production code change

Informal tags:

- `package`: Modifications to package settings, modules, or GitHub projects
- `typo`: Fix typos

### Create a pull request

When creating a pull request, keep the following in mind:

- Describe what the modification is, why it is needed, and how it works.
- Check to see if there are duplicate pull requests.
- Please use English in all content.

A maintainer reviews and tests the code before merging it. That can take some time, and they may ask for further edits or for clarification in the comments.
