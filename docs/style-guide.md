# Style Guide

The base standard is the global `rust-guide` skill. This document records only
where `fml` deliberately deviates from it and the rules specific to this
codebase. Anything `rust-guide` already states is not repeated here.

> The backticked `#N` citations throughout this document predate the 2026-08-26
> repo recreation and resolve to unrelated new issues — see
> [`docs/INDEX.md`](INDEX.md#note-on-pre-recreation-issuepr-numbers). Plain #N
> citations are current issue/PR numbers.

## Enforcement tiers

`rust-guide`'s tiers apply, with one repo-specific mechanism for tier 2: a
**repo-local test assertion**, a `#[test]` that walks the filesystem, the
surface registry, or another in-crate side-table and fails if the rule is
violated. The established patterns are `src/surfaces/registry.rs`'s
fleet-consistency tests (`#113 [pre-recreation]`) and the source scans in
`tests/repo/source_rules.rs`; reuse them for a new mechanically checkable rule.
Rules below name their enforcing test; the rest are tier 3, reviewer-checked.

---

## 1. Module/file hierarchy

**Rule (tier 2, enforced by `src_has_no_test_files` in
`tests/repo/source_rules.rs`):** unit tests live **inline** in the file under
test, whatever their size, and nothing named `tests.rs` or `*_tests.rs` lives in
`src/`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rejects_unknown_surface() { /* ... */ }
}
```

Integration tests live in three test crates under `tests/`, each a directory
with a `main.rs`: `tests/api/` (the library: registry, sync and the
`fmt`/`lint`/`fix` passes over synthetic repositories), `tests/cli/` (only what
the process alone shows: exit codes and the LSP stdio protocol), and
`tests/repo/` (repository hygiene and the source-tree rules). A new integration
test joins the module for its subject rather than adding a new top-level file.

### Canonical module paths

**Tier 2 (enforced by `internal_code_uses_canonical_module_paths` in
`tests/repo/source_rules.rs`):** internal code spells out the canonical,
structural path (e.g. `crate::surfaces::tooling`, `crate::engine::version`),
never a re-export shortcut. The crate has no `pub use` re-exports.

### Visibility

Each parent module is the gatekeeper for its children (`rust-guide` §3D). Items
are plain `pub` when another module uses them and private otherwise. Restricted
visibility (`pub(crate)`) appears only on a module declaration, never on an
item. The library exports only what the `fml` binary and the integration tests
use; `cli` is declared in `src/main.rs`, so the library cannot reach it.

### Module-only imports

**Tier 2 (enforced by `no_item_imports` in `tests/repo/source_rules.rs`):**
every `use` statement in `src/` and `tests/` must import a module, never an
item. The only permitted exceptions are named traits (imported when method
syntax requires them in scope), internal X-macros, and `use super::*;` inside
test modules.

---

## 2. Naming conventions

Extracted from what all 12 language surfaces do consistently — see
`src/surfaces/lang/{rust,python,cpp,java,go,markdown,yaml,json,toml,typst,javascript,kotlin}.rs`.

- **Surface struct**: `<Lang>Surface`, a unit struct
  (`#[derive(Debug, Default)] pub struct RustSurface;`). One per file, with its
  `impl LanguageSurface` and `impl DeclaresFacets` in that same file.
- **Native config**: `<tool>_config(ctx) -> native::ToolConfig` (e.g.
  `rustfmt_config`), one function per managed file, naming the real file the
  tool reads (e.g. `.rustfmt.toml`). `fml sync` and the inline arguments both
  render from it.
- **Test functions**: `<behavior_under_test>`, with no `test_` prefix (the
  `tests` module already says it), describing the behavior, not just the
  function (`get_surface_by_name_canonical_and_aliases`, not `get_surface`).
  Keep `test_` only where the bare name would shadow the item under test, which
  `use super::*` brings into scope.
- **Registry/lookup functions**: free functions in `registry.rs`
  (`get_surface_by_name`, `detect_surfaces_smart`) rather than static methods on
  `SurfaceRegistry` when no registry instance is needed.
- **Predicate methods (tier 2, enforced by `is_predicate_methods_carry_must_use`
  in `tests/repo/source_rules.rs`):** `is_*` returning `bool` carries
  `#[must_use]`. The scan normalizes visibility, `const`/`async`/`unsafe`
  modifiers and joins multi-line signatures; its first version matched only
  single-line `pub fn` signatures and stayed green with `#[must_use]` deleted
  from `ExitStatus::is_clean` (`#201 [pre-recreation]`).

---

## 3. Documentation requirements

The doc lints (`missing_docs`, `clippy::missing_errors_doc`,
`clippy::missing_panics_doc`) are enabled in `Cargo.toml`'s `[lints]` table,
which reaches every target (lib, bin, and each `tests/*.rs` crate). On top of
`rust-guide`:

- Every `pub mod` declaration carries an outer `///` doc comment above the `mod`
  keyword, though `missing_docs` does not require it. **Tier 2 (enforced by
  `pub_mod_declarations_carry_doc_comments` in `tests/repo/source_rules.rs`).**
- The `//!` header is enforced by `files_carry_module_doc_comment` in
  `tests/repo/source_rules.rs`. A §1 `tests.rs` file is exempt.
- **Doc comments on `JsonSchema`-derived types are published output.**
  `schemars` lifts a doc comment on a type or field deriving `JsonSchema` (such
  as `LangConfig` in `src/config.rs`) verbatim into
  `schema/formality.schema.json`, where users and IDE tooltips read it. Editing
  one changes the published schema and fails `tests/repo/schema_drift.rs` until
  the schema is regenerated. Never edit one incidentally; batch prose fixes onto
  a schema change already happening for a functional reason. Internal rationale
  and tracker syntax (`(Fixes #N)`, `TODO`) go in `//` comments, never in `///`
  on schema types. **Motivating case:** `#150` / PR `#194` put `(Fixes #150)`
  into `extra_args` schema tooltips and failed `schema_drift.rs`; the rationale
  moved to call-site comments.
- **Claims about external tool behavior cite a reproduction.** A doc comment,
  ADR, code rationale, or diagnostic asserting how an external tool behaves
  (exit codes, flag syntax, duplicate-flag handling, error formatting) cites a
  command actually run against the version this repo pins (`Cargo.toml` or
  `docs/language-surfaces.md`): command line, tool version, output. This applies
  to premises inherited from issue text too. Record it in the PR body, the
  relevant ADR, or a test or code comment, by scope. **Motivating case:** `#173`
  / PR `#197` adopted an issue's claim that a duplicate `--linter-enabled`
  re-enabled Biome's linter; Biome 2.5.10 (pinned) rejects duplicate flags
  outright. The same review found markdownlint-cli2 v0.23.2 consumes unknown
  flags as globs rather than failing.

---

## 4. Architectural patterns

### `ExecutionContext` and `Arc`-sharing

`ExecutionContext` (`src/surfaces.rs`) is built once per surface, per
invocation, and the `Runner` (`src/engine/runner.rs`) dispatches all matched
surfaces in parallel via `rayon::par_iter`. `paths: Arc<Vec<PathBuf>>` and
`global_config: Arc<ResolvedGlobalConfig>` are `Arc`-wrapped because every
surface sees the same values; without it each of the 12 surfaces would
deep-clone them per invocation. `root: Arc<PathBuf>` is wrapped for consistency,
not for a comparable saving, so do not cite it as precedent for wrapping the
next small field. `Arc<PathBuf>` rather than `Arc<Path>` is deliberate: each
field wraps the type's natural owned form. `lang_config` is a plain owned
`ResolvedLangConfig` because it is genuinely per-surface. See
[ADR 0001](adr/0001-arc-shared-execution-context.md).

**Tier 3:** a new field on `ExecutionContext` (or a similarly fanned-out
per-invocation struct) holding a value shared identically across every parallel
surface invocation is wrapped in `Arc`, not cloned per surface.

### `LanguageSurface` trait contract

`LanguageSurface: DeclaresFacets + Send + Sync` (`src/surfaces.rs`) is the core
abstraction every surface implements. Required methods: `name`, `detect`,
`tool_info`, `format`, `lint`, `sync_config`, `clone_box`. `aliases`,
`file_extensions`, and `supports_lint_fix` have defaults, overridden only when a
surface differs (e.g. `aliases()` returning `&["rs"]` for Rust). `clone_box`
exists solely so `Box<dyn LanguageSurface>` implements `Clone`; every surface
implements it as `Box::new(self.clone())`.

Surface methods take everything they need as arguments
(`format`/`lint`/`sync_config` take `&ExecutionContext`; `detect` takes `&Path`;
`tool_info` takes `&ResolvedLangConfig`) and never read ambient state
(`std::env`, global config). That is what makes the `rayon::par_iter` dispatch
in `Runner::run` safe without further synchronization.

### Manifest probes vs. surface detection

**Rule (tier 3):** probing for a build manifest (`Cargo.toml`, `go.mod`,
`package.json`, ...) uses `.is_file()`, never `.exists()`; a directory can share
the manifest's name.

The distinction between tool preflights and surface detection is load-bearing:

1. **Tool execution preflights** (e.g. `RustSurface::lint` checking for
   `Cargo.toml`, `GoSurface::lint` for `go.mod`) walk ancestors with
   `crate::surfaces::glob::find_manifest_upwards`. In a Cargo workspace member
   or Go submodule the manifest lives above `ctx.root`, and the tool itself
   walks up to find it.
2. **Surface detection (`LanguageSurface::detect`)** stays **root-only**
   (`root.join(...)`). If `detect` walked parents, running `fml` in any nested
   directory would activate surfaces for every ancestor project type.

**Motivating case:** `#185` / PR `#193` — `RustSurface::lint` checked only
`ctx.root`, so `fml lint` from a workspace subdirectory failed with an
`ExecutionError` although `cargo clippy` would have succeeded.

### Production users for new API surface

`rust-guide`'s YAGNI rule applies with one sharpening: **unit tests are not a
production user.** A new variant, field, or parameter needs a non-test call site
in the same PR. `dead_code` is blind to items used only inside `#[cfg(test)]`,
so reviewers check this by hand. A PR justifying new surface by a downstream
issue's needs verifies that claim against the downstream issue's acceptance
criteria. **Motivating case:** `#177` / PR `#195` added `ProbeArg::ToolPath` for
`#178`, which turned out to need custom line extraction regardless; the variant
was reverted.

The same holds for prose: doc comments, `--help` text, and `docs/` describe
behavior that exists today. A planned capability belongs in an issue, and no CI
check (drift test, generated table) may exist only to keep speculative prose in
sync. **Motivating case:** `#123` — `fml lsp`'s docs described a child-LSP
router that was never built.

### `Runner` dispatch

`Runner::run` (`src/engine/runner.rs`) is the single dispatch point for every
subcommand that acts across surfaces (`fmt`, `lint`, `sync`, `fix`): it takes
the filtered `Vec<Box<dyn LanguageSurface>>`, builds one `ExecutionContext` per
surface, and fans out via `rayon::par_iter`. `fix` runs three parallel stages:
`lint(fix: true)` on every surface, then `format(check: false)`, then a
check-only re-lint of the surfaces that still reported violations, so status and
exit code reflect the tree after formatting (a violation the formatter resolved
must not report `[FAIL]`). A new cross-surface subcommand goes through
`Runner::run` with a `RunnerAction` variant, not its own dispatch loop.

---

## 5. Error handling conventions

This crate follows `rust-guide` §3C: a module fails with the narrowest type that
says what went wrong, and defines its own `Error` only when that adds
information.

- `config::Error`, `target::Error` and `surfaces::Error` are the custom errors,
  each defined in the module that raises it. There is no crate-wide error enum:
  no caller needs to match across subsystems.
- A missing tool is not an error: it is the run status
  `SurfaceStatus::ToolMissing`.
- How a run ends is `runner::ExitStatus` (`Clean`, `Violations`, `Error`,
  ordered best to worst); `compute_exit_status` folds results with `max`.
- The library never prints an error. The CLI renders one through `cli::ui`'s
  `[ERR]` line, not an ad hoc `eprintln!`.

---

## 6. Testing conventions

Every rule here is verified the same way: reintroduce the defect and watch the
test fail.

### Test specificity: assert what only the tested path produces

**Rule (tier 3):** a test asserting that a specific code path ran asserts on a
message, attribute, or artifact that only that path produces. Matching a generic
variant (`matches!(status, SurfaceStatus::ExecutionError { .. })`) is vacuous
when the surface can reach that variant another way, which multi-tool surfaces
(Markdown: `markdownlint` + `prettier`; Python: `ruff` + `isort`) routinely can.
Assert on distinct message text, inspect the assembled argv, or unit-test
argument builders in isolation.

**Motivating case:** `#150` / PR `#194` tested `extra_args` forwarding to
`markdownlint-cli2` by matching `ExecutionError`; `prettier` failed on the same
flag, so both tests stayed green with the forwarding deleted.

### Exit status: assert only what the test controls

**Rule (tier 2 in `src/`, tier 3 in `tests/`):** a test asserts on the narrowest
function that makes the decision under test. It does not assert the exit status
of a full command run (or test `run_cli` runner) whose outcome also depends on
tools or environment the test is not about. If the decision is buried in a
command handler, extract it as a private function that production calls (§4) and
test that. Lifecycle tests whose subject is the tool (`fmt` then `fmt --check`
with rustfmt or ruff) are exempt, but skip when that one tool is missing. Unit
tests in `src/` reach the private function directly, so
`unit_tests_do_not_dispatch_full_commands` (`tests/repo/source_rules.rs`)
forbids them from calling full command runners at all; `tests/` sees only the
public API, so reviewers check it there.

**Motivating case:** #291 / PR #400 — `relative_root_resolves_to_absolute`
asserted `ExitStatus::Clean` from a real `fml doctor` run on the checkout. It
failed 6 of 20 runs while another process relinked `taplo`, and still passed
with `std::path::absolute` removed.

### Source-scan tests: assert absence, bound the window, prefer runtime assertions

**Rule (tier 3):** a source-scan test (`include_str!` over Rust source to assert
a structural property the type system cannot express):

1. **asserts the absence** of the defect, not only the presence of the fix;
2. **bounds its scan window** to the function or block under test, never to end
   of file.

Prefer a runtime assertion (execution report, return value, side effect)
wherever one can express the property; reach for a source scan only for strictly
structural invariants, and document why in the test's doc comment.

**Motivating case:** `#106` / PR `#196` —
`run_doctor_folds_install_into_tally_before_rendering_footer` scanned to EOF for
the substring `scan.`, and stayed green when the bug was reintroduced as
`ToolTally::from_scan(&scan)`.

### File-matching invariant tests: strip comments before matching

**Rule (tier 3):** a test asserting on a file's contents (workflow YAML, config,
source) strips comments before matching whenever that file's comments quote the
guarded text. Applies to workflow guards (`tests/repo/release_workflow.rs`),
source scans (`src/cli/doctor/tests.rs`), and any self-documenting file.

**Motivating case:** `#164` / PR `#199` — deleting the live `fetch-depth: 0`
line from `release.yml` failed no test, because a `# LOCAL EDIT` comment quoted
it verbatim.
