# Style Guide

This is `fml`'s style guide: how code in this repository is written, organized,
and tested, beyond what any single PR review remembers from one session to the
next. It exists so a standard survives past the audit that found it — see
`.agents/orchestrate.md` §4 for the process obligation this document backs (a QA
reviewer who finds an uncovered violation promotes the rule here, or files a
follow-up to encode it, rather than fixing one PR and moving on).

## Base and scope

The base is the actual
[Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/) — what
`rustfmt` implements by default — plus the enabled `clippy` lints. This document
does not restate anything either of those already enforces; it only covers
`fml`-specific convention on top of them, and this codebase's own architecture.

## The three tiers

Rules here are sorted into three enforcement tiers, strongest first. A rule's
tier is a statement about _how_ it's enforced, not how important it is — tier 3
is the smallest tier by design, because anything that can be pushed up to tier 1
or 2 should be.

1. **Tool-native lint** — `rustfmt` defaults, or a `clippy` lint enabled in this
   crate. Authoritative: this document exists only for what these tools don't
   already cover, and never repeats their behavior. Checked in CI via
   `fml fmt --check` / `fml lint` (`cargo clippy --all-targets -- -D warnings`),
   which is `fml` dogfooding itself in `.github/workflows/ci.yml`.
2. **Repo-local test assertion** — a `#[test]` (in `--lib`, so it runs in the
   fast `Library Tests` PR check, not just the slower `main`-only CI) that walks
   the filesystem, the surface registry, or another in-crate side-table and
   fails if the rule is violated. The established pattern is
   `src/surfaces/registry.rs`'s fleet-consistency tests from `#113`
   (`test_all_fleet_surfaces_present`, alias/case-insensitive lookup) — reuse
   that mechanism for a new mechanically-checkable rule rather than inventing
   another one. `test_no_stray_test_files_outside_sanctioned_pattern` in
   `src/lib.rs` (added alongside this document) is the same pattern applied to
   the module/file hierarchy rule in §1 below.
3. **Documented, reviewer-checked** — prose, cited by section number in review.
   Smallest tier by design. **Every rule in this tier carries an explicit
   "promote to tier 2 if a mechanical check is found" note** — that note is not
   decoration, it's the instruction: if you're about to cite a tier-3 rule in
   review and realize it's actually checkable, write the `#[test]` instead of
   just citing the rule again.

---

## 1. Module/file hierarchy

**Rule (tier 2, enforced by
`test_no_stray_test_files_outside_sanctioned_pattern` in `src/lib.rs`):** test
modules live **inline**, in the file under test:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_something() { /* ... */ }
}
```

The one sanctioned exception is a **directory module** (`some/mod.rs`) whose
`mod.rs` is large enough that a sibling file keeps it readable — there, the
sibling is named exactly `tests.rs` and declared with `mod tests;`:

```text
src/engine/runner/
├── mod.rs      // `mod tests;` near the bottom
└── tests.rs    // `use super::*;`, then #[test] fns
```

No other `*_tests.rs` naming (`registry_tests.rs`, `mod_tests.rs`,
`facets_tests.rs`, etc.) is sanctioned, even though names like that appear in
this repo's own history. This was a deliberate, then-reversed convention: issue
`#82`'s
`refactor(surfaces): split src/surfaces/mod.rs into cohesive sub-modules` commit
introduced the sibling `<name>_tests.rs` files (`glob_tests.rs`, `mod_tests.rs`,
`registry_tests.rs`, `sync_tests.rs`, `tooling_tests.rs`;
`config/facets_tests.rs` predates it); issue `#120`'s
`refactor(structure): relocate editorconfig into domain sub-package` commit then
explicitly collapsed every one of them back inline, and a survey of the current
tree confirms which way the codebase actually settled — 25 files use inline
`mod tests { ... }` (all 12 language surfaces, `errors.rs`, `config/facets.rs`,
`config/schema.rs`, `engine/update.rs`, `commands/migrate.rs`,
`commands/lsp_diagnostics.rs`, …) against 5 that use the `mod.rs` + sibling
`tests.rs` split (`ui/table`, `config`, `engine/runner`, `engine/version`,
`commands/doctor`). Inline is the default; the sibling-file split is reserved
for directory modules specifically, not a free choice per file.

If you're adding a new language surface, see
[Adding a New Language Surface](new-surface-guide.md) — its test-coverage
checklist follows this same convention.

### Top-level layout

Per `AGENTS.md`:

- `src/config` — `formality.toml` parsing, resolution, schema.
- `src/engine` — execution, diffing, update checks.
- `src/surfaces` — one file per language surface (see
  [new-surface-guide.md](new-surface-guide.md) to add one).
- `src/ui` — table rendering.
- `src/commands` — CLI subcommand handlers.

`src/lib.rs` also carries a block of `DEPRECATED / STALE ALIAS` re-exports
(`pub use commands::doctor;` and similar) preserving pre-reorganization
`crate::foo::*` paths for external consumers and old integration tests. **Tier 3
(promote to tier 2 if a mechanical check is found):** new internal code always
spells out the canonical, structural path (e.g. `crate::ui::table`,
`crate::engine::version`) — never the shortened alias, even where it would
resolve to the same item. Only the alias list itself, and code that predates the
reorganization, uses the short form.

---

## 2. Naming conventions

Extracted from what all 12 language surfaces already do consistently — see
`src/surfaces/{rust,python,cpp,java,go,markdown,yaml,json,toml,typst,javascript,kotlin}.rs`.

- **Surface struct**: `<Lang>Surface`, a unit struct
  (`#[derive(Debug, Default, Clone, Copy)] pub struct RustSurface;` — `Copy`
  where the surface has no state, which is every current surface). One per file,
  and the file's `impl LanguageSurface for <Lang>Surface` and
  `impl DeclaresFacets for <Lang>Surface` both live in that same file, not split
  across others.
- **Native config struct**: `<Tool>Config` (e.g. `RustfmtConfig`), implementing
  `NativeConfig` with `const FILE_NAME: &'static str` set to the real
  dotfile/config name the tool reads (e.g. `.rustfmt.toml`). One native config
  struct per managed file, not one struct multiplexing several files.
- **Test functions**: `test_<behavior_under_test>` — `snake_case` starting with
  `test_`, describing the behavior, not the function under test alone (e.g.
  `test_get_surface_by_name_canonical_and_aliases`, not `test_get_surface`).
- **Registry/lookup functions**: free functions in `registry.rs`
  (`get_surface_by_name`, `resolve_canonical_name`, `detect_surfaces`,
  `detect_surfaces_smart`) rather than static methods on `SurfaceRegistry` when
  the operation doesn't need an existing registry instance
  (`SurfaceRegistry::default()` still supplies the actual fleet).
- **Predicate methods**: `is_*` returning `bool`, `#[must_use]`
  (`SurfaceResult::is_success`, `is_violation`, `is_error`;
  `ExitStatus::is_clean`, `is_violations`, `is_error`;
  `FacetSupport::is_configurable`, `is_fixed`, `is_unsupported`).
- **Tier 3 (promote to tier 2 if a mechanical check is found):** a pure getter
  or predicate (no I/O, no mutation) carries `#[must_use]`. This is common
  enough in the codebase to be a real convention, but no repo-local test
  currently walks the AST to check it — `clippy::must_use_candidate` is
  allow-by-default in this crate's lint set, so it isn't already tier 1 here.

---

## 3. Documentation requirements

Tier 1 already governs the bulk of this: `src/lib.rs` sets
`#![warn(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]`
crate-wide (landed via `#121`, this issue's blocker). That means:

- Every public item needs a `///` doc comment, or the build warns (`-D warnings`
  in CI makes that a hard failure).
- Every public function returning `Result` documents its error conditions (an
  `# Errors` section, or a doc sentence covering it for a short function).
- Every function that can panic documents when (`# Panics`), or doesn't panic.

On top of that tier-1 floor, this codebase's own convention (tier 3 — promote to
tier 2 if a mechanical check is found):

- Every `pub mod` declaration in `src/lib.rs` and `src/surfaces/mod.rs` etc.
  carries an outer `///` doc comment one line above the `mod` keyword describing
  what the module is for, even though `missing_docs` doesn't require this for
  module declarations specifically (see the `pub mod cli;` block at the top of
  `src/lib.rs`, and the per-surface `pub mod <lang>;` block in
  `src/surfaces/mod.rs`).
- An inline `#[cfg(test)] mod tests` block carries
  `#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]`
  directly under the `#[cfg(test)]` attribute, opting test code out of the
  crate-wide tier-1 doc lints above — test functions document themselves by name
  (§2). Every inline test module in the tree already does this; a new one that
  omits it will fail CI's `-D warnings` the moment it adds a `pub` item or a
  `Result`-returning helper.
- Every file with meaningful crate-level content (not just re-exports) opens
  with a `//!` module-level doc comment summarizing what lives in it (see the
  top of `src/surfaces/mod.rs`, `src/surfaces/registry.rs`).
- A non-obvious architectural choice gets a comment explaining _why_, not just
  _what_ — e.g. the `Arc`-sharing rationale on `ExecutionContext` (§4 below), or
  the `DEPRECATED / STALE ALIAS` markers in `src/lib.rs` explaining why each
  alias still exists. A comment restating what the next line of code already
  says is not this.

---

## 4. Architectural patterns

### `ExecutionContext` and `Arc`-sharing

`ExecutionContext` (`src/surfaces/mod.rs`) is built once per surface, per
invocation, and the `Runner` (`src/engine/runner/mod.rs`) dispatches all matched
surfaces in parallel via `rayon::par_iter`. Two of its fields —
`paths: Arc<Vec<PathBuf>>` and `global_config: Arc<ResolvedGlobalConfig>` — are
wrapped in `Arc` specifically because every surface in that parallel dispatch
sees the _same_ values: without the `Arc`, each of the (currently) 12 surfaces
would deep-clone the full candidate path list and global config on every
invocation, instead of a cheap refcount bump. `lang_config`, by contrast, is a
plain owned `ResolvedLangConfig` — it's genuinely per-surface
(`config.resolve_for_lang(surface.name())`), so there's nothing shared to `Arc`
there.

**Tier 3 (promote to tier 2 if a mechanical check is found):** a new field on
`ExecutionContext` (or a similarly fanned-out per-invocation struct) that holds
a value shared identically across every parallel surface invocation gets wrapped
in `Arc`, not cloned per-surface. A field that's already computed per-surface
(like `lang_config`) does not need this.

### `LanguageSurface` trait contract

`LanguageSurface: DeclaresFacets + Send + Sync` (`src/surfaces/mod.rs`) is the
core abstraction every surface implements. Required methods: `name`, `detect`,
`tool_info`, `format`, `lint`, `sync_config`, `clone_box`. `display_name`,
`aliases`, `file_extensions`, and `supports_lint_fix` all have default
implementations and are overridden only when a surface's behavior differs from
the default (e.g. `aliases()` returning `&["rs"]` for Rust). `clone_box` exists
solely to let `Box<dyn LanguageSurface>` implement `Clone`
(`impl Clone for Box<dyn LanguageSurface>` delegates to it) — every surface's
implementation is the same one-line `Box::new(self.clone())` pattern; don't
hand-write a different one per surface.

Every surface method that actually does work takes its inputs as arguments
(`format`/`lint`/`sync_config` take `&ExecutionContext`; `detect` takes `&Path`;
`tool_info` takes `&ResolvedLangConfig`) and never reaches into global state
(`std::env`, ambient config) directly — anything a surface needs comes through
those arguments, not from ambient lookup. This is what makes the
`rayon::par_iter` dispatch in `Runner::run` safe without additional
synchronization.

### `Runner` dispatch

`Runner::run` (`src/engine/runner/mod.rs`) is the single dispatch point for
every subcommand that acts across surfaces (`fmt`, `lint`, `sync`, `fix`) — it
takes the already-filtered `Vec<Box<dyn LanguageSurface>>`, builds one
`ExecutionContext` per surface, and fans out via `rayon::par_iter`. `Fix` is the
one multi-stage action: it runs `lint(fix: true)` across every surface first,
then `format(check: false)`, as two separate parallel stages rather than
interleaving lint-then-format per surface — so a fix pass is
lint-fix-everything, then format-everything, not format(surface A) before
lint(surface B) has even started. A new subcommand that needs to act across
surfaces goes through `Runner::run` with a new or existing `RunnerAction`
variant, rather than writing its own dispatch loop.

---

## 5. Error handling conventions

Landed via `#119` ("crate-wide error type hierarchy & standardized exit code /
diagnostic pipeline"), this issue's other blocker having already resolved by the
time this document was written. `src/errors.rs` is the single source of truth:

- No `anyhow`/`thiserror` — this crate hand-rolls its error hierarchy. Neither
  is a dependency (see `Cargo.toml`); don't add one for a new error site.
- `FormalityError` is the top-level enum, one variant per subsystem (`Config`,
  `Git`, `ToolMissing`, `Surface`, `Io`, plus `InvalidCli(String)` for cases
  with no dedicated subsystem type yet). Each subsystem variant wraps its own
  error enum (`ConfigError`, `GitError`, `ToolMissingError`, `SurfaceError`,
  `IoError`), which implements `fmt::Display` and `std::error::Error` directly —
  no derive macro, matching the no-`thiserror` rule above.
- A new fallible operation in an existing subsystem adds a variant to that
  subsystem's enum, not a new top-level `FormalityError` variant and not a bare
  `String`. `InvalidCli(String)` is the deliberate exception for CLI usage
  errors, not a precedent for other subsystems.
- `FormalityError::exit_status()` maps every variant to `ExitStatus::Error`
  (exit code 2) — that's the whole mapping today. If a future error case needs a
  different exit status (e.g. distinguishing a lint violation from an
  operational failure), that's a real design decision, not a mechanical change —
  raise it rather than special-casing `exit_status()` unilaterally.
- Rendering to the user goes through `render_diagnostic()` /
  `print_diagnostic()` (`[ERR]` red-bold prefix), not an ad hoc
  `eprintln!("Error: {e}")` at the call site.

**Tier 3 (promote to tier 2 if a mechanical check is found):** every
`FormalityError` variant's inner type implements `std::error::Error`, so
`?`-conversion via `From` stays ergonomic at call sites. No test currently walks
the trait impls to confirm this for a newly added variant.

---

## Amending this document

Per `.agents/orchestrate.md` §4: if a QA review finds a real violation this
document doesn't cover, that PR's sign-off isn't complete until the rule is
either promoted into this document (tier 1/2 if mechanically checkable, in the
same PR) or filed as a small, scoped follow-up issue to encode it. Don't leave a
newly-found standard undocumented, and don't turn the follow-up into another
open-ended sweep.
