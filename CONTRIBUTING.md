# Contributing

## Setup

Install the Rust toolchain that `rust-toolchain.toml` pins, and the tools fml
wraps at the versions CI uses:

```sh
.github/install-tools.sh
git config core.hooksPath .githooks
```

The pre-commit hook builds fml and runs `fml fmt --staged` and
`fml lint --staged` on the commit.

## Checks

A PR is ready when these pass:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -q -- fmt --check
cargo run -q -- lint
```

Always run the freshly built binary with `cargo run`, never an `fml` on
`PATH`. CI runs the tests on Linux, macOS and Windows.

## Code

- Read [docs/architecture.md](docs/architecture.md) for where code belongs.
- Import modules, not items, and call through them: `use std::path;` then
  `path::Path`. Traits may be imported by name.
- Document every item.
- Keep commits small and building: each one passes clippy, fmt and the tests.

## Commits and PRs

Commits and PR titles follow Conventional Commits with a scope:
`type(scope): summary`, such as `fix(surface): ...` or `ci(release): ...`.
PRs are squash-merged, so the PR title becomes the commit on `main`.
