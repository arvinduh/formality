# Architecture

A whole-repo module map: what each `src/` subdirectory is responsible for, and
where to go for the detail this document deliberately doesn't repeat. See
[docs/INDEX.md](INDEX.md) for the full doc set; this page only covers shape, not
per-feature behavior.

## Top-level crate (`src/lib.rs`, `src/main.rs`, `src/cli.rs`, `src/errors.rs`)

`src/main.rs` is the binary entry point and Process Host per rust-guide §3H. It
owns terminal color detection (`NO_COLOR`, `CLICOLOR_FORCE`), argument parsing
via `cli::Cli::parse_checked()`, top-level command dispatch to `fml::commands`,
background update notifier checks, and exit code mapping. `src/cli.rs` belongs
to the binary target (declared via `mod cli;` in `src/main.rs`), defining the
`clap`-based argument schema (`Cli`, `Commands`) and validation. The `fml`
library crate (`src/lib.rs`) has zero knowledge of `cli` and zero dependency on
`clap`, declaring the six library modules (`commands`, `config`, `engine`,
`errors`, `surfaces`, `ui`), and re-exporting `generate_schema` for external
integration tests (`tests/schema_drift.rs`). `src/errors.rs` is the crate-wide
error hierarchy — `FormalityError` and its per-subsystem inner types
(`GitError`, `SurfaceError`, the `IoError` struct, and `ConfigError`, which
`src/config` defines and this file re-exports) — with no `anyhow`/`thiserror`
dependency; see [style-guide.md](style-guide.md) §5 for the full convention.

## `src/config`

Everything about `formality.toml`/`.formality.toml`: parsing, cascade merging
across directory levels, path resolution, and the typed schema (`schema.rs`)
used both to validate config and to generate `schema/formality.schema.json` for
`fml schema`. `facets.rs` defines the canonical facet vocabulary (indentation,
line length, import sorting, ...) — see [facet-rosetta.md](facet-rosetta.md) for
what a facet is and why it exists. `lang_table.rs` is an X-macro table
generating the repetitive per-language options wiring shared by `LangConfig`,
`resolve_for_lang` and strict parsing, so adding a new typed per-language option
doesn't require hand-wiring it in each place. `options.rs` holds the
per-language strongly-typed formatting option structs (e.g. `RustOptions`);
`strict.rs` parses one config document, rejecting a key the typed structs do not
declare or a value of the wrong type with its key path and line; `resolve.rs`
implements the actual cascade-merge and path-resolution logic that turns raw
parsed TOML into a `ResolvedGlobalConfig`/`ResolvedLangConfig` a surface can act
on.

## `src/engine`

Execution, diffing, and version/update checking — the machinery that actually
runs formatters/linters across surfaces and reports results, as opposed to
`src/surfaces`, which defines _what_ each surface does. `engine/runner/mod.rs`
is `Runner::run`, the single dispatch point for every subcommand that acts
across surfaces (`fmt`, `lint`, `sync`, `fix`): it builds one `ExecutionContext`
per matched `LanguageSurface` and fans them out in parallel via
`rayon::par_iter`. See [style-guide.md](style-guide.md) §4 for the
`ExecutionContext` `Arc`-sharing rationale and the `Fix` three-stage dispatch
pattern (`lint(fix: true)` → `format()` → check-only re-lint of the surfaces
that still reported violations, so the reported status and exit code reflect the
post-format tree), and
[docs/adr/0001-arc-shared-execution-context.md](adr/0001-arc-shared-execution-context.md)
for the decision record. `engine/diff.rs` renders unified diffs for
`fmt --check`/`fml lint` output. `engine/version/` (`mod.rs`, `mstv.rs`)
resolves each surface's underlying tool version and enforces
minimum-supported-tool- version checks. `engine/update.rs` implements `fml`'s
own self-update check against GitHub Releases.

## `src/surfaces`

The `LanguageSurface` trait and the fleet of 12 per-language implementations
(`rust.rs`, `python.rs`, `cpp.rs`, `java.rs`, `go.rs`, `markdown.rs`, `yaml.rs`,
`json.rs`, `toml.rs`, `typst.rs`, `javascript.rs`, `kotlin.rs`), plus the shared
machinery they're all built on: `registry.rs` (the `SurfaceRegistry`,
canonical-name/alias lookup, and fleet-consistency tests), `glob.rs`
(candidate-file discovery and exclude-pattern matching), `native.rs` (the
`NativeConfig` trait for reading/writing a tool's own dotfile config), `sync.rs`
(`fml sync`'s generate-and-verify logic), `tooling.rs` (shared
subprocess/tool-invocation helpers), and `editorconfig.rs` (`.editorconfig`
generation, its own small domain sub-package per issue `#120 [pre-recreation]`).
See [language-surfaces.md](language-surfaces.md) for what each surface wraps and
[new-surface-guide.md](new-surface-guide.md) for how to add a 13th.

## `src/ui`

Terminal UI rendering, currently just semantic table formatting
(`ui/table/mod.rs`, `render.rs`): width policies, wrapping/truncation,
terminal-width clamping, and semantic color roles. This is the same machinery
both `fml::ui::table`'s public JSON-spec renderer (the `fml table` CLI command
that used to front it was removed in v0.3.0 (#255)) and `fml`'s own internal
output (`fml doctor`) render through — see [table-spec.md](table-spec.md) for
the JSON specification it consumes.

## `src/commands`

Mostly one file per CLI subcommand handler, dispatched from `run_command_inner`
in `src/lib.rs`: `fmt.rs`, `lint.rs`, `fix.rs` (the composite
lint-fix-then-format pipeline — see [style-guide.md](style-guide.md) §4's
`Runner` dispatch section), `sync.rs`, `init.rs`, `schema.rs` (`fml schema`,
JSON Schema generation), `lsp.rs` and `lsp_diagnostics.rs` (the `fml lsp`
Language Server — document formatting via `fml fmt` plus diagnostics publishing
via `fml lint`, with `lsp_diagnostics.rs` providing structured per-violation
diagnostics, `#159 [pre-recreation]`), and `doctor/` (a directory module —
`mod.rs`, `gitignore.rs`, `venv.rs` — implementing `fml doctor`'s
workspace/toolchain verification checks). Tool installation lives entirely in
`doctor/mod.rs` (`install_missing_tools_framed`), called only by
`fml doctor --install`. `fmt`/`lint`/`fix` instead call
`preflight_warn_stale_tools`, which only warns about stale tools, never
installs. `mod.rs` at the top of this directory also holds shared helpers used
by more than one command handler.

## Cross-cutting: process and release docs

Two things intentionally live outside `src/` and this map: the repo's process
facts (gate, CI checks, merge rules — see `AGENTS.md`) and the release procedure
(`v*` tags — see [release.md](release.md)). Neither is a code module, so neither
gets a paragraph here; both are linked from [docs/INDEX.md](INDEX.md).
