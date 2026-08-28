# 0002 — `tokei` as a library dependency, not a subprocess, for a future `fml stat`

> `#N` citations below predate the 2026-08-26 repo recreation and no longer
> resolve — see
> [`docs/INDEX.md`](../INDEX.md#note-on-pre-recreation-issuepr-numbers).

**Status:** Superseded — `fml stat` will not be built; see
[Supersession](#supersession-2026-08-28) below. The rest of this document is
kept as the historical record of the library-vs-subprocess reasoning, per this
directory's own convention of marking a superseded file rather than deleting it.

**Original status (superseded):** Proposed — `fml stat` does not exist in the
codebase yet; this ADR backfills a decision made in design conversation ahead of
implementation, per issue `#131`.

## Context

A future `fml stat` command (referenced as an example of new user-facing CLI
surface in `.agents/orchestrate.md` §10's "Applied-feature checkpoint" rule,
alongside `fml migrate schema`) would need to count lines of code per language.
[`tokei`](https://github.com/XAMPPRocky/tokei) already does this and is the
natural tool to reach for. The choice is between shelling out to a `tokei`
binary the way `fml` shells out to per-surface tools (`rustfmt`, `ruff`,
`clang-format`, ...) via `src/surfaces/tooling.rs`, or depending on the `tokei`
crate directly as a library and calling its counting logic in-process.

## Decision

Depend on `tokei` as a library crate, not invoke it as a subprocess.

## Rationale

- Every existing `LanguageSurface` shells out to an _external_ per-language tool
  (rustfmt, clippy, ruff, ...) because those tools' own behavior is the point —
  `fml` orchestrates other projects' formatters/linters, it doesn't reimplement
  them. Line counting is different: it isn't delegating to a separate
  ecosystem's authoritative tool the way `cargo fmt` is authoritative for Rust
  formatting, so there's no equivalent reason to require a `tokei` binary be
  installed and discovered on `PATH` the way `fml install` handles the other 12
  surfaces' tools.
- A library call returns structured Rust data directly, which fits `fml`'s
  existing pattern of consuming structured output (JSON where a subprocess tool
  supports it) rather than parsing free-form CLI text — see
  [table-spec.md](../table-spec.md) for the same preference applied to
  `fml table`'s own output.
- No extra runtime dependency for users: `fml stat` would work the moment the
  `fml` binary itself is installed, with no separate `tokei` install step,
  version-compat matrix, or `MSTV` (minimum-supported-tool-version) check to
  maintain in `src/engine/version/`.

## Consequences

- Adds `tokei` as a direct `Cargo.toml` dependency, compiled into the `fml`
  binary — a binary-size and build-time tradeoff against the alternative of not
  linking it until `fml stat` is actually implemented.
- `fml stat` would not need the subprocess-discovery/version-check machinery
  `src/engine/version/` and `src/surfaces/tooling.rs` provide for the other 12
  surfaces, so implementing it doesn't extend that machinery — it's a different
  shape of command, closer to `fml table`'s self-contained rendering than to a
  `LanguageSurface`.
- Per `.agents/orchestrate.md` §10, the concrete `fml stat` proposal (example
  invocation, example output) still needs to be presented to the user for
  confirmation before implementation begins — this ADR records the
  library-vs-subprocess choice only, not the command's design.

## Note on sourcing

Unlike the other three ADRs in this directory, this one is cited from exactly
one place: issue `#131`'s own body, which names "tokei-as-library-not-subprocess
(for a future `fml stat`)" as one of four decisions "made in this session's
design conversation that currently exist nowhere durable." That is a real record
that the choice was made, by the repo owner, and it is why this ADR exists — but
it records the _conclusion_ without the reasoning, and nothing else in this
repository mentions `tokei` at all (verified against `git log --all -S tokei`, a
tree-wide grep, and GitHub issue/PR search: zero hits outside this directory).

The **Rationale** section above is therefore reconstructed from `fml`'s existing
conventions (see [language-surfaces.md](../language-surfaces.md) and
[table-spec.md](../table-spec.md)) — it is not a transcript of the original
argument. That, together with `fml stat` not existing yet and
`.agents/orchestrate.md` §10's applied-feature checkpoint still being
outstanding for it, is why this ADR's original status was `Proposed` rather than
`Accepted` — which is exactly what let it be revisited and superseded below
instead of treated as settled history.

## Supersession (2026-08-28)

Issue `#19` (`feat(dx): fml stat`, `status:design-phase`) was closed
`not_planned` during a codebase-wide efficiency/scope audit, before reaching
`.agents/orchestrate.md` §10's applied-feature checkpoint this ADR's
Consequences section flagged as still outstanding. `fml stat` will not be built,
so the library-vs-subprocess choice this ADR records no longer applies to
anything. Closing reasoning (full detail on the issue):

- The Context/Rationale above already concedes the plan was to vendor `tokei` as
  a Cargo dependency — i.e. `fml stat` would just be tokei recompiled into the
  `fml` binary, not a genuinely zero-install capability. That trades a one-time
  external install (`scc`/`tokei` are both single static binaries) for
  permanently tracking upstream tokei's language-definition updates as a
  vendored dependency, plus binary-size growth.
- The one real differentiator — `fml` already knows its own file
  discovery/exclusion rules — is a much narrower need than the full
  LOC/comment/test-ratio dashboard issue `#19` proposed, and is cheaper to serve
  later (if it ever actually matters) by exposing `fml`'s resolved file list for
  `scc --include-list` to consume than by owning a counting engine.
- `fml`'s own stated design principle — orchestrate other projects'
  best-in-class tools rather than reimplement them (README,
  `docs/facet-rosetta.md`) — argues against this ADR's premise rather than for
  it: `scc`/`tokei` are exactly that best-in-class tool for polyglot line
  counting, the same way `rustfmt` is for Rust formatting.

No replacement ADR is needed — this is a "don't build it" decision, not a
different implementation choice.
