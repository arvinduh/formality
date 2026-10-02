# Language Surface Guides

A **surface** in `fml` is a self-contained `LanguageSurface` implementation
(`src/surfaces/<lang>.rs`) that knows how to detect, format, lint, and sync
native tool config for one language. This document walks through all 12 surfaces
currently in the fleet: what tools each one wraps, what "Smart Format"
(format-before-lint mechanical fixes) it applies during `fml fmt`, which native
config file(s) `fml sync` manages, and what per-language `[lang.<name>]` options
are available beyond the shared [facet rosetta](facet-rosetta.md).

`<name>` is the surface's canonical name, as in each `[lang.<name>] options`
entry below. An alias (`[lang.py]`) or another casing (`[lang.RUST]`) fails
config loading with the section to rename it to; a name matching no surface
loads with an `Unrecognized language section` warning.

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
- **`extra_args` handling**: `--extend-select` on the import pass is
  discriminated so exit 1 with findings reports as a `[FAIL]` violation row
  rather than `[ERR] Execution error` (Fixes
  [#208](https://github.com/arvinduh/formality/issues/208)); see
  [`extra_args` and exit-code contracts](#extra_args-and-exit-code-contracts).

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
- **`extra_args` caveat**: `--set-exit-if-changed` makes a successful reformat
  report as `[ERR] Execution error` — a known limitation, not guarded; see
  [`extra_args` and exit-code contracts](#extra_args-and-exit-code-contracts).

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
  issues `fml fmt` could have fixed. A step before the fixer and another after
  prettier escape a `#` that starts a paragraph continuation line, whether the
  author or prettier's prose wrap put it there (`#299)` becomes `\#299)`, which
  renders the same). Otherwise MD018/MD020 report it, and `markdownlint --fix`
  turns the prose into a heading (#314, #413). An unspaced `#Title` that opens
  its own block is left to the fixer, which makes it `# Title`. `fml fix`'s lint
  pass escapes before its own `--fix` too. markdownlint picks the lines from the
  content on stdin, so it never writes the file.
- **Lint**: `markdownlint-cli2`.
- **Managed config**: `.markdownlint.json`, plus the shared `.prettierrc.json`
  (see below). `MD010`/`no-hard-tabs` and `MD029`/`ol-prefix` are off in the
  config fml generates (`fml fmt`/`fix`/`lint`/`lsp`, and the
  `.markdownlint.json` that `fml sync` writes) (#479): their fixers change what
  a list renders as. MD010 swaps a tab for one space, which moves a tab-indented
  paragraph out of its list item, and MD029 renumbers `10.` to `1.`, dropping
  the list's start number. prettier already turns those tabs into spaces and
  renumbers the items after the first, both without changing the rendering. A
  project's own `.markdownlint.*` replaces that config entirely, so it brings
  both rules and their fixers back unless it also sets them to `false`.
- **`[lang.markdown]` options**: `prose_wrap` (`"always"` / `"never"` /
  `"preserve"`); `no_inline_html` (bool, default `false`) — controls
  markdownlint's `MD033`/`no-inline-html` rule. Ships **disabled** by default:
  it is a house-style rule, not a correctness one, and there is no markdown
  equivalent for centered badge blocks (`<p align="center">` + `<img>`) or
  `<details>`/`<summary>` disclosure widgets. Set `no_inline_html = true` to opt
  back in.
- **Facets**: `indent_tabs`/`indent_width`/`line_length`/`prose_wrap`
  configurable; `quote_style`, `trailing_comma`, `import_sort`, `edition`,
  `standard` unsupported.
- **`supports_lint_fix`**: `true`.
- **`extra_args` keys**: `markdownlint-cli2` and `prettier`, each reaching only
  its own tool; see
  [`extra_args`: one list per tool](#extra_args-one-list-per-tool).

## YAML

- **Format**: `prettier`.
- **Lint**: `yamllint`.
- **Managed config**: a generated `yamllint` config, plus the shared
  `.prettierrc.json` (see below).
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
- **Managed config**: the shared `.prettierrc.json` only (see below); JSON has
  no native config of its own.
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
- **Lint**: none dedicated — Typst diagnostics flow through `fml lsp`, which
  runs `typst compile`, rather than a standalone `fml lint` linter today.
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
- **`extra_args` caveat**: `--linter-enabled` is refused — `fml fmt` passes it
  itself, see
  [`extra_args` and exit-code contracts](#extra_args-and-exit-code-contracts).

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

## Shared config files

Two managed files are not owned by any one surface, because more than one
surface needs them:

- **`.editorconfig`** — aggregates every active surface's layout facets.
- **`.prettierrc.json`** — used by the Markdown, YAML and JSON surfaces, which
  all format via `prettier`.

`fml sync` writes each of them **once**, in a pass that runs after the
per-surface fan-out and reports itself under its own name (`editorconfig`,
`prettier`). A surface declares that it consumes the prettier config via
`LanguageSurface::uses_prettier`; it must never sync that file from its own
`sync_config`. Three surfaces doing exactly that raced on one path under
`surfaces.par_iter()`, which made the report nondeterministic and risked a
sharing violation on Windows (#130).

Because there is one file, all of its surfaces must resolve it the same way.
Conflicting `[lang.<name>]` overrides — say `[lang.markdown] line_length = 100`
or `[lang.markdown] prose_wrap = "preserve"` against the global default — are
reported as an explicit error naming the surfaces and the settings they disagree
on.

**What `fml sync` does on such a conflict:** it still writes every file it can
resolve unambiguously — `.editorconfig`, `.markdownlint.json`, the `yamllint`
config, `taplo.toml` and so on — and withholds only `.prettierrc.json`, the one
file the overrides genuinely disagree about. The run then exits `2`, so
`fml sync --check` used as a pre-commit hook fails until the overrides are
aligned. Unrelated surfaces are never blocked by the conflict, but the run as a
whole is not reported as clean, because one requested file was not materialized.

`fml fmt` is unaffected either way: it passes each surface's own settings to
`prettier` inline and never reads the file, so a `prose_wrap` override that
`fml sync` declines to write is still applied by `fml fmt`. Aligning the
overrides (or moving the setting to the global table) is what reconciles the
two.

---

## `extra_args` and exit-code contracts

Each `[lang.<name>.extra_args]` list is appended **after** `fml`'s own flags, so
a user-supplied value wins. For nearly every flag that is the intent. A few
flags are different: they change what a non-zero exit code _means_.

Each surface decides whether a non-zero exit is "ran, found violations"
(`[FAIL]`) or "could not run" (`[ERR]`, process exit 2) from the tool's
exit-code contract _as `fml` invokes it_. An `extra_args` entry that
reintroduces a "ran fine, and found/changed something" exit code makes that
decision wrong, and a lint finding gets reported as an execution error.

The flags known to do this, each reproduced against the version
`fml doctor --install` pins:

| Surface        | Flag                     | Status                           | What you'll see                                                                                                                                                                                                                          |
| -------------- | ------------------------ | -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **python**     | `--extend-select <rule>` | **Handled (Fixes #208)**         | On the `ruff check --select I --fix` import pass, exit 1 when selection is widened or findings are present is classified as a `[FAIL]` violation row with process exit 1, not `[ERR] Execution error`. Verified on `ruff 0.16.4`.        |
| **java**       | `--set-exit-if-changed`  | **Unguarded — known limitation** | `google-java-format --replace` still rewrites the file, then exits 1 because it changed something; reported as `[ERR] Execution error` rather than a successful format. Verified on `google-java-format@2.3.0` (upstream 1.35.0).        |
| **javascript** | `--linter-enabled`       | **Refused** with an explanation  | Not actually an instance of the above: `fml fmt` passes this flag itself and biome rejects it given twice, so the format pass fails either way. `fml` now says so instead of surfacing biome's opaque error. Verified on `biome@2.5.10`. |

**The java row remains a live bug and known limitation.** If you set
`--set-exit-if-changed`, `fml` will report a real result as an execution failure
and exit 2. It is not guarded, because the flag does not contradict anything
`fml` passes — detecting it would mean maintaining an enumeration of each tool's
flag vocabulary, which goes stale every time a tool adds one. The python case
was resolved in #208 by discriminating exit 1 when `extra_args` widens selection
or the output contains lint findings.

The javascript row is guarded only because `fml fmt` runs
`biome check --write --linter-enabled=false` and biome rejects a duplicated
flag: **no** value in `extra_args` ever worked there, `=true` and `=false`
alike. The refusal replaces an error that explained nothing with one naming the
flag, `extra_args`, and the way out — configure biome's linter under `fml lint`,
where it belongs. It does not fix a misclassified exit code; there was never a
lint finding on that path to misclassify.

See [ADR 0005](adr/0005-extra-args-exit-code-contracts.md) for the full
reasoning, including why re-deriving each classifier from the final argv (which
_would_ have covered the python and java cases) was rejected.

---

## `extra_args`: one list per tool

`[lang.<name>.extra_args]` is a table. Each key names one tool the surface runs,
and its list is appended after `fml`'s own flags on that tool's invocations
only. A tool without a key gets no extra arguments.

```toml
[lang.markdown.extra_args]
prettier = ["--prose-wrap", "always"]
markdownlint-cli2 = ["--no-globs"]

[lang.python.extra_args]
ruff-check = ["--extend-select", "F"]
ruff-format = ["--preview"]
```

Keys are the binary names `fml doctor` reports and the install chains use. The
one exception is python, whose single `ruff` binary runs two passes, so its keys
name the subcommand too.

| Surface        | Keys                                                                                             |
| -------------- | ------------------------------------------------------------------------------------------------ |
| **rust**       | `rustfmt` (`fml fmt`, via `cargo fmt` or bare), `clippy-driver` (`fml lint`, via `cargo clippy`) |
| **python**     | `ruff-check` (the `--select I --fix` import pass of `fml fmt`, and `fml lint`), `ruff-format`    |
| **cpp**        | `clang-format`, `clang-tidy`                                                                     |
| **java**       | `google-java-format`, `checkstyle`                                                               |
| **go**         | `gofmt`, `goimports`, `golangci-lint`                                                            |
| **markdown**   | `markdownlint-cli2` (also when the older `markdownlint` binary stands in), `prettier`            |
| **yaml**       | `prettier`, `yamllint`                                                                           |
| **json**       | `prettier`                                                                                       |
| **toml**       | `taplo`                                                                                          |
| **typst**      | `typstyle`                                                                                       |
| **javascript** | `biome`                                                                                          |
| **kotlin**     | `ktlint`                                                                                         |

Any other key fails config loading, naming the keys the surface accepts:

```text
unknown key `lang.python.extra_args.ruff` in formality.toml:3: `python` runs no tool named `ruff`; its extra_args keys are `ruff-check`, `ruff-format`.
```

The flat list (`extra_args = [...]` directly under `[lang.<name>]`), which
reached every tool a pass ran, is removed; it fails config loading, naming the
table form:

```text
`lang.markdown.extra_args` in formality.toml:3 is a list, but extra_args takes one list per tool: write it as a table, e.g. `[lang.markdown.extra_args]` then `markdownlint-cli2 = ["--flag"]`. Tools for `markdown`: `markdownlint-cli2`, `prettier`.
```

One markdown hazard remains, now confined to its own key: a `--config` under
`markdownlint-cli2` lands after the temp config `fml` injects, and
markdownlint-cli2 honours the **last** `--config` it sees, so it replaces the
markdownlint settings your `formality.toml` resolved. Reproduced against
`markdownlint-cli2 v0.23.2 (markdownlint v0.41.1)`.

---

## Detection

Each surface's `detect(&self, root: &Path, present: &PresentExtensions) -> bool`
decides whether it's "active" for a workspace when `languages` isn't set
explicitly in `formality.toml`: is one of its `marker_files()` a regular file at
`root`, or does at least one candidate file with one of its `file_extensions()`
exist under `root` (excluding common ignore directories). `present` is the set
of extensions from one walk of `root`, shared by every surface, so
auto-detection walks the tree once. `fml doctor` shows detection and tool status
for active surfaces, and `fml doctor --all` shows both for every surface in the
fleet.

## Adding a 13th surface

See [Adding a new language surface](new-surface-guide.md) for the full
implementation and self-registration walkthrough.
