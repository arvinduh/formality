# Table Specification & Library API (`fml::ui::table`)

Formality's table engine renders opinionated, terminal-aware tables from Rust
structures or JSON specifications. It exists so any tool or consumer can get
formality's table styling (semantic color roles, width policies,
wrapping/truncation, terminal-width clamping) without reimplementing a table
renderer.

## Library Usage (`fml::ui::table`)

The primary interface for table rendering is the `fml::ui::table` library
module:

```rust
use fml::ui::table::{render_json, render, Table, Column, Row, Cell, Span, Style, WidthPolicy};

// 1. Render from a JSON specification string:
let json_str = r#"{
  "columns": [{"header": {"spans": [{"text": "Surface"}]}}],
  "rows": [{"cells": [{"spans": [{"text": "Rust"}]}]}]
}"#;
let rendered = render_json(json_str).expect("Valid table JSON");
print!("{rendered}");

// 2. Or construct programmatically in Rust:
let mut table = Table::new();
table.add_column(Column::new(Cell::text("Surface")).width(WidthPolicy::Fixed(12)));
table.add_column(Column::new(Cell::text("Status")).width(WidthPolicy::Fixed(10)));

table.add_row(Row::new(vec![
  Cell::text("rust"),
  Cell::styled("OK", Style::Ok),
]));

let rendered = render(&table);
print!("{rendered}");
```

## CLI Usage (Deprecated)

> [!NOTE] The `fml table` CLI command is deprecated as of `v0.3.0` and will be
> removed in `v0.4.0`. Use the `fml::ui::table` library API directly instead.

```bash
# From a JSON string argument (deprecated):
fml table --json '{"columns": [...], "rows": [...]}'

# From stdin (deprecated):
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
}
```

`WidthPolicy` variants:

| Variant        | JSON form            | Meaning                                                                                                            |
| :------------- | :------------------- | :----------------------------------------------------------------------------------------------------------------- |
| Auto (default) | `"auto"`             | Size to content, subject to clamping                                                                               |
| Fixed          | `{"fixed": 14}`      | **At least** this wide: honored as a target, but widened past `fixed` rather than split a token in its own content |
| Min            | `{"min": 8}`         | Never narrower than this (nor than its own widest token)                                                           |
| Max            | `{"max": 40}`        | **Hard cap** — never wider than this, even if a token must be hard-split to fit                                    |
| Range          | `{"range": [8, 40]}` | Clamp to `[min, max]`; `max` is a hard cap (as `Max`)                                                              |
| Percent        | `{"pct": 25}`        | Percentage of total available table width; a hard cap (as `Max`)                                                   |

`Overflow` variants:

| Variant        | JSON form                         | Meaning                                                                                                                                                                                                                                                                                                                                                                                                          |
| :------------- | :-------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wrap (default) | `"wrap"`                          | Wrap onto additional lines, breaking only at spaces and after path separators (`/` `\`) and `,`/`;` — never mid-token where the column can hold the token. A token wider than the column is kept whole by widening the column under `Fixed`/`Min`/`Auto`; under a hard cap (`Max`/`Range`/`Pct`), or when even widening cannot keep the table within its width budget, the token is hard-split as a last resort. |
| Truncate       | `{"truncate": {"suffix": "..."}}` | Cut content and append `suffix` (default `"..."`)                                                                                                                                                                                                                                                                                                                                                                |
| Clip           | `"clip"`                          | Hard-cut with no suffix                                                                                                                                                                                                                                                                                                                                                                                          |

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
`fml doctor` builds a `Table` value in `src/commands/doctor/mod.rs` and renders
it through `fml::ui::table` exactly as `render_json` does from equivalent JSON.

## Framing (`fml`'s own output)

`fml doctor` and `fml fmt` / `fml lint` / `fml fix` (the `Runner`) never
hand-assemble their own header/rule lines. Each command builds one
`ui::table::Frame` (`src/ui/table/frame.rs`) from its primary rendered table and
renders every block — the table, follow-up notice sections, the diagnostics
block, the install summary — through `Frame::section`, which is the single
definition of the `header → rule → body → rule` shape. One `Frame` per command
means every rule that command prints is the same width: the table's content
width, capped at 80 columns (or a genuinely narrower terminal).
`Frame::wrap_body` wraps free-form notice/diagnostic prose to that same width on
the same token boundaries as the table renderer. Run-root paths in both table
cells and the diagnostics block are rendered relative via `ui::paths`
(`display_path` / `relativize_text`); absolute only when the path genuinely lies
outside the run root.
