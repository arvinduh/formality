# AGENTS.md

fml formats and lints every file in a repo with one tool per language. See
[docs/architecture.md](docs/architecture.md) for the crates and their rules, and
[CONTRIBUTING.md](CONTRIBUTING.md) for the checks and code conventions.

## Toolchain

`rust-toolchain.toml` pins stable `1.98.1`, and CI installs it from there. fml
must build on stable.

## Compatibility

Until 1.0.0 nothing keeps backwards compatibility, CLI flags and config keys
included: no shims, deprecated aliases or migration paths. Removing something
removes it outright.

## CI and merging

- `.github/workflows/ci.yml` runs `cargo fmt --check`, clippy and the tests on
  Linux for each PR. Branch protection requires its job name, `Tests
  (ubuntu-latest)`, so renaming it needs the same change there.
- Windows and macOS run the tests only at release, in each build job
  (`.github/build-setup.yml`). A failure there blocks the release.
- PRs are squash-merged.
- `.github/install-tools.sh` pins every external tool CI uses.

## Releases

Bare `x.y.z` tags, never `vx.y.z`. See [docs/release.md](docs/release.md).

## Ask first

- Branch protection or required check names.
- Version bumps and release tags.

## Never

- Commit directly to `main`.
