# AGENTS.md

fml: polyglot format/lint/config orchestrator, 12 language surfaces.

Process and Rust style come from the global directives and the `orchestrate` and
`rust-guide` skills. This file records only what is specific to this repo. Check
`docs/INDEX.md` before reading source; `docs/style-guide.md` holds the Rust
deviations and repo-specific code rules.

## Gate

The full gate, run before a PR is marked ready:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -q -- fmt
```

- `cargo test --lib -q` is the fast inner loop. It skips every file under
  `tests/`, so it is never the gate.
- Always run the freshly built binary (`cargo run -q -- ...`), never a global
  `fml` on `PATH`.
- Schema drift: `cargo test --test schema_drift`; regenerate with
  `UPDATE_SCHEMA=1 cargo test -j 2 --test schema_drift`. A changed schema needs
  a forward `SCHEMA_VERSION` bump in `src/config/schema.rs`.
- The root carries only `formality.toml`, no generated native configs
  (`.rustfmt.toml`, `.prettierrc`, ...), so `fml sync --check` is not run here.

## Pre-commit hook

Activate with `git config core.hooksPath .githooks`. `.githooks/pre-commit`
builds the binary, then runs `fml fmt --staged --allow-missing` and
`fml lint --staged --allow-missing`.

- `fmt --staged` can rewrite a file after it was staged, leaving the commit and
  the working tree diverged. Run `git status` after every commit.
- A commit blocked by its own dogfooding is the hook working; fix the file, do
  not route around it.

## CI and merging

- `.github/workflows/pr-check.yml` runs `Library Tests` (clippy + full
  `cargo test`), `Formality Dogfooding` (`fml fmt --check`, `fml lint`, schema
  drift and version progression), `Fresh-Install Regression` (3-OS matrix), and
  `Security Audit`.
- Branch protection on `main` requires `Library Tests` and
  `Formality Dogfooding` plus resolved conversations. It requires zero approving
  reviews.
- Branch protection matches checks by job `name`. A worker cannot see branch
  protection, so the lead checks any PR that renames a job, moves a check to
  another workflow, or changes triggers against
  `gh api repos/arvinduh/formality/branches/main/protection` before merging.
- Merge with `gh pr merge --squash --delete-branch`.
- No shared `CARGO_TARGET_DIR` is configured: each worktree builds its own
  multi-GB `target/`, which goes away only with the worktree.

## Smart Format

`fml fmt` must leave files that pass trivial lint checks. Mechanical fixes
(import sorting, structural markdownlint fixes) belong in
`LanguageSurface::format()`; `fml lint` is semantic analysis only.

## Layout

- `src/config` — `formality.toml` parsing, resolution, schema
- `src/engine` — execution, diffing, update checks
- `src/surfaces` — one file per language; `docs/new-surface-guide.md` adds one
- `src/ui` — table rendering
- `src/commands` — CLI subcommand handlers

## Issues

Workflow state is derived from GitHub
([ADR 0006](docs/adr/0006-derived-issue-state.md)). Topical labels:
`architecture`, `dx`, `documentation`, `rust`, `ci`, `compatibility`, `surface`,
`bug`, `enhancement`.

Issue and PR numbers cited in code or docs written before 2026-08-26 predate the
repo's recreation and point at unrelated issues; see
`docs/INDEX.md#note-on-pre-recreation-issuepr-numbers`.

## Ask first

- Branch protection or required status check names.
- Version bumps: hand edits in a dedicated `chore(release)` PR
  (`docs/release.md`). `Cargo.toml` and `editors/vscode/package.json` move
  together (`tests/version_lockstep.rs`). The `semver` crate in `Cargo.toml`
  parses external tools' versions, not formality's own.

## Never

- Commit directly to `main`.
