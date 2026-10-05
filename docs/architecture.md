# Architecture

A whole-repo module map: what each `src/` subdirectory is responsible for, and
where to go for the detail this document deliberately doesn't repeat. See
[docs/INDEX.md](INDEX.md) for the full doc set; this page only covers shape, not
per-feature behavior.

## Top-level crate (`src/lib.rs`, `src/main.rs`, `src/cli.rs`, `src/cli/*`, `src/errors.rs`)

`src/main.rs` is the binary entry point and minimal Process Host per rust-guide
§3H. It parses CLI arguments via `cli::Cli::parse_checked()`, delegates
execution to `cli::run()`, and maps `ExitStatus` to process exit codes.
`src/cli.rs` and the `src/cli/` directory define the `clap`-based argument
schema (`Cli`, `Commands`), validation, and per-subcommand modular adapters
(`doctor.rs`, `fix.rs`, `fmt.rs`, `init.rs`, `lint.rs`, `lsp.rs`, `schema.rs`,
`sync.rs`). Top-level dispatch, color overrides (`NO_COLOR`, `CLICOLOR_FORCE`),
config loading, and update checks are orchestrated in `cli::run()`. `src/lib.rs`
exports the core library modules (`cli`, `config`, `engine`, `errors`,
`surfaces`, `ui`). `src/errors.rs` is the crate-wide error hierarchy
(`FormalityError`, `ExitStatus`) powered by `thiserror` with per-subsystem leaf
errors (`config::Error`, `surfaces::Error`, `GitError`, `IoError`).

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

Execution pipelines, diffing, git path resolution, and version/update checking —
the core engine machinery that coordinates formatters/linters across surfaces
and reports results, as opposed to `src/surfaces`, which defines _what_ each
surface does.

- `engine/fmt.rs`, `engine/lint.rs`, `engine/fix.rs`: execution pipelines for
  formatting, linting, and autofixing.
- `engine/plan.rs`: plan dispatch, candidate path resolution, and reporting.
- `engine/git.rs`: git path resolution and staged/changed file inspection.
- `engine/sync.rs`, `engine/init.rs`, `engine/schema.rs`: config sync, project
  initialization, and schema generation pipelines.
- `engine/doctor/`: workspace and toolchain verification diagnostics and
  toolchain installations (`install_missing_tools_framed`).
- `engine/lsp.rs` and `engine/lsp_diagnostics.rs`: the in-process Language
  Server implementing document formatting and structured diagnostics.
- `engine/runner.rs`: `Runner::run`, executing passes in parallel across
  surfaces via `rayon::par_iter`. See [style-guide.md](style-guide.md) §4 for
  `ExecutionContext` `Arc`-sharing and the three-stage fix pipeline.
- `engine/diff.rs`: renders unified diffs for `fmt --check`/`fml lint` output.
- `engine/version/`: resolves tool versions and enforces minimum tool versions.
- `engine/update.rs`: implements self-update checks against GitHub Releases.

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

## `src/cli`

Modular adapters mapping CLI flags to engine pipelines:

- `cli.rs`: top-level argument parser (`Cli`, `Commands`), validation, and
  process execution coordinator (`cli::run()`).
- `cli/doctor.rs`, `cli/fix.rs`, `cli/fmt.rs`, `cli/init.rs`, `cli/lint.rs`,
  `cli/lsp.rs`, `cli/schema.rs`, `cli/sync.rs`: per-subcommand argument structs
  and execution adapters delegating to corresponding `engine` pipelines.

## Cross-cutting: process and release docs

Two things intentionally live outside `src/` and this map: the repo's process
facts (gate, CI checks, merge rules — see `AGENTS.md`) and the release procedure
(`v*` tags — see [release.md](release.md)). Neither is a code module, so neither
gets a paragraph here; both are linked from [docs/INDEX.md](INDEX.md).
