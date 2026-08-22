# AGENTS.md

fml: polyglot format/lint/config orchestrator, 12 language surfaces.

## Commands

```bash
cargo test --lib -q
cargo clippy -q
cargo run -q -- fmt
cargo run -q -- sync --check
```

## Layout

- `src/config` — formality.toml parsing, resolution, schema
- `src/engine` — execution, diffing, update checks
- `src/surfaces` — one file per language; see `docs/new-surface-guide.md` to add
  one
- `src/ui` — table rendering
- `src/commands` — CLI subcommand handlers

## Conventions

- Commits: `type(scope): description (Fixes #issue)`, Conventional Commits
  style.
- Format fixes belong in `fml fmt` (mechanical); `fml lint` is semantic-only.
- Always run the freshly built binary (`cargo run -q -- ...`), never a stale
  global `fml` on PATH.
- Check `docs/` before reading source to understand structure or conventions
  already documented there — don't re-derive what's written down.

## Always

- Run `cargo test --lib -q && cargo clippy -q` before any commit.

## Ask first

- Anything touching branch protection or CI required-status-check names.
- Version bumps (owned by dedicated tooling, not hand-edits).

## Never

- Commit directly to `main`.

Multi-agent orchestration process (worktrees, QA gate, dispatch order,
design-phase and applied-feature rules): see `.agents/orchestrate.md`, or GitHub
issue #77 as the tool-agnostic mirror.
