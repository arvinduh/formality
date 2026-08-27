# `fml table` — Semantic JSON Table Specification

`fml table` renders an opinionated, terminal-aware table from a JSON
specification. It exists so any script or tool (not just `fml`'s own internal
output — see `fml doctor` / `fml list-surfaces`, which render via the same
`src/ui/table` machinery) can get formality's table styling (semantic color
roles, width policies, wrapping/truncation, terminal-width clamping) without
reimplementing a table renderer.

```bash
# From a JSON string argument:
fml table --json '{"columns": [...], "rows": [...]}'

# From stdin (no --json flag):
echo '{"columns": [...], "rows": [...]}' | fml table
```

Output goes to stdout; exit code `0` on success, `2` if the JSON fails to parse
or doesn't match the schema below.

The Rust types backing this spec live in `src/ui/table/mod.rs` and
`src/ui/table/render.rs` — this document mirrors their `Serialize`/
`Deserialize` shape (`#[serde(rename_all = "snake_case")]` throughout).

## Top-level shape

```jsonc
{
  "columns": [/* Column[] */],
  "rows": [/* Row[] */],
  "layout": {/* Layout, optional */},
}
```

## `Column`

```jsonc
{
  "header": { "spans": [{ "text": "Surface", "style": "strong" }] }, // Cell
  "align": "left", // "left" | "center" | "right" — default "left"
  "width": "auto", // WidthPolicy — default "auto"
  "overflow": "wrap", // Overflow — default "wrap"
  "priority": 0, // u8 — higher-priority columns are kept longest when clamping to terminal width
}
```

`WidthPolicy` variants:

| Variant        | JSON form            | Meaning                                   |
| :------------- | :------------------- | :---------------------------------------- |
| Auto (default) | `"auto"`             | Size to content, subject to clamping      |
| Fixed          | `{"fixed": 14}`      | Always exactly this many columns wide     |
| Min            | `{"min": 8}`         | Never narrower than this                  |
| Max            | `{"max": 40}`        | Never wider than this                     |
| Range          | `{"range": [8, 40]}` | Clamp to `[min, max]`                     |
| Percent        | `{"pct": 25}`        | Percentage of total available table width |

`Overflow` variants:

| Variant        | JSON form                         | Meaning                                           |
| :------------- | :-------------------------------- | :------------------------------------------------ |
| Wrap (default) | `"wrap"`                          | Word-wrap content onto additional lines           |
| Truncate       | `{"truncate": {"suffix": "..."}}` | Cut content and append `suffix` (default `"..."`) |
| Clip           | `"clip"`                          | Hard-cut with no suffix                           |

## `Row`

```jsonc
{
  "cells": [/* Cell[] */],
  "max_height": 3, // optional cap on wrapped-line count for this row
  "kind": "data", // RowKind — default "data"
}
```

`RowKind` variants: `"data"` (default), `"rule"` (horizontal separator, no cells
needed), `"blank"` (empty spacer row), or `{"group": "Section title"}` (a
group-header row rendered as its own banner, e.g. to break tool output into
named sections).

## `Cell`

```jsonc
{
  "spans": [{ "text": "rustfmt", "style": "tool" }],
  "align": "left", // optional per-cell override of the column's align
  "overflow": "wrap", // optional per-cell override of the column's overflow
}
```

## `Span`

```jsonc
{ "text": "OK", "style": "ok" }
```

`Style` (semantic, not raw color — the active `Palette` maps each to an actual
ANSI/truecolor code, or no color at all if disabled):

| Style    | Typical use                                                  |
| :------- | :----------------------------------------------------------- |
| `plain`  | Default, unstyled text (the default when `style` is omitted) |
| `dim`    | De-emphasized / secondary text                               |
| `strong` | Emphasis, headers                                            |
| `path`   | File and directory paths                                     |
| `tool`   | Tool/binary names                                            |
| `ok`     | Success / pass status                                        |
| `warn`   | Warning / violation status                                   |
| `error`  | Error / failure status                                       |
| `info`   | Informational notes                                          |

## `Layout`

```jsonc
{
  "max_width": 100, // u16, default 100
  "clamp_to_terminal": true, // bool, default true — shrink to actual terminal width when narrower
  "padding": [1, 1], // [left, right] cell padding, default [1, 1]
  "density": "compact", // "compact" (default) | "comfortable"
  "indent": 0, // u16 left-indent applied to the whole table
}
```

Color output itself is controlled by `Palette::detect()` at the process level
(not part of the JSON spec): it honors `NO_COLOR`, `FORCE_COLOR`,
`CLICOLOR_FORCE`, `COLORTERM`, `GITHUB_ACTIONS`, and falls back to no color when
stdout isn't a TTY.

## Full example

```bash
fml table --json '{
  "columns": [
    {"header": {"spans": [{"text": "Surface", "style": "strong"}]}, "width": {"fixed": 12}},
    {"header": {"spans": [{"text": "Status", "style": "strong"}]}, "width": {"fixed": 10}},
    {"header": {"spans": [{"text": "Tool", "style": "strong"}]}}
  ],
  "rows": [
    {"cells": [
      {"spans": [{"text": "rust"}]},
      {"spans": [{"text": "OK", "style": "ok"}]},
      {"spans": [{"text": "rustfmt", "style": "tool"}]}
    ]},
    {"cells": [
      {"spans": [{"text": "python"}]},
      {"spans": [{"text": "WARN", "style": "warn"}]},
      {"spans": [{"text": "ruff", "style": "tool"}]}
    ]},
    {"kind": "rule", "cells": []},
    {"cells": [
      {"spans": [{"text": "go"}]},
      {"spans": [{"text": "MISSING", "style": "error"}]},
      {"spans": [{"text": "golangci-lint", "style": "tool"}]}
    ]}
  ],
  "layout": {"max_width": 80}
}'
```

This is the same rendering path formality's own commands use internally — e.g.
`fml list-surfaces` builds a `Table` value in `src/commands/surfaces.rs` and
renders it exactly as `fml table` would from equivalent JSON.
