## What this changes

<!-- What the change is, and why it is needed. If it fixes an issue, say `Fixes #123`. -->

## How it works

<!-- The approach, and anything a reviewer would otherwise have to work out from the diff. If you considered another way and rejected it, this is the place to say so. -->

## Type of change

<!-- Tick everything that applies. -->

- [ ] `fix` — a bug fix
- [ ] `feat` — a new feature
- [ ] `docs` — documentation only
- [ ] `refactor` — no change in behaviour
- [ ] `test` — tests only
- [ ] `chore` / `package` — build, tooling, dependencies
- [ ] **Breaking change** — an existing API behaves differently or is gone
- [ ] **File format change** — a file written before this change reads differently after it, or not at all

## Checklist

- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [ ] `cargo test --workspace` passes
- [ ] `npm run lint`, `npx prettier . --check` and `npm test` pass in every JavaScript folder I touched
- [ ] I added or updated tests for the behaviour I changed
- [ ] I updated the documentation in **both** `docs/en` and `docs/ko` (or said below why not)
- [ ] I added an entry under `## vNext` in the changed package's `CHANGELOG.md`, or this change is invisible to a consumer
- [ ] This adds **no new runtime dependency** — or it does, and I have said below what it does, what its licence is, and what it brings with it

## Anything else

<!-- Benchmark numbers for a change that claims to be faster. Known gaps. Follow-up work. -->
