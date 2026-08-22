# Compatibility Matrix

This document outlines the compatibility between `fml` CLI binary versions and
versioned JSON Schema releases (`s{N}`).

## Schema Versioning Overview

`fml` uses independent schema releases tagged as `s{N}` (e.g., `s1`, `s2`)
alongside CLI binary releases tagged as `v*` (e.g., `v0.1.0`).

- **Binary Releases (`v*`)**: Target multi-platform executable builds for the
  `fml` CLI and editor plugins.
- **Schema Releases (`s*`)**: Target `schema/formality.schema.json`, published
  independently so project configuration files can reference a stable schema
  version via `#:schema` directives.

### Referencing Schema Releases

In `formality.toml` or `.formality.toml`, reference a specific schema tag
(`s{N}`):

```toml
#:schema https://github.com/arvinduh/formality/releases/download/s1/formality.schema.json

[global]
indent_size = 2
line_length = 80
```

## Version Compatibility Matrix

| `fml` Binary Version | Recommended Schema Tag | Schema Release URL                                                                 | Status & Notes                                                                          |
| :------------------- | :--------------------- | :--------------------------------------------------------------------------------- | :-------------------------------------------------------------------------------------- |
| `v0.1.x`             | `s1`                   | `https://github.com/arvinduh/formality/releases/download/s1/formality.schema.json` | Active (Initial schema revision covering canonical globals and per-language overrides). |

## Compatibility Guarantees

1. **Backwards Compatibility**: Newer `fml` binaries retain backwards
   compatibility with configurations valid under older `s{N}` schema releases.
2. **Schema Evolution**: Non-breaking schema additions (such as adding new
   optional surface settings) remain within the active `s{N}` tag range.
   Breaking schema structure changes trigger a bump to the next schema tag
   (e.g., `s2`).
