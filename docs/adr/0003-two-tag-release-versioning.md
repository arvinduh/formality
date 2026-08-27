# 0003 — Independent `v*`/`s*` release tags instead of a single unified tag

> `#N` citations below predate the 2026-08-26 repo recreation and no longer
> resolve — see
> [`docs/INDEX.md`](../INDEX.md#note-on-pre-recreation-issuepr-numbers).

**Status:** Accepted **Decided in:** `#126`, landed via PR `#139`.

## Context

Originally, `fml`'s binary release, the VS Code extension, and
`schema/formality.schema.json` were all coupled to the same `v{semver}` tag —
`fml init`'s generated `#:schema` directive built its URL from
`env!("CARGO_PKG_VERSION")`, the binary's own version. That meant a schema-only
fix (e.g. adding one optional field) couldn't ship without cutting a full binary
release, and `Cargo.toml` staying pinned at `0.1.0` (no tag past `v0.1.0` had
ever been pushed) meant every `fml init` generated a stale schema reference.

The design conversation in `#126` considered decoupling _three_ things onto
independent tags: the binary, the VS Code extension, and the schema. It landed
on two, not three: `tests/version_lockstep.rs` already deliberately enforces
`Cargo.toml`'s version matching `editors/vscode/package.json`'s version — an
existing, intentional invariant — and the repo owner confirmed keeping
binary+extension coupled rather than adding a third independent
`vscode-v*`-style tag. Only the schema was pulled out. The tag prefix itself was
also simplified during that conversation, from an earlier `schema-v{N}` draft to
the shorter, `v{semver}`-parallel `s{major}.{minor}`.

## Decision

Two independent tag namespaces, not one and not three:

- **`v{semver}`** (e.g. `v0.1.0`) — the binary release _and_ the VS Code
  extension together, kept coupled via `tests/version_lockstep.rs`.
- **`s{major}.{minor}`** (e.g. `s1.0`, `s1.1`, `s2.0`) —
  `schema/formality.schema.json` releases, independent of binary release
  cadence. `major` bumps on a breaking schema change, `minor` on an
  additive/compatible one.

See [release.md](../release.md) for the full cutting procedure for each tag
type, and [compatibility.md](../compatibility.md) for the binary-version-to-
schema-version compatibility matrix this split makes necessary.

## Consequences

- A schema-only fix (new optional field, docs correction) can ship as an `s*`
  tag without waiting for or forcing an unrelated binary release.
- Users pin `#:schema` directives to a specific `s{major}.{minor}` release asset
  URL rather than a raw branch file; `fml migrate schema` rewrites a project's
  `#:schema` line to the current tag without hand-editing.
- The extension stays coupled to the binary's `v*` tag — adding a genuinely
  independent extension release cadence later would be a new decision, not a
  natural extension of this one, since `#126` explicitly considered and rejected
  that shape for now.
- `docs/compatibility.md` and `docs/release.md` both need updating whenever a
  new schema major/minor is cut — see [release.md](../release.md)'s "Update
  documentation & matrix" step.
