# Contributing

## Setup

Install the Rust toolchain that `rust-toolchain.toml` pins, and the tools fml
wraps at the versions CI uses:

```sh
.github/install-tools.sh
```

## Checks

A PR is ready when these pass:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs them on Linux for each PR, and the tests run on Windows and macOS before
each release. CI does not check the repo's Markdown, TOML or YAML; `fml fmt` and
`fml lint` do, locally.

## Code

- Read [docs/architecture.md](docs/architecture.md) for where code belongs.
- Import modules, not items, and call through them: `use std::path;` then
  `path::Path`. Traits may be imported by name.
- Document every item.
- Keep commits small and building: each one passes clippy, fmt and the tests.

## Commits and PRs

Commits and PR titles follow Conventional Commits with a scope: `type(scope):
summary`, such as `fix(surface): ...` or `ci(release): ...`. PRs are
squash-merged, so the PR title becomes the commit on `main`.
