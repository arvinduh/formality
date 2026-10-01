# 0007 — One current schema instead of versioned schema releases

**Status:** Accepted **Decided in:** #401, landed via PR #427. **Supersedes:**
[0003](0003-two-tag-release-versioning.md).

## Context

[ADR 0003](0003-two-tag-release-versioning.md) moved the JSON schema onto its
own `s{major}.{minor}` tags so a config could pin a schema release and `fml`
could warn when that pin went stale. The machinery behind it (`SCHEMA_VERSION`,
the stale/current/outdated schema status and its warnings, `schema-release.yml`
and the version-progression CI step) only pays off if an older config keeps
working with a newer `fml`. Before 1.0.0 nothing promises that (AGENTS.md,
"Compatibility"), and on 2026-10-01 the owner set 1.0.0 as the first official
release.

## Decision

- One schema, matching the current binary, published only as the
  `formality.schema.json` asset of each `v*` release. `fml init` writes
  `#:schema https://github.com/arvinduh/formality/releases/latest/download/formality.schema.json`,
  and the root `formality.toml` uses the same URL.
- No schema tags, no schema version constant, no pin comparison. A schema change
  needs no version bump; `tests/schema_drift.rs` still keeps
  `schema/formality.schema.json` equal to `fml schema` output.
- Instead of staleness warnings, `fml` rejects a config key it does not know, or
  a value of the wrong type, naming the file, key path and line and the likely
  fix (`fml --version`, `fml schema`).

## Consequences

- A config written for a different `fml` fails loudly instead of being partly
  applied.
- The editor schema follows the latest release, so a user on an older `fml` can
  see keys their binary rejects; the error points them at `fml --version`.
- The existing schema-tag GitHub releases stay in place; deleting them is the
  owner's call.
- Compatibility promises start at 1.0.0 and need their own decision then.
