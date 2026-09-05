# Language Surface Guides

A **surface** in `fml` is a self-contained `LanguageSurface` implementation
(`src/surfaces/<lang>.rs`) that knows how to detect, format, lint, and sync
native tool config for one language. This document walks through all 12 surfaces
currently in the fleet: what tools each one wraps, what "Smart Format"
(format-before-lint mechanical fixes) it applies during `fml fmt`, which native
config file(s) `fml sync` manages, and what per-language `[lang.<name>]` options
are available beyond the shared [facet rosetta](facet-rosetta.md).

Every surface also supports the shared `[global]` keys where its facet support
allows (`indent_size`, `line_length`, `use_tabs`, `prose_wrap`) — this guide
only documents facets/options _specific_ to that surface. See
[Facet Rosetta](facet-rosetta.md) for the full support matrix and
`schema formality.schema.json` (`fml schema`) for the authoritative,
machine-generated shape.

---

## Rust

- **Format**: `cargo fmt` / `rustfmt`, with `reorder_imports = true` enabled so
  `fml fmt` sorts `use` statements as part of the normal formatting pass.
- **Lint**: `cargo clippy --all-targets -- -D warnings` (`--fix` variants add
  `--fix --allow-no-vcs --allow-dirty --allow-staged`).
- **Managed config**: `.rustfmt.toml`.
- **`[lang.rust]` options**: `edition` (e.g. `"2021"`, `"2024"`), `version`
  (rustfmt edition-adjacent version pin).
- **Facets**: `indent_tabs` fixed to spaces, `indent_width`/`line_length`/
  `import_sort`/`edition` configurable; `quote_style`, `trailing_comma`,
  `prose_wrap`, `standard` unsupported (no such concept in Rust/rustfmt).

## Python

- **Format**: `ruff check --select I --fix` (import sort pre-pass) →
  `ruff format` (the Smart Format pipeline described in the surface formatting
  matrix — imports are sorted _before_ the formatter runs so the formatted
  output never immediately fails an isort-style lint check).
- **Lint**: `ruff check`.
- **Managed config**: `ruff.toml`.
- **`[lang.python]` options**: `quote_style` (`"single"` / `"double"`),
  `target_version` (e.g. `"py311"`), `ignore_rules` (list of Ruff rule codes to
  ignore during linting, e.g. `["E501", "F401"]`).
- **Facets**: `indent_tabs`/`indent_width`/`line_length`/`quote_style`/
  `import_sort` configurable; `trailing_comma`, `prose_wrap`, `edition`,
  `standard` unsupported.

## C / C++

- **Format**: `clang-format`, with `SortIncludes: true` enabled so include
  ordering is normalized in the same pass as layout formatting.
- **Lint**: `clang-tidy`.
- **Managed config**: `.clang-format` (and `.clang-tidy`, generated with a broad
  default check set and `FormatStyle: none` so clang-tidy never fights
  clang-format over layout).
- **`[lang.cpp]` options**: `standard` (e.g. `"c++17"`, `"c11"`),
  `column_limit`, `based_on_style` (e.g. `"Google"`, `"LLVM"`),
  `pointer_alignment`, `break_before_braces`, `sort_includes` — these accept
  both snake_case and the native clang-format `PascalCase`/`kebab-case`
  spellings as aliases (e.g. `BasedOnStyle` / `based-on-style` /
  `based_on_style` all map to the same key).
- **Facets**: `indent_tabs`/`indent_width`/`line_length`/`import_sort`/
  `standard` configurable; `quote_style`, `trailing_comma`, `prose_wrap`,
  `edition` unsupported.

## Java

- **Format**: `google-java-format`, which organizes imports as part of the same
  `--replace` invocation — no separate import-sort pass needed.
- **Lint**: `checkstyle`.
- **Managed config**: `checkstyle.xml` (plus `.editorconfig` for the resolved
  indent width, since both must agree).
- **`[lang.java]` options**: `style` (`"google"` default, 2-space indent, or
  `"aosp"`, 4-space indent).
- **Facets**: `indent_tabs` fixed to spaces, `line_length` fixed to 100
  (google-java-format hardcodes this — there is no flag to change it),
  `indent_width` and `standard` are `Configurable` but resolve _through_ the
  `style` key rather than being set as an independent numeric value directly;
  `import_sort` configurable; `quote_style`, `trailing_comma`, `prose_wrap`,
  `edition` unsupported.
- **`supports_lint_fix`**: `false` — checkstyle is diagnostics-only and has no
  auto-fix mode, so `fml fix` only reformats Java files (via
  `google-java-format`) and reports checkstyle violations without attempting to
  fix them.

## Go

- **Format**: `goimports -w` (falls back to `gofmt -s -w` if `goimports` isn't
  installed) — grouping/sorting imports and simplifying code in the same pass.
- **Lint**: `golangci-lint run` (`./...` when unscoped, explicit files when the
  run is scoped to specific paths or a `--staged`/`--changed` filter).
- **Managed config**: `.golangci.yml`.
- **`[lang.go]` options**: `local_prefixes` (passed to `goimports -local` so
  first-party import groups are separated from third-party ones, e.g.
  `"example.com/myorg"`), `linters` (explicit golangci-lint linter list;
  defaults to golangci-lint's own default set when unset).
- **Facets**: `indent_tabs` **fixed** to `tab` — Go's tooling has no
  space-indentation mode, this is a non-negotiable language rule, not a
  per-project style choice; `indent_width`, `line_length`, `quote_style`,
  `trailing_comma`, `prose_wrap`, `edition`, `standard` all unsupported;
  `import_sort` configurable.

## Markdown

- **Format**: `markdownlint-cli2 --fix` (structural fixes: blank lines, table
  padding) → `prettier --write` (prose formatting), the Smart Format order that
  keeps `fml lint`'s markdownlint pass from immediately failing on cosmetic
  issues `fml fmt` could have fixed.
- **Lint**: `markdownlint-cli2`.
- **Managed config**: `.markdownlint.json`, `.prettierrc.json`.
- **`[lang.markdown]` options**: `prose_wrap` (`"always"` / `"never"` /
  `"preserve"`).
- **Facets**: `indent_tabs`/`indent_width`/`line_length`/`prose_wrap`
  configurable; `quote_style`, `trailing_comma`, `import_sort`, `edition`,
  `standard` unsupported.
- **`supports_lint_fix`**: `true`.

## YAML

- **Format**: `prettier`.
- **Lint**: `yamllint`.
- **Managed config**: `.prettierrc.json` (shared with JSON/Markdown) plus a
  generated `yamllint` config.
- **`[lang.yaml]` options**: `indent_sequence` (whether sequence items are
  indented under their parent key), `document_start` (require the `---` document
  marker), `truthy` (restrict truthy-value spellings, e.g. forbid bare
  `yes`/`no`).
- **Facets**: `indent_tabs` fixed to spaces, `indent_width`/`line_length`/
  `quote_style`/`prose_wrap` configurable; `trailing_comma`, `import_sort`,
  `edition`, `standard` unsupported.

## JSON

- **Format**: `prettier`.
- **Lint**: prettier itself acts as the check (`prettier --check`); no dedicated
  JSON linter is wired in.
- **Managed config**: `.prettierrc.json`.
- **`[lang.json]` options**: none currently (reserved for future knobs).
- **Facets**: `indent_tabs`/`indent_width` configurable; `quote_style` **fixed**
  to `double` and `trailing_comma` **fixed** to `none` — both are JSON-spec
  requirements, not style choices; `line_length`, `import_sort`, `prose_wrap`,
  `edition`, `standard` unsupported.

## TOML

- **Format**: `taplo fmt`, with key ordering/alignment normalization.
- **Lint**: `taplo lint`.
- **Managed config**: `taplo.toml`.
- **`[lang.toml]` options**: `align_entries` (whether to align entries across
  lines), `indent_entries` (whether to indent table entry keys), `indent_tables`
  (whether to indent table contents).
- **Facets**: `indent_tabs`/`indent_width`/`line_length` configurable;
  everything else unsupported (TOML has no imports, quote-style choice, or
  prose-wrap concept).

## Typst

- **Format**: `typstyle`.
- **Lint**: none dedicated — Typst diagnostics flow through the LSP (`tinymist`)
  rather than a standalone `fml lint` linter today.
- **Managed config**: none — `typstyle` is driven entirely by CLI flags (e.g.
  `--column`) rather than a persisted config file.
- **`[lang.typst]` options**: none currently (reserved for future knobs).
- **Facets**: `indent_tabs` fixed to spaces, `indent_width`/`line_length`
  configurable; everything else unsupported.

## JavaScript / TypeScript

- **Format**: `biome check --write --linter-enabled=false` — this is the Smart
  Format pass: it runs Biome's formatter _and_ `organizeImports` (governed by
  `biome.json`'s `organizeImports.enabled`) with the linter explicitly disabled,
  so `fml fmt` never applies lint fixes; those are reserved for `fml fix`.
  Covers `.js`, `.jsx`, `.ts`, `.tsx`, `.mjs`, `.cjs`, `.mts`, `.cts`.
- **Lint**: `biome lint` (or `biome check` when running the fix path).
- **Managed config**: `biome.json`.
- **`[lang.javascript]` options**: `quote_style`, `trailing_comma`, `semicolons`
  (`"always"` / `"as-needed"`), `organize_imports` (bool).
- **Facets**: `indent_tabs`/`indent_width`/`line_length`/`quote_style`/
  `trailing_comma`/`import_sort` all configurable; `prose_wrap`, `edition`,
  `standard` unsupported.
- **`supports_lint_fix`**: `true`.

## Kotlin

- **Format**: `ktlint -F` — a single Smart Format pass that fixes both style
  violations and import order together (ktlint doesn't separate formatting from
  import organization the way Biome/isort do).
- **Lint**: `ktlint` (without `-F`).
- **Managed config**: none dedicated — ktlint's official code style reads layout
  facets (`indent_size`, `max_line_length`, etc.) from `.editorconfig` rather
  than its own config file, so `fml sync` manages Kotlin's settings through the
  shared `.editorconfig` output instead of a Kotlin-specific file.
- **`[lang.kotlin]` options**: none currently — reserved for future knobs (e.g.
  `ktlint_code_style`).
- **Facets**: `indent_tabs` fixed to spaces, `quote_style` fixed to `double`
  (ktlint's standard ruleset enforces double-quoted strings);
  `indent_width`/`line_length`/`trailing_comma`/`import_sort` configurable;
  `prose_wrap`, `edition`, `standard` unsupported.
- **`supports_lint_fix`**: `true`.

---

## Detection

Each surface's `detect(&self, root: &Path) -> bool` decides whether it's
"active" for a workspace when `languages` isn't set explicitly in
`formality.toml` — generally: does at least one file with the surface's
`file_extensions()` exist under `root` (excluding common ignore directories).
`fml list-surfaces` shows detection status for every surface in the fleet,
active or not; `fml doctor --all` does the same for tool installation status.

## Adding a 13th surface

See [Adding a new language surface](new-surface-guide.md) for the full
implementation and self-registration walkthrough.
