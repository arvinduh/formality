# Compatibility Matrix

This document outlines the compatibility between `fml` CLI binary versions and
versioned JSON Schema releases (`s{major}.{minor}`).

## Schema Versioning Overview

`fml` uses independent schema releases tagged as `s{major}.{minor}` (e.g.,
`s1.0`, `s1.1`, `s2.0`) alongside CLI binary releases tagged as `v*` (e.g.,
`v0.1.0`). The two counters are deliberately independent, not mirrored — a
schema change and a binary release happen at different rates, so a schema tag
that tried to track the binary's semver (e.g. `s0.1.0` mirroring `v0.1.0`) would
either need bumping on every unrelated binary release or silently drift out of a
parity it never really had. `major` bumps on a breaking schema change; `minor`
bumps on an additive/compatible one.

- **Binary Releases (`v*`)**: Target multi-platform executable builds for the
  `fml` CLI and editor plugins.
- **Schema Releases (`s*`)**: Target `schema/formality.schema.json`, published
  independently so project configuration files can reference a stable schema
  version via `#:schema` directives.

### Referencing Schema Releases

In `formality.toml` or `.formality.toml`, reference a specific schema tag
(`s{major}.{minor}`):

```toml
#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json

[global]
indent_size = 2
line_length = 80
```

## Version Compatibility Matrix

Each binary expects the schema tag in its `SCHEMA_VERSION`
(`src/config/schema.rs`) and warns when a `#:schema` directive names an older
one.

| `fml` Binary Version | Expected Schema Tag | Notes                                   |
| :------------------- | :------------------ | :-------------------------------------- |
| `v0.1.0`             | none                | Predates the `#:schema` version check.  |
| `v0.2.0`, `v0.2.1`   | `s1.1`              | Schema release `s1.1` is published.     |
| unreleased `main`    | `s1.3`              | No `s1.3` schema release is tagged yet. |

## Compatibility Guarantees

1. **Backwards Compatibility**: None before fml's first official release
   ([AGENTS.md](../AGENTS.md#compatibility)). From that release on, newer `fml`
   binaries retain backwards compatibility with configurations valid under older
   `s{major}.{minor}` schema releases.
2. **Schema Evolution**: Non-breaking schema additions (such as adding new
   optional surface settings) bump the `minor` component (e.g. `s1.0` -> `s1.1`)
   and stay within the active major range. Breaking schema structure changes
   bump `major` and reset `minor` to `0` (e.g. `s1.5` -> `s2.0`).
