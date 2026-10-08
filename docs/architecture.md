# Architecture

A whole-repo module map: what each `src/` subdirectory is responsible for, and
where to go for the detail this document deliberately doesn't repeat. See
[docs/INDEX.md](INDEX.md) for the full doc set; this page only covers shape, not
per-feature behavior.

## Library and binary

`fml` is two crates in one package. The library (`src/lib.rs`) exports four
modules: `config`, `engine` and `surfaces`. It returns data and has no knowledge
of `clap`, terminal rendering, or stdout. The binary (`src/main.rs`) declares
`mod cli;` itself and is the only place that prints.

```text
main.rs (mod cli;)  ──►  src/cli/  ──►  fml::{config, engine, surfaces}
```

`src/main.rs` is the process host per rust-guide §3H: it parses arguments,
installs the stderr logger (`-v`, or `RUST_LOG`), runs the command, and maps
`runner::ExitStatus` to the process exit code. Errors live in the module that
raises them (`config::Error`, `target::Error`, `surfaces::Error`); there is no
crate-wide error enum.

### Visibility

Each parent module decides what escapes. Items inside a module are plain `pub`
when another module uses them and private otherwise; restricted visibility
(`pub(crate)`) appears only on module declarations, at the gatekeeper, never on
items. The library exports only what the binary and the integration tests use.

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

Pure, in-memory engine primitives that coordinate execution across surfaces. The
engine returns results; it never renders or prints them. `src/surfaces` defines
_what_ each surface does, and `src/cli` turns engine results into output.

- `engine/lsp.rs` (+ `lsp/diagnostics.rs`): what the language server computes —
  running one pass over one file, formatting edits, and structured per-violation
  diagnostics parsed from each linter's machine-readable output.
- `engine/diff.rs`: renders unified diffs for `fmt --check` output.
- `engine/doctor.rs` (+ `doctor/gitignore.rs`, `doctor/venv.rs`): `check` (each
  tool's presence and MSTV/pin status) and `install` (one tool, with
  post-install verification), plus virtualenv detection and `.gitignore`
  hygiene.
- `engine/runner.rs`: `Runner::run` executes passes in parallel across surfaces
  via `rayon::par_iter` and returns `Vec<SurfaceResult>`; `compute_exit_status`
  folds those into an `ExitStatus`. See [style-guide.md](style-guide.md) §4 for
  `ExecutionContext` `Arc`-sharing and the three-stage fix pipeline.
- `engine/target.rs`: git path resolution, candidate path resolution, scope
  resolution, and surface filtering.
- `engine/update.rs` (+ `update/install.rs`): the background release check
  against GitHub Releases, and `fml update`'s pre-checks and its `axoupdater`
  run of the release installer into the binary's own directory.
- `engine/version/`: resolves tool versions and enforces minimum tool versions.

## `src/surfaces`

The `LanguageSurface` trait (`surfaces.rs`) and everything the surfaces share:

- `lang/`: the 12 per-language implementations (`cpp`, `go`, `java`,
  `javascript`, `json`, `kotlin`, `markdown`, `python`, `rust`, `toml`, `typst`,
  `yaml`).
- `registry.rs`: the `SurfaceRegistry`, canonical-name/alias lookup,
  auto-detection, and fleet-consistency tests.
- `tooling.rs`: the toolkit surfaces call to find and spawn their tools — binary
  lookup, install chains, exit classification, and the shared missing-tool
  results.
- `glob.rs`: candidate-file discovery and exclude-pattern matching.
- `sync.rs` + `sync/`: native config synchronization for `fml sync` —
  `native.rs` (`ToolConfig`, one tool's settings rendered either as its native
  file or as inline arguments), `prettier.rs` (the shared `.prettierrc.json`),
  and `editorconfig.rs` (`.editorconfig` generation, per issue
  `#120 [pre-recreation]`).

`tooling`, `glob` and `registry` live here rather than in `engine` because the
language implementations call them; `engine` depends on `surfaces`, never the
reverse. See [language-surfaces.md](language-surfaces.md) for what each surface
wraps and [new-surface-guide.md](new-surface-guide.md) for how to add a 13th.

## `src/cli`

The binary's thin orchestration layer: each command composes library calls and
prints their results.

- `cli.rs`: the parser (`Cli` with its global `--root`, `--config` and `-v`
  flags, and the `Command` enum), cargo-style help colors, config loading, and
  the update notice.
- `cli/pass.rs`: `fmt`, `lint` and `fix`, which share their file selection and
  pipeline and differ only in the `runner::Plan`.
  `cli/{sync,doctor,init, schema}.rs` hold one command each, as
  `Args::run(self, ctx)`.
- `cli/lsp.rs` (+ `lsp/server.rs`): the stdio JSON-RPC server over
  `engine::lsp`.
- `cli/log.rs`: the stderr logger. `cli/ui.rs`: plain-line output, one line per
  result.

## Tests

- Unit tests sit next to the code they test: inline `#[cfg(test)] mod tests`, or
  `foo/tests.rs` once a module's tests pass about 300 lines.
- `tests/api/`: the library's public API — registry, `.editorconfig` generation,
  and the `fmt`/`lint`/`fix`/`sync` passes over synthetic repositories.
- `tests/cli/`: only what the process alone shows — exit codes and the LSP stdio
  protocol.
- `tests/repo/`: repository hygiene — schema drift, release workflow edits,
  toolchain pins, version lockstep, and the source-tree rules in
  `source_rules.rs` that rustc and clippy cannot express.

## Cross-cutting: process and release docs

Two things intentionally live outside `src/` and this map: the repo's process
facts (gate, CI checks, merge rules — see `AGENTS.md`) and the release procedure
(`v*` tags — see [release.md](release.md)). Neither is a code module, so neither
gets a paragraph here; both are linked from [docs/INDEX.md](INDEX.md).
