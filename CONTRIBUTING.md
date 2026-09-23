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

| Path            | What it is                                       | How it is run                                                          |
| --------------- | ------------------------------------------------ | ---------------------------------------------------------------------- |
| `crates/darudb` | The engine and the Rust API, the crate `darudb`  | `cargo test -p darudb` from the repository root                        |
| `packages/node` | The Node.js binding, the npm package `darudb`    | `cd packages/node && npm install`, then `npm run build` and `npm test` |
| `docs`          | The documentation site, shared by every language | `cd docs && npm install`, then `npm run dev`                           |
| `design`        | The engine's specifications, in English only     | Read before changing the file format, commits, recovery or locking     |

The repository root holds the Cargo workspace (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`) and nothing for JavaScript. There is no root `package.json` and no npm workspace: each JavaScript folder is entered and installed on its own.

A Dart binding is planned and will live in `packages/dart`.

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
- **`packages/node/index.js` and `index.d.ts` are generated**, together with the native addon, by `npm run build` from the `#[napi]` items in `packages/node/src/lib.rs`. They are git-ignored. Change the Rust source and rebuild; an edit to either file is lost on the next build.
- **The Rust version is pinned** in `rust-toolchain.toml`. `rustup` installs that version on the first `cargo` command in the repository, so every machine and every CI runner compiles with the same compiler.
- **Tests never touch a fixed path.** Every test that writes a database writes it into a temporary directory of its own, so the suites can run in parallel and leave nothing behind.

## Running the checks

Everything CI runs, you can run. From the repository root, for the engine and everything else written in Rust:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For the Node.js binding:

```bash
cd packages/node
npm ci
npm run build        # the native addon, `index.js` and `index.d.ts`
npm run lint
npx prettier . --check
npm test             # node:test, against the addon just built
```

`npm test` loads the addon that `npm run build` left in the folder, so build first after every change to Rust code, in the engine as well as in the binding.

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

`crates/darudb` has **no** runtime dependency today. A runtime dependency is added only where writing the code ourselves would be worse: a cryptographic primitive is the standing example, because a hand-written cipher is a vulnerability waiting for a reviewer. It has to be permissively licensed.

A pull request that adds a runtime dependency to the engine or to a binding should say, in the description:

- **What it does that we would otherwise write.**
- **Its licence.** MIT, ISC, BSD, Apache-2.0 and Zlib are fine. Copyleft licences (GPL, LGPL, AGPL, MPL) are not, because they would reach into the applications that embed this one.
- **Its own dependency tree and its size.** A small crate that brings twelve more is not a small crate.
- **Whether it builds for every target the packages ship to**, the oldest operating systems included.

Development dependencies are held to a much looser standard, because they never reach a consumer.

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

- `[core]`: `crates/darudb`, the engine and the Rust API
- `[node]`: `packages/node`, the Node.js binding
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
