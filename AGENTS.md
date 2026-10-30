# AGENTS.md

fml formats and lints every file in a repo with one tool per language. See
[docs/architecture.md](docs/architecture.md) for the crates and their rules,
and [CONTRIBUTING.md](CONTRIBUTING.md) for the checks and code conventions.

## Toolchain

`rust-toolchain.toml` pins stable `1.98.1`, and CI pins the same version. fml
must build on stable.

## Compatibility

Until 1.0.0 nothing keeps backwards compatibility, CLI flags and config keys
included: no shims, deprecated aliases or migration paths. Removing something
removes it outright.

## CI and merging

- `.github/workflows/pr-check.yml` runs the tests on Linux (`Library Tests`),
  macOS and Windows, plus `Formality Dogfooding` (`fml fmt --check` and
  `fml lint` on this repo) and `Security Audit`.
- Branch protection on `main` requires `Library Tests` and
  `Formality Dogfooding` by job name, so renaming either needs the same change
  in branch protection.
- PRs are squash-merged.
- `.github/install-tools.sh` pins every external tool CI uses.

## Releases

Bare `x.y.z` tags, never `vx.y.z`. See [docs/release.md](docs/release.md).

## Ask first

- Branch protection or required check names.
- Version bumps and release tags.

## Never

- Commit directly to `main`.
