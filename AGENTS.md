# AGENTS.md

fml: polyglot format/lint/config orchestrator, 12 language surfaces.

## Commands

Before committing, run the full suite — it's what CI actually runs:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -q -- fmt
```

While iterating, `cargo test --lib -q` is a faster inner loop, but it only runs
the library's unit tests — it skips every file under `tests/` (9 integration
test files as of this writing), so it is not sufficient before committing.

Activate the staged pre-commit hook:

```bash
git config core.hooksPath .githooks
```

## Layout

- `src/config` — formality.toml parsing, resolution, schema
- `src/engine` — execution, diffing, update checks
- `src/surfaces` — one file per language; see `docs/new-surface-guide.md` to add
  one
- `src/ui` — table rendering
- `src/commands` — CLI subcommand handlers

Root layout: `formality.toml` alone carries canonical config without generated
native config files (`.rustfmt.toml`, `.prettierrc`, etc.). `fml sync --check`
is not run against this repository's root.

## Progressive 2-Tier Quality Gate

1. **Tier 1 (Local pre-commit)**: `.githooks/pre-commit` (activated via
   `git config core.hooksPath .githooks`) builds the fresh binary and runs
   `fml fmt --staged` and `fml lint --staged` before commits.
2. **Tier 2 (Parallel PR checks)**: `.github/workflows/pr-check.yml` runs 3
   independent parallel jobs:
   - `Library Tests` (**required status check**):
     `cargo clippy --all-targets -- -D warnings` and full unit/integration test
     suite (`cargo test --verbose`).
   - `Formality Dogfooding` (**required status check**): `fml fmt --check` and
     `fml lint` against this repo, plus schema drift check
     (`cargo test --test schema_drift`; regenerate via
     `UPDATE_SCHEMA=1 cargo test -j 2 --test schema_drift`) and schema version
     progression enforcement.
   - `Security Audit`: `cargo audit` against Rust advisory database.

## Conventions

- Commits: `type(scope): description (Fixes #issue)`, Conventional Commits
  style.
- Format fixes belong in `fml fmt` (mechanical); `fml lint` is semantic-only.
- Always run the freshly built binary (`cargo run -q -- ...`), never a stale
  global `fml` on PATH.

## Always

- Run `cargo test && cargo clippy --all-targets -- -D warnings` before any
  commit — `cargo test --lib -q` is fine for a fast inner loop while iterating,
  but it skips everything under `tests/` and is not sufficient before
  committing.
- Check `docs/INDEX.md` before reading source to understand structure or
  conventions already documented there.

## Ask first

- Anything touching branch protection or CI required-status-check names.
- Version bumps (owned by dedicated tooling, not hand-edits).

## Never

- Commit directly to `main`.

Default to dispatching worker subagents in isolated worktrees, not editing
source directly — see `.agents/orchestrate.md` §8 for the narrow, enumerated
exceptions. That file also covers worktrees, the QA gate, dispatch order, and
design-phase/applied-feature rules.
