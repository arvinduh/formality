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
`src/lib.rs`'s test module; reuse them for a new mechanically checkable rule.
Rules below name their enforcing test; the rest are tier 3, reviewer-checked.

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

The one sanctioned exception is a **directory module** (`some.rs` + `some/`)
whose root is large enough that a submodule file keeps it readable — there, the
submodule is named exactly `tests.rs` and declared with
`#[cfg(test)] mod tests;`:

```text
src/engine/
├── runner.rs
└── runner/
    ├── violations.rs
    └── tests.rs
```

No other `*_tests.rs` naming (`registry_tests.rs`, `mod_tests.rs`, ...) is
sanctioned. `#82 [pre-recreation]` introduced sibling `<name>_tests.rs` files
and `#120 [pre-recreation]` collapsed every one of them back inline; the tree
settled on inline as the default and the `tests.rs` split only for directory
modules (`ui/table`, `config`, `engine/runner`, `engine/version`,
`engine/doctor`).

### Canonical module paths

**Tier 2 (enforced by `test_internal_code_uses_canonical_module_paths` in
`src/lib.rs`):** internal code spells out the canonical, structural path (e.g.
`crate::ui::table`, `crate::engine::version`), never a crate-root shortcut.

One crate-root re-export remains, load-bearing:
`pub use config::schema::generate_schema;` is reached as `fml::generate_schema`
by `tests/schema_drift.rs`. A re-export earns a place at the crate root only by
being reached that way by real code.

### Module-only imports

**Tier 2 (enforced by `test_no_item_imports` in `src/lib.rs`):** every `use`
statement in `src/` and `tests/` must import a module, never an item. The only
permitted exceptions are named traits (imported when method syntax requires them
in scope), internal X-macros, and `use super::*;` inside test modules.

---

## 2. Naming conventions

Extracted from what all 12 language surfaces do consistently — see
`src/surfaces/{rust,python,cpp,java,go,markdown,yaml,json,toml,typst,javascript,kotlin}.rs`.

- **Surface struct**: `<Lang>Surface`, a unit struct
  (`#[derive(Debug, Default)] pub struct RustSurface;`). One per file, with its
  `impl LanguageSurface` and `impl DeclaresFacets` in that same file.
- **Native config struct**: `<Tool>Config` (e.g. `RustfmtConfig`), implementing
  `NativeConfig` with `const FILE_NAME: &'static str` set to the real file name
  the tool reads (e.g. `.rustfmt.toml`). One struct per managed file.
- **Test functions**: `test_<behavior_under_test>`, describing the behavior, not
  just the function (`test_get_surface_by_name_canonical_and_aliases`, not
  `test_get_surface`).
- **Registry/lookup functions**: free functions in `registry.rs`
  (`get_surface_by_name`, `detect_surfaces_smart`) rather than static methods on
  `SurfaceRegistry` when no registry instance is needed.
- **Predicate methods (tier 2, enforced by
  `test_is_predicate_methods_carry_must_use` in `src/lib.rs`):** `is_*`
  returning `bool` carries `#[must_use]`. The scan normalizes visibility,
  `const`/`async`/`unsafe` modifiers and joins multi-line signatures; its first
  version matched only single-line `pub fn` signatures and stayed green with
  `#[must_use]` deleted from `ExitStatus::is_clean` (`#201 [pre-recreation]`).

---

## 3. Documentation requirements

The doc lints (`missing_docs`, `clippy::missing_errors_doc`,
`clippy::missing_panics_doc`) are enabled in `Cargo.toml`'s `[lints]` table,
which reaches every target (lib, bin, and each `tests/*.rs` crate). On top of
`rust-guide`:

- Every `pub mod` declaration carries an outer `///` doc comment above the `mod`
  keyword, though `missing_docs` does not require it. **Tier 2 (enforced by
  `test_pub_mod_declarations_carry_doc_comments` in `src/lib.rs`).**
- The `//!` header is enforced by `test_files_carry_module_doc_comment` in
  `src/lib.rs`. A §1 `tests.rs` file is exempt.
- **Doc comments on `JsonSchema`-derived types are published output.**
  `schemars` lifts a doc comment on a type or field deriving `JsonSchema` (such
  as `LangConfig` in `src/config.rs`) verbatim into
  `schema/formality.schema.json`, where users and IDE tooltips read it. Editing
  one changes the published schema and fails `tests/schema_drift.rs` until the
  schema is regenerated. Never edit one incidentally; batch prose fixes onto a
  schema change already happening for a functional reason. Internal rationale
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

### Raw vs. ANSI-stripped text and offsets

**Rule (tier 3):** a byte offset derived from raw, ANSI-bearing text never
indexes ANSI-stripped text, and vice versa. Where an operation spans both
(classify on stripped text, splice raw text), the translation goes through a
shared, unit-tested helper (such as `src/ui/paths::char_before_ansi`), never ad
hoc slicing or re-stripping at the point of use.

Stripping escapes also strips their payloads: an OSC-8 hyperlink
(`\x1b]8;;url\x1b\...`) carries the URI inside the escape. "Nothing precedes
this offset" in stripped text does not hold in raw text, and splicing on that
basis can land inside the payload. Reviewers check coordinate integrity in any
UI or diagnostic string manipulation.

**Motivating cases** (`src/ui/paths.rs`, `#182` / PR `#191`): first,
`relativize_line` checked token boundaries on raw text, saw the trailing `m` of
an SGR sequence as a path character, and left colored lines un-relativized; then
the fix stripped the prefix before checking, which erased an OSC-8 URI and
spliced the hyperlink target. `char_before_ansi` steps back over only complete
adjacent escape sequences.

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

This crate follows `rust-guide` §3C: per-module `thiserror` error hierarchies,
leaf modules own their failures, and parents aggregate with
`#[error(transparent)]`.

- Errors derive `thiserror::Error` without stuttering type names
  (`config::Error`, `surfaces::Error`, `errors::Error`).
- `errors::Error` (aliased as `FormalityError` for backward compatibility) is
  the top-level enum aggregating subsystem errors (`Config`, `Git`, `Surface`,
  `Io`, plus `InvalidCli(String)`). A missing tool is not an error: it is the
  run status `SurfaceStatus::ToolMissing`.
- A new failure in an existing subsystem adds a variant to that subsystem's
  `Error` enum, not a new top-level variant and not a bare `String`.
  `InvalidCli(String)` is the deliberate exception for CLI usage errors.
- `impl From<Error> for ExitStatus` (and `From<&Error>`) maps every variant to
  `ExitStatus::Error` (exit code 2). A case needing a different exit status is a
  design decision to raise, not a special case to add.
- User-facing rendering goes through `render_diagnostic()` /
  `print_diagnostic()` (`[ERR]` prefix), not an ad hoc `eprintln!`.

**Tier 2 (enforced by `test_all_inner_error_enums_implement_std_error` in
`src/errors.rs`):** every `Error` variant's inner type implements
`std::error::Error`.

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
`test_unit_tests_do_not_dispatch_full_commands` (`src/lib.rs`) forbids them from
calling full command runners at all; `tests/` sees only the public API, so
reviewers check it there.

**Motivating case:** #291 / PR #400 — `test_relative_root_resolves_to_absolute`
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
`test_run_doctor_folds_install_into_tally_before_rendering_footer` scanned to
EOF for the substring `scan.`, and stayed green when the bug was reintroduced as
`ToolTally::from_scan(&scan)`.

### File-matching invariant tests: strip comments before matching

**Rule (tier 3):** a test asserting on a file's contents (workflow YAML, config,
source) strips comments before matching whenever that file's comments quote the
guarded text. Applies to workflow guards
(`tests/release_workflow_local_edits.rs`), source scans
(`src/engine/doctor/tests.rs`), and any self-documenting file.

**Motivating case:** `#164` / PR `#199` — deleting the live `fetch-depth: 0`
line from `release.yml` failed no test, because a `# LOCAL EDIT` comment quoted
it verbatim.
