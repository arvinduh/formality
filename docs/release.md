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
     body, starting from the previous `v*` tag) and marks it the latest release,
     so `/releases/latest/download/...` resolves here.

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

   If anything (docs, install instructions) references a specific release URL or
   version number, update those references to point at the new tag.

## The JSON schema

There is one schema, the one matching the latest release. It ships only as the
`formality.schema.json` asset of each `v*` release, and `fml init` writes a
`#:schema` directive that follows the latest release:

```toml
#:schema https://github.com/arvinduh/formality/releases/latest/download/formality.schema.json
```

A plain `vX.Y.Z` tag becomes GitHub's latest release, so this URL, the prebuilt
downloads and the dist installers all move to it together. A `vX.Y.Z-rc.N` tag
is published as a prerelease and moves none of them.

Before 1.0.0 the schema makes no compatibility promise
([ADR 0007](adr/0007-one-current-schema.md)): a config a newer `fml` no longer
accepts fails with the key path, line and a fix-it hint instead.
`tests/schema_drift.rs` keeps `schema/formality.schema.json` equal to
`fml schema` output; regenerate it with
`UPDATE_SCHEMA=1 cargo test --test schema_drift`.

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
   issue #134) so only binary release tags trigger a build, matching
   `release-extras.yml`'s filter.
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
- Every cargo-dist installer that `release.yml` downloads is exactly the
  `cargo-dist-version` pinned in `Cargo.toml`. Because `allow-dirty` makes
  regeneration a no-op, a pin bump fails this check until the workflow is
  regenerated (issue #421).
- The `push.tags` filters of `release.yml` and `release-extras.yml` are
  identical (issue #165).

The guard runs in the `Library Tests` CI job (`cargo test --verbose`), one of
the repository's required status checks, ensuring that any regeneration dropping
a local edit fails PR checks rather than surfacing at release time.

If a future cargo-dist version makes an edit unnecessary, delete the edit and
its `# LOCAL EDIT` comment from `release.yml`, drop its corresponding entry from
`EDITS` in `tests/release_workflow_local_edits.rs`, and remove it from the list
of local edits at the top of this section, lowering each "four" that counts
them, all in one commit.
