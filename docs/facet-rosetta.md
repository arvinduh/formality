# Facet Rosetta

`fml` describes every language surface's formatting/linting behavior through a
small, canonical vocabulary of **facets** — concepts like indentation, line
length, or import sorting that recur across languages but are configured
differently (or not at all) by each surface's underlying tool. The facet rosetta
is how `fml` reasons about "does this surface support what the user just asked
for" instead of hardcoding nine-plus special cases throughout the codebase.

This document is the canonical reference for that vocabulary: what each facet
means, how each of the 12 language surfaces declares support for it, and how
`fml` validates a `formality.toml` against those declarations. The
implementation lives in `src/config/facets.rs` (the `Facet` enum and
`DeclaresFacets` trait) and `src/surfaces/<lang>.rs` (each surface's
`impl DeclaresFacets`).

## The canonical facets

| Facet          | Canonical name   | Meaning                                                            |
| :------------- | :--------------- | :----------------------------------------------------------------- |
| Indent style   | `indent_tabs`    | Indentation using tabs instead of spaces                           |
| Indent width   | `indent_width`   | Number of spaces per indentation level                             |
| Line length    | `line_length`    | Maximum line length / column limit before wrapping                 |
| Quote style    | `quote_style`    | Quotation mark style (single vs. double)                           |
| Trailing comma | `trailing_comma` | Trailing comma style in multiline structures                       |
| Import sorting | `import_sort`    | Sorting and organization of imports / includes                     |
| Prose wrap     | `prose_wrap`     | Prose wrapping behavior for text/Markdown                          |
| Edition        | `edition`        | Language edition / compiler epoch (e.g. Rust 2021, 2024)           |
| Standard       | `standard`       | Language standard version (e.g. C++17, c11) or formatter style set |

Facets are parsed leniently — `Facet::from_name` accepts common aliases
(`indent-size`, `tab_width`, `max_width`, `print_width`, `column_limit`,
`isort`, `std`, …) in addition to the canonical snake_case name, so
`formality.toml` and CLI flags don't force users to memorize the exact
identifier.

## Support levels

Every surface declares, for every facet, one of three support levels via
`FacetSupport`:

- **`Configurable`** — the user can freely set this facet in `formality.toml`
  (globally under `[global]` or per-language under `[lang.<name>]`) and the
  surface's formatter/linter will honor it.
- **`Fixed(value)`** — the underlying tool enforces a single, non-negotiable
  value (e.g. Go is always tab-indented; JSON is always double-quoted). Setting
  a _compatible_ value in config is accepted silently; setting an _incompatible_
  value produces a facet diagnostic (warning, or error under strict validation)
  rather than being silently ignored.
- **`Unsupported`** — the concept doesn't exist for this language/tool at all
  (e.g. Rust has no configurable quote style; TOML has no import-sorting
  concept). Attempting to configure it is diagnosed the same way as an
  incompatible `Fixed` value.

`is_value_compatible_with_fixed` (in `src/config/facets.rs`) is what decides
whether a configured value matches a `Fixed` facet's expected value — it accepts
common synonyms (`"spaces"` / `"false"` / `"off"` all match a `Fixed("spaces")`
indent-tabs facet, for example) so config authors aren't forced to spell things
exactly the way the internal constant does.

## The rosetta table

`Configurable` / `Fixed(value)` / `Unsupported`, one row per surface:

| Surface      | `indent_tabs` | `indent_width` | `line_length` | `quote_style` | `trailing_comma` | `import_sort` | `prose_wrap` | `edition`    | `standard`       |
| :----------- | :------------ | :------------- | :------------ | :------------ | :--------------- | :------------ | :----------- | :----------- | :--------------- |
| **Rust**     | Fixed(spaces) | Configurable   | Configurable  | Unsupported   | Unsupported      | Configurable  | Unsupported  | Configurable | Unsupported      |
| **Python**   | Configurable  | Configurable   | Configurable  | Configurable  | Unsupported      | Configurable  | Unsupported  | Unsupported  | Unsupported      |
| **C / C++**  | Configurable  | Configurable   | Configurable  | Unsupported   | Unsupported      | Configurable  | Unsupported  | Unsupported  | Configurable     |
| **Java**     | Fixed(spaces) | Configurable\* | Fixed(100)    | Unsupported   | Unsupported      | Configurable  | Unsupported  | Unsupported  | Configurable\*\* |
| **Go**       | Fixed(tab)    | Unsupported    | Unsupported   | Unsupported   | Unsupported      | Configurable  | Unsupported  | Unsupported  | Unsupported      |
| **Markdown** | Configurable  | Configurable   | Configurable  | Unsupported   | Unsupported      | Unsupported   | Configurable | Unsupported  | Unsupported      |
| **YAML**     | Fixed(spaces) | Configurable   | Configurable  | Configurable  | Unsupported      | Unsupported   | Configurable | Unsupported  | Unsupported      |
| **JSON**     | Configurable  | Configurable   | Unsupported   | Fixed(double) | Fixed(none)      | Unsupported   | Unsupported  | Unsupported  | Unsupported      |
| **TOML**     | Configurable  | Configurable   | Configurable  | Unsupported   | Unsupported      | Unsupported   | Unsupported  | Unsupported  | Unsupported      |
| **Typst**    | Fixed(spaces) | Configurable   | Configurable  | Unsupported   | Unsupported      | Unsupported   | Unsupported  | Unsupported  | Unsupported      |
| **JS / TS**  | Configurable  | Configurable   | Configurable  | Configurable  | Configurable     | Configurable  | Unsupported  | Unsupported  | Unsupported      |
| **Kotlin**   | Fixed(spaces) | Configurable   | Configurable  | Fixed(double) | Configurable     | Configurable  | Unsupported  | Unsupported  | Unsupported      |

\* Java's `indent_width` is _conditionally_ fixed: `google-java-format`'s
`--aosp` flag pins it to 4 spaces (vs. 2 for the default Google style), so `fml`
reports it as `Configurable` and resolves the effective width from the
configured `style` (`[lang.java] style = "google" | "aosp"`) rather than a
single constant — `.editorconfig` and `checkstyle.xml` both read that same
resolved value, so they can never disagree.

\*\* Java's `standard` facet doubles as the Google-vs-AOSP style selector — see
`[lang.java] style` below, not a language-spec version like C++'s `standard`.

### Reading the table

- A blank cell never appears — every surface declares a status for every facet,
  even if that status is `Unsupported`. This is deliberate: it means
  `fml doctor` / facet validation can always give a precise answer instead of
  silently ignoring an unrecognized combination.
- `Fixed` rows are not failures — they document a tool's non-negotiable behavior
  so `fml` can _warn_ instead of silently dropping a user's configured value
  (e.g. setting `use_tabs = true` under `[lang.go]` is a no-op that will be
  flagged, not a silent bug).
- Facets with **no cross-language equivalent** in this table (per-language tool
  knobs like C++'s `based_on_style`, Go's `local_prefixes`, or JavaScript's
  `semicolons`) are _not_ canonical facets — they're surface-specific typed
  options in `src/config/options.rs` (see
  [Language surface guides](language-surfaces.md) for the full list per surface)
  and are configured under `[lang.<name>]` directly, without going through facet
  validation.

## Where facets are used

- **`FormalityConfig` resolution** (`src/config/resolve.rs`) merges embedded
  defaults, user config, and project config layer by layer, then validates the
  resolved per-language settings against each surface's `declared_facets()`.
- **`fml sync`** uses facet support to decide what to emit into native tool
  configs (`.rustfmt.toml`, `ruff.toml`, `.clang-format`, `.editorconfig`,
  `checkstyle.xml`, `biome.json`, `.golangci.yml`, `.editorconfig`-driven ktlint
  settings, …) — a `Configurable` facet with a value set becomes an explicit key
  in the generated file; an `Unsupported` or non-matching `Fixed` facet produces
  a diagnostic instead.
- **`fml doctor`** and **`fml list-surfaces`** can surface facet support
  alongside tool detection so users can see at a glance what's tunable for each
  active surface.

## Extending the rosetta

Adding a new canonical facet means touching three things in lockstep: the
`Facet` enum variant + `name()`/`description()`/`from_name()` arms in
`src/config/facets.rs`, an `impl DeclaresFacets` arm in **every** surface file
under `src/surfaces/` (the trait has no default per-facet fallback, by design —
every surface must make an explicit decision), and this document's rosetta
table. See [Adding a new language surface](new-surface-guide.md) for the
parallel process of adding a whole new _surface_ rather than a new facet.
