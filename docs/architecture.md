# Architecture

A whole-repo module map: what each `src/` subdirectory is responsible for, and
where to go for the detail this document deliberately doesn't repeat. See
[docs/INDEX.md](INDEX.md) for the full doc set; this page only covers shape, not
per-feature behavior.

## Top-level crate (`src/lib.rs`, `src/main.rs`, `src/cli.rs`, `src/errors.rs`)

`src/main.rs` is a thin binary entry point — it calls `fml::run()` and exits
with the returned code, nothing else. The real work lives in the `fml` library
crate (`src/lib.rs`), which declares the seven top-level modules (`cli`,
`commands`, `config`, `engine`, `errors`, `surfaces`, `ui`), owns the actual
subcommand dispatch (`run_command_inner` — the single `match args.command` that
routes every `Commands` variant to its handler, after loading and merging
config), and re-exports two crate-root items reached that way by this crate's
own integration tests (`SCHEMA_VERSION`, `generate_schema`) — see
[style-guide.md](style-guide.md) §1 for why those two survived the `#133`
alias-minimization sweep while the rest of the old `DEPRECATED / STALE ALIAS`
block did not: new code always uses the canonical structural path, never a
crate-root shortcut. `src/cli.rs` defines the `clap`-based argument parser only
(`Cli`, `Commands`, `MigrateCommands`) — it parses, it does not dispatch.
`src/errors.rs` is the crate-wide error hierarchy — `FormalityError` and its
per-subsystem inner enums (`ConfigError`, `GitError`, `ToolMissingError`,
`SurfaceError`, `IoError`) — with no `anyhow`/`thiserror` dependency; see
[style-guide.md](style-guide.md) §5 for the full convention.

## `src/config`

Everything about `formality.toml`/`.formality.toml`: parsing, cascade merging
across directory levels, path resolution, and the typed schema (`schema.rs`)
used both to validate config and to generate `schema/formality.schema.json` for
`fml schema`. `facets.rs` defines the canonical facet vocabulary (indentation,
line length, import sorting, ...) — see [facet-rosetta.md](facet-rosetta.md) for
what a facet is and why it exists. `lang_table.rs` is an X-macro table
generating the repetitive per-language options wiring shared by
`LangConfig`/`resolve_for_lang`/ `default_tools_for_lang`, so adding a new typed
per-language option doesn't require hand-wiring it in three places. `options.rs`
holds the per-language strongly-typed formatting option structs (e.g.
`RustfmtConfig`); `resolve.rs` implements the actual cascade-merge and
path-resolution logic that turns raw parsed TOML into a
`ResolvedGlobalConfig`/`ResolvedLangConfig` a surface can act on.

## `src/engine`

Execution, diffing, and version/update checking — the machinery that actually
runs formatters/linters across surfaces and reports results, as opposed to
`src/surfaces`, which defines _what_ each surface does. `engine/runner/mod.rs`
is `Runner::run`, the single dispatch point for every subcommand that acts
across surfaces (`fmt`, `lint`, `sync`, `fix`): it builds one `ExecutionContext`
per matched `LanguageSurface` and fans them out in parallel via
`rayon::par_iter`. See [style-guide.md](style-guide.md) §4 for the
`ExecutionContext` `Arc`-sharing rationale and the `Fix` two-stage dispatch
pattern, and
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
generation, its own small domain sub-package per issue `#120`). See
[language-surfaces.md](language-surfaces.md) for what each surface wraps and
[new-surface-guide.md](new-surface-guide.md) for how to add a 13th.

## `src/ui`

Terminal UI rendering, currently just semantic table formatting
(`ui/table/mod.rs`, `render.rs`): width policies, wrapping/truncation,
terminal-width clamping, and semantic color roles. This is the same machinery
both `fml table`'s public JSON-spec renderer and `fml`'s own internal output
(`fml doctor`, `fml list-surfaces`) render through — see
[table-spec.md](table-spec.md) for the JSON specification it consumes.

## `src/commands`

Mostly one file per CLI subcommand handler, dispatched from `run_command_inner`
in `src/lib.rs`: `fmt.rs`, `lint.rs`, `fix.rs` (the composite
lint-fix-then-format pipeline — see [style-guide.md](style-guide.md) §4's
`Runner` dispatch section), `sync.rs`, `init.rs`, `migrate.rs` (config
schema-reference migration), `schema.rs` (`fml schema`, JSON Schema generation),
`surfaces.rs` (`fml list-surfaces`), `table.rs` (`fml table`), `lsp.rs` and
`lsp_diagnostics.rs` (the `fml lsp` Language Server Protocol passthrough and its
structured per-violation diagnostics, `#159`), and `doctor/` (a directory module
— `mod.rs`, `gitignore.rs`, `venv.rs` — implementing `fml doctor`'s
workspace/toolchain verification checks). The exception to
one-file-per-subcommand is `fml install`, which has no file of its own: it
dispatches to `doctor::run_doctor(..., install: true)`, the same handler as
`fml doctor --install`, so tool installation lives in `doctor/mod.rs`
(`install_missing_tools`, `preflight_install` — the latter also called by
`fmt`/`lint`/`fix` for their `--install` flag). `mod.rs` at the top of this
directory also holds shared helpers used by more than one command handler (e.g.
the missing-tool warning printer).

## Cross-cutting: process and release docs

Two things intentionally live outside `src/` and this map: the multi-agent
orchestration process (worktrees, QA gate, dispatch order — see
`.agents/orchestrate.md`) and the release procedure (binary `v*` tags, schema
`s*` tags — see [release.md](release.md)). Neither is a code module, so neither
gets a paragraph here; both are linked from [docs/INDEX.md](INDEX.md).
