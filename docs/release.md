# Release Procedure

This document describes how a release of `fml` is cut. Versioned releases are
live: several `v*` releases have already shipped, each with published binaries
for five targets, a generated changelog, and attached schema/extension assets.
The mechanics below — changelog generation, tagging, and publishing — are in
continuous use, and this is the document to follow before cutting the next one.

For the current version, read `version` in `Cargo.toml` (do not restate it here
— it goes stale). `editors/vscode/package.json` is kept identical to it,
enforced by `tests/version_lockstep.rs`.

The version bump lands as its own `chore(release): bump version to X.Y.Z` commit
through a pull request rather than a direct push to `main`; pushing the matching
`vX.Y.Z` tag afterward is what drives changelog generation and publishing.

## Overview

Releases are cut from `main` and are driven by
[Conventional Commits](https://www.conventionalcommits.org/). Every commit
merged to `main` should follow the `<type>(<scope>): <description>` format
already used throughout this repository's history (see `git log`). This lets
[git-cliff](https://git-cliff.org/) derive a structured changelog directly from
commit messages, and makes the right semver bump for a release obvious (`feat`
-> minor, `fix` -> patch, `!`/`BREAKING CHANGE` -> major).

## Prerequisites

- [`git-cliff`](https://git-cliff.org/) installed locally for previewing
  changelog output:

  ```sh
  cargo install git-cliff --locked
  ```

  (CI does not require a local install — the workflow below runs it via the
  `orhun/git-cliff-action` GitHub Action.)

- Push access to `main` and permission to push tags.
- The commit history on `main` since the last tag should already follow
  Conventional Commits — `git cliff --unreleased` will silently skip anything
  that doesn't parse, so a noisy changelog usually means a commit message needs
  fixing before tagging, not after.

## Steps

1. **Confirm `main` is releasable.**

   ```sh
   git checkout main
   git pull
   cargo test --lib -q
   cargo clippy -q
   cargo run -q -- fmt --check
   cargo run -q -- sync --check
   cargo run -q -- lint
   ```

2. **Bump the version.**

   Update `version` in `Cargo.toml` and `editors/vscode/package.json` together
   (they must stay identical — `tests/version_lockstep.rs` enforces this).
   Follow semver based on the changes since the last tag: any `feat` commit
   means at least a minor bump, any breaking change means a major bump,
   otherwise a patch bump. This lands as its own
   `chore(release): bump version to X.Y.Z` commit through a pull request, not a
   direct push to `main`.

3. **Preview the changelog.**

   ```sh
   git cliff --unreleased --tag vX.Y.Z
   ```

   Review the output for accuracy. If a commit is miscategorized, that usually
   means its type/scope prefix was wrong — fix it going forward rather than
   trying to edit history.

4. **Do not commit a changelog.**

   `CHANGELOG.md` is deliberately **not** checked into the repo. It is generated
   fresh at release time by `.github/workflows/release.yml` (via
   `orhun/git-cliff-action` with `--latest --strip header`) when the tag is
   pushed, and published two ways: as the GitHub Release body and as a
   `CHANGELOG.md` file attached to the release as an asset. The local
   `git cliff` invocation in step 3 is a preview only — there is no committed
   copy to update, and adding one would immediately drift from the release-time
   output.

5. **Tag the release.**

   ```sh
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin vX.Y.Z
   ```

   Pushing the tag triggers `.github/workflows/release.yml`, which:
   - Builds the `fml` binary for Linux (x86_64, aarch64), macOS (x86_64,
     aarch64), and Windows (x86_64) and uploads each archive to the GitHub
     Release.
   - Builds the VS Code extension `.vsix` package.
   - Generates `schema/formality.schema.json` from the built binary.
   - Generates the release-scoped changelog section via `git-cliff` and attaches
     it as `CHANGELOG.md` on the release, and uses it as the GitHub Release
     body.
   - Uploads the `.vsix` and the JSON schema as release assets.
   - Sets `make_latest: true` so GitHub's latest release pointer and
     `/releases/latest/download/` URLs point to this binary release.

6. **Verify the published release.**

   Check the [Releases page](https://github.com/arvinduh/formality/releases) for
   the new tag: confirm all five platform archives, the `.vsix`, and
   `schema/formality.schema.json` are attached, and that the release notes look
   correct.

7. **Announce / update references.**

   If anything (docs, `#:schema` directives in example `formality.toml` files,
   install instructions) references a specific release URL or version number,
   update those references to point at the new tag. Users can run
   `fml migrate schema` to rewrite their own project's `#:schema` directive to
   the new tag without hand-editing it.

## Schema Releases (`s*` tags)

In addition to binary releases (`v*`), `fml` supports independent schema
releases tagged with the `s{major}.{minor}` pattern (e.g. `s1.0`, `s1.1`,
`s2.0`). A major bump means a breaking schema change; a minor bump means an
additive/compatible one. This is deliberately independent of the binary's
`v{semver}` tag — the two change at different rates, and forcing them to track
each other (e.g. `s0.1.0` mirroring `v0.1.0`) would either churn the schema tag
on every binary release or let it silently drift out of a parity it never really
had. See [`SchemaVersion`](../src/config/schema.rs) and
[ADR 0003](adr/0003-two-tag-release-versioning.md) for the design rationale.

Schema releases publish `schema/formality.schema.json` as an independent GitHub
Release asset under the corresponding `s{major}.{minor}` tag so users can pin
their `formality.toml` or `.formality.toml` configuration files to stable schema
versions via `#:schema` directives:

```toml
#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json
```

### Latest Release Invariant (`make_latest`)

Because binary releases (`v*`) and schema releases (`s*`) share the same GitHub
Releases space, workflow configuration enforces a strict invariant:

- **Binary releases (`v*`)**: Explicitly set `make_latest: true` in
  `.github/workflows/release.yml`. This ensures GitHub's `/releases/latest`
  endpoint, prebuilt binary download URLs (`/releases/latest/download/...`), and
  install scripts (`install.sh`, `install.ps1`) always resolve to the most
  recent binary release.
- **Schema releases (`s*`)**: Explicitly set `make_latest: false` in
  `.github/workflows/schema-release.yml`. This ensures publishing an independent
  schema tag (e.g. `s1.0`, `s1.1`) never overtakes the latest binary release or
  breaks binary downloads.

### Schema Release Procedure

1. **Verify schema freshness on `main`.**

   ```sh
   git checkout main
   git pull
   cargo test --test schema_drift
   ```

2. **Tag the schema release.**

   ```sh
   git tag -a s1.0 -m "s1.0 schema release"
   git push origin s1.0
   ```

3. **CI Automation.**

   Pushing an `s*` tag triggers `.github/workflows/schema-release.yml`, which:
   - Builds `fml` from the tagged commit.
   - Generates `schema/formality.schema.json`.
   - Creates a GitHub Release for tag `s{major}.{minor}` and uploads
     `formality.schema.json` as a release asset.

4. **Verify the schema release.**

   Check the GitHub Releases page for tag `s{major}.{minor}` and confirm that
   `formality.schema.json` is attached to the release.

5. **Update documentation & matrix.**

   Update [compatibility.md](compatibility.md) and example `#:schema` directives
   in documentation if a new schema version (e.g. `s1.1` or `s2.0`) was cut.
   Individual users don't need to hand-edit their own `formality.toml` —
   `fml migrate schema` rewrites their `#:schema` line to the current tag.

## Changelog conventions

`cliff.toml` at the repository root controls how `git-cliff` groups and formats
commits. Commit types map to changelog sections as follows:

| Commit type      | Changelog section        |
| ---------------- | ------------------------ |
| `feat`           | Features                 |
| `fix`            | Bug Fixes                |
| `perf`           | Performance              |
| `refactor`       | Refactor                 |
| `doc`/`docs`     | Documentation            |
| `style`          | Styling                  |
| `test`           | Testing                  |
| `build`          | Build                    |
| `ci`             | CI/CD                    |
| `chore(release)` | (omitted from changelog) |
| `chore`          | Miscellaneous            |
| `revert`         | Reverts                  |

Commits that don't match a Conventional Commits prefix are skipped when
generating the changelog, so keeping commit messages conventional is what keeps
the changelog useful.
