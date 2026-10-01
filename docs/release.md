# Release Procedure

This document describes how a release of `fml` is cut. `version` in `Cargo.toml`
and `editors/vscode/package.json` is kept in lockstep by
`tests/version_lockstep.rs`; the bump lands on `main` in its own
`chore(release)` PR, and pushing a matching `vX.Y.Z` tag to `main` drives the
build-and-publish pipeline
([cargo-dist](https://opensource.axo.dev/cargo-dist/)).

## Overview

Releases are cut from `main` and are driven by
[Conventional Commits](https://www.conventionalcommits.org/). Every commit
merged to `main` should follow the `<type>(<scope>): <description>` format
already used throughout this repository's history (see `git log`). GitHub's
`--generate-notes` groups the merged PRs into the release body, and the commit
types are what the semver bump in step 2 is read off (`feat` -> minor, `fix` ->
patch, `!`/`BREAKING CHANGE` -> major). That bump is a hand-edit in its own
`chore(release)` PR — there is no automated version-bump tool.

The binary release pipeline is
[cargo-dist](https://opensource.axo.dev/cargo-dist/):
`[workspace.metadata.dist]` in `Cargo.toml` is the source of truth for targets,
installers, and the dist version, and it generates
`.github/workflows/release.yml`. There is **no** crates.io publish and **no**
committed `CHANGELOG.md`.

## Prerequisites

- [`dist`](https://opensource.axo.dev/cargo-dist/) installed locally for
  previewing what a tag will build:

  ```sh
  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v0.32.0/cargo-dist-installer.sh | sh
  dist plan
  ```

  (CI installs its own pinned copy — the version in `[workspace.metadata.dist]`
  `cargo-dist-version` — so a local install is only for previewing.)

- Push access to `main` and permission to push tags.
- The commit history on `main` since the last tag should already follow
  Conventional Commits so the generated release notes read cleanly.

## Steps

1. **Confirm `main` is releasable.**

   ```sh
   git checkout main
   git pull
   cargo test
   cargo clippy -q
   cargo run -q -- fmt --check
   cargo run -q -- lint
   ```

   `sync --check` is excluded from the root because the repo root carries only
   `formality.toml` without native config files (`.rustfmt.toml`, `.prettierrc`,
   etc.).

2. **Bump the version.**

   Update `version` in `Cargo.toml` and `editors/vscode/package.json` together
   (they must stay identical — `tests/version_lockstep.rs` enforces this).
   Follow semver based on the changes since the last tag: any `feat` commit
   means at least a minor bump, any breaking change means a major bump,
   otherwise a patch bump. Commit this as its own
   `chore(release): bump version to vX.Y.Z` commit.

3. **Preview what the tag will build.**

   ```sh
   dist plan
   ```

   Confirm the three target archives (`fml-<target>.tar.xz` / `.zip` for
   `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` and
   `x86_64-pc-windows-msvc`), the two installers (`fml-installer.sh`,
   `fml-installer.ps1`), and the checksum files are listed.

4. **Tag the release.**

   ```sh
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin vX.Y.Z
   ```

   Pushing the tag triggers two workflows in parallel:

   `.github/workflows/release.yml` (cargo-dist), which:
   - Builds the `fml` binary for Linux (x86_64), macOS (aarch64), and Windows
     (x86_64), packaging each as `fml-<target>.tar.xz` (`.zip` on Windows) with
     a `.sha256` sidecar.
   - Builds the `shell` / `powershell` installers and a combined `sha256.sum`.
   - Creates the GitHub Release for the tag with
     `gh release create --generate-notes` (GitHub groups the merged PRs into the
     body, starting from the previous `v*` tag so an `s*` schema release
     published in between can't widen the range) and marks it the latest
     release, so `/releases/latest/download/...` resolves here.

   `.github/workflows/release-extras.yml`, which:
   - Builds the VS Code extension `.vsix` package.
   - Generates `schema/formality.schema.json` from the built binary.
   - Waits for the release above to exist, then **appends** the `.vsix` and the
     JSON schema to it as assets (it never creates the release itself — that
     would race dist).

5. **Verify the published release.**

   Check the [Releases page](https://github.com/arvinduh/formality/releases) for
   the new tag: confirm all three platform archives, the two installers, the
   checksum files, the `.vsix`, and `schema/formality.schema.json` are attached,
   and that the release notes look correct.

6. **Announce / update references.**

   If anything (docs, `#:schema` directives in example `formality.toml` files,
   install instructions) references a specific release URL or version number,
   update those references to point at the new tag. Users can run `fml init` to
   rewrite their own project's `#:schema` directive to the new tag without
   hand-editing it.

## Schema Releases (`s*` tags)

In addition to binary releases (`v*`), `fml` supports independent schema
releases tagged with the `s{major}.{minor}` pattern (e.g. `s1.0`, `s1.1`,
`s2.0`). A major bump means a breaking schema change; a minor bump means an
additive/compatible one. This is deliberately independent of the binary's
`v{semver}` tag — the two change at different rates, and forcing them to track
each other (e.g. `s0.1.0` mirroring `v0.1.0`) would either churn the schema tag
on every binary release or let it silently drift out of a parity it never really
had. See [`SchemaVersion`](../src/config/schema.rs) and
[ADR 0003](adr/0003-two-tag-release-versioning.md) for the original design
rationale.

Schema releases publish `schema/formality.schema.json` as an independent GitHub
Release asset under the corresponding `s{major}.{minor}` tag so users can pin
their `formality.toml` or `.formality.toml` configuration files to stable schema
versions via `#:schema` directives:

```toml
#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json
```

### Latest Release Invariant

Because binary releases (`v*`) and schema releases (`s*`) share the same GitHub
Releases space, workflow configuration enforces a strict invariant:

- **Binary releases (`v*`)**: cargo-dist creates the release without
  `--prerelease` for a plain `vX.Y.Z` tag, so GitHub marks it the latest
  release. This keeps GitHub's `/releases/latest` endpoint, the prebuilt
  download URLs (`/releases/latest/download/...`), and the dist installer assets
  (`fml-installer.sh` / `fml-installer.ps1`) resolving to the most recent binary
  release. A `vX.Y.Z-rc.N` tag is published as a prerelease and does not move
  the latest pointer.
- **Schema releases (`s*`)**: Explicitly set `make_latest: false` in
  `.github/workflows/schema-release.yml`. This ensures publishing an independent
  schema tag (e.g. `s1.0`, `s1.1`) never overtakes the latest binary release or
  breaks binary downloads. `s*` tags do not match `release.yml`'s tag filter, so
  cargo-dist never runs for them.

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
   `fml init` rewrites their `#:schema` line to the current tag.

## Release notes

Release notes are produced by GitHub's own `gh release create --generate-notes`
in `.github/workflows/release.yml` (the cargo-dist `host` job). GitHub lists the
pull requests merged since the previous release and links each contributor.
There is no committed `CHANGELOG.md` and no `git-cliff` step: the generated
release body _is_ the changelog. Its absence from the repo root is deliberate,
not an oversight — nothing writes or reads a checked-in changelog file, so there
is no file to keep current between releases.

Keeping PR titles in Conventional Commits form
(`<type>(<scope>): <description>`) is what makes the generated notes readable,
and is what the manual semver decision in step 2 is based on. Adding a
`.github/release.yml` would let GitHub group those PRs into labelled sections;
no such file exists today, so the notes use GitHub's default grouping.

## Regenerating `release.yml` and local edits

The release workflow (`.github/workflows/release.yml`) is generated from
`[workspace.metadata.dist]` in `Cargo.toml` by `dist generate --mode=ci`. It
carries four hand-applied local edits, each marked with a
`# LOCAL EDIT (issue #N)` comment that explains it:

1. **Tag glob constrained to a leading `v`** (`- 'v[0-9]+.[0-9]+.[0-9]+*'`,
   issue #134) so independent schema releases (`s*` tags handled by
   `schema-release.yml`) cannot trigger binary release builds.
2. **`fetch-depth: 0` on the `host` job checkout** (issue #134) so the full tag
   history is available for `--notes-start-tag`.
3. **`gh release create --generate-notes --notes-start-tag`** (issue #134)
   instead of dist's default `--notes-file` changelog body. Because this
   repository has no committed `CHANGELOG.md`, reverting to `--notes-file` would
   silently publish releases with an empty body.
4. **ARM64 note in the PowerShell installer** (issue #166): a step in the
   `build-global-artifacts` job, after `cargo-dist` and before
   `Upload artifacts`, that patches a note into `fml-installer.ps1` saying ARM64
   Windows gets the x64 build on purpose.

### `allow-dirty` makes regeneration a no-op

`Cargo.toml` sets `allow-dirty = ["ci"]` so cargo-dist accepts the edited
workflow. With that setting, `dist generate --mode=ci` (with or without
`--allow-dirty`) leaves `release.yml` untouched and prints nothing, so a
`cargo-dist-version` bump alone never reaches the workflow.

### Regeneration procedure

When `[workspace.metadata.dist]` or `cargo-dist-version` changes:

1. Save a copy of the current `release.yml`.
2. Comment out `allow-dirty = ["ci"]` in `Cargo.toml`, then run
   `dist generate --mode=ci`. This writes the pristine template, without any
   local edit.
3. Diff the pristine output against the saved copy. Every hunk outside the four
   `# LOCAL EDIT` sites is a real template change; Dependabot's `actions/*`
   version bumps also show up here and are kept.
4. Re-apply all four local edits in their places, restore `allow-dirty`, and run
   the guard test:

   ```sh
   cargo test --test release_workflow_local_edits
   ```

### The local-edits guard test

`tests/release_workflow_local_edits.rs` guards against accidental reversion. It
asserts that:

- All required substrings are present on live (non-comment) lines in
  `release.yml`.
- Reverted dist-generated defaults (such as `--notes-file` or prefix-less globs)
  are absent.
- Each edit sits in the job, and the step order, it only works in.
- The number of `# LOCAL EDIT (issue #N)` marker comments matches the expected
  edit count, ensuring each edit remains documented with re-application
  instructions.

The guard runs in the `Library Tests` CI job (`cargo test --verbose`), one of
the repository's required status checks, ensuring that any regeneration dropping
a local edit fails PR checks rather than surfacing at release time.

If a future cargo-dist version makes an edit unnecessary, delete the edit from
`release.yml` and drop its corresponding entry from `EDITS` in
`tests/release_workflow_local_edits.rs` in the same commit.
