# Style Guide

This is `fml`'s style guide: how code in this repository is written, organized,
and tested, beyond what any single PR review remembers from one session to the
next. It exists so a standard survives past the audit that found it — see
`.agents/orchestrate.md` §4 for the process obligation this document backs (a QA
reviewer who finds an uncovered violation promotes the rule here, or files a
follow-up to encode it, rather than fixing one PR and moving on).

> The `#N` citations throughout this document predate the 2026-08-26 repo
> recreation and resolve to unrelated new issues — see
> [`docs/INDEX.md`](INDEX.md#note-on-pre-recreation-issuepr-numbers).

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
   already cover, and never repeats their behavior. Checked pre-merge on every
   PR via `fml fmt --check` / `fml lint`
   (`cargo clippy --all-targets -- -D warnings`), which is `fml` dogfooding
   itself in the `Formality Dogfooding` / `Library Tests` jobs of
   `.github/workflows/pr-check.yml` (`ci.yml` re-runs the same checks on `main`
   as a post-merge safety net — see #184 [pre-recreation]).
2. **Repo-local test assertion** — a `#[test]` (part of the full test suite that
   runs in the `Library Tests` PR check, gating every PR before merge) that
   walks the filesystem, the surface registry, or another in-crate side-table
   and fails if the rule is violated. The established pattern is
   `src/surfaces/registry.rs`'s fleet-consistency tests from
   `#113 [pre-recreation]` (`test_all_fleet_surfaces_present`,
   alias/case-insensitive lookup) — reuse that mechanism for a new
   mechanically-checkable rule rather than inventing another one.
   `test_no_stray_test_files_outside_sanctioned_pattern` in `src/lib.rs` (added
   alongside this document) is the same pattern applied to the module/file
   hierarchy rule in §1 below.
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
`#82 [pre-recreation]`'s
`refactor(surfaces): split src/surfaces/mod.rs into cohesive sub-modules` commit
introduced the sibling `<name>_tests.rs` files (`glob_tests.rs`, `mod_tests.rs`,
`registry_tests.rs`, `sync_tests.rs`, `tooling_tests.rs`;
`config/facets_tests.rs` predates it); issue `#120 [pre-recreation]`'s
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

`src/lib.rs` no longer carries the `DEPRECATED / STALE ALIAS` block of top-level
module re-exports (`pub use commands::doctor;` and similar) that used to
preserve pre-reorganization `crate::foo::*` paths. It was removed in
`#133 [pre-recreation]`'s sweep once an audit confirmed nothing used it: no
internal call site referenced the short form, and this crate's own integration
tests (`tests/*.rs`) already addressed everything through the canonical
structural path (`fml::surfaces::editorconfig::generate_editorconfig`, not
`fml::generate_editorconfig`) except the two items kept below. **Tier 2
(enforced by `test_internal_code_uses_canonical_module_paths` in
`src/lib.rs`):** new internal code always spells out the canonical, structural
path (e.g. `crate::ui::table`, `crate::engine::version`) — never a crate-root
shortcut, even where one would resolve to the same item.

Two crate-root re-exports remain, and both are genuinely load-bearing rather
than compatibility shims: `pub use config::SCHEMA_VERSION;` and
`pub use config::schema::generate_schema;` are reached as `fml::SCHEMA_VERSION`
/ `fml::generate_schema` by this crate's own `tests/integration_tests.rs` and
`tests/schema_drift.rs`. A re-export earns a place at the crate root by being
actually reached that way by real code — not by habit, and not "just in case."

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
- **Predicate methods (tier 2, enforced by
  `test_is_predicate_methods_carry_must_use` in `src/lib.rs`):** `is_*`
  returning `bool` carries `#[must_use]` (`SurfaceResult::is_success`,
  `is_violation`, `is_error`; `ExitStatus::is_clean`, `is_violations`,
  `is_error`; `FacetSupport::is_configurable`, `is_fixed`, `is_unsupported`).
  Promoted from tier 3 during `#133 [pre-recreation]`'s sweep: a text-scan
  `#[test]`, the same filesystem-walk mechanism as
  `test_no_stray_test_files_outside_sanctioned_pattern`, is enough to check
  every `is_*` predicate with a `-> bool` signature in the tree — the sweep's
  own audit turned up two real misses (`DeclaresFacets::is_facet_configurable`,
  `surfaces::java::is_aosp_style`), both fixed in the same PR. A
  `#201 [pre-recreation]` QA review then proved the first version of this scan
  didn't actually check the rule it claimed to: it matched only a single-line
  `pub fn is_*(...) -> bool` signature, so it stayed green with `#[must_use]`
  deleted from `ExitStatus::is_clean` (a `pub const fn`, and this rule's own
  named exemplar) and was blind to `pub(crate)`/`pub(super)` visibility and
  multi-line signatures — missing three more real violations
  (`surfaces::tooling::is_available`,
  `config::facets::is_value_compatible_with_fixed`,
  `surfaces::glob::is_excluded_normalized`) in the process. The scan now
  normalizes visibility/`const`/`async`/`unsafe` modifiers and joins a signature
  across lines before checking for `-> bool`; the fix was verified by re-running
  the delete-`#[must_use]`-from-`is_clean` check and confirming the test now
  fails.
- **Tier 3 (promote to tier 2 if a mechanical check is found):** the broader
  "pure getter or predicate (no I/O, no mutation) carries `#[must_use]`" case
  beyond the `is_*` family above stays tier 3 — a text scan can reliably spot
  the `is_*` naming pattern, but can't tell a pure getter from an impure one by
  name alone. `clippy::must_use_candidate` is allow-by-default in this crate's
  lint set, so this isn't already tier 1 either.

---

## 3. Documentation requirements

Tier 1 already governs the bulk of this: `src/lib.rs` sets
`#![warn(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]`
crate-wide (landed via `#121 [pre-recreation]`, this issue's blocker). That
means:

- Every public item needs a `///` doc comment, or the build warns (`-D warnings`
  in CI makes that a hard failure).
- Every public function returning `Result` documents its error conditions (an
  `# Errors` section, or a doc sentence covering it for a short function).
- Every function that can panic documents when (`# Panics`), or doesn't panic.

On top of that tier-1 floor, this codebase's own convention:

- Every `pub mod` declaration in `src/lib.rs` and `src/surfaces/mod.rs` etc.
  carries an outer `///` doc comment one line above the `mod` keyword describing
  what the module is for, even though `missing_docs` doesn't require this for
  module declarations specifically (see the `pub mod cli;` block at the top of
  `src/lib.rs`, and the per-surface `pub mod <lang>;` block in
  `src/surfaces/mod.rs`). **Tier 2 (enforced by
  `test_pub_mod_declarations_carry_doc_comments` in `src/lib.rs`).**
- An inline `#[cfg(test)] mod tests` block, or a directory module's sibling
  `mod tests;` declaration (§1's exception), carries
  `#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]`
  directly under the `#[cfg(test)]` attribute, opting test code out of the
  crate-wide tier-1 doc lints above — test functions document themselves by name
  (§2). A test module that omits it will fail CI's `-D warnings` the moment it
  adds a `pub` item or a `Result`-returning helper. **Tier 2 (enforced by
  `test_test_modules_carry_allow_doc_lints` in `src/lib.rs`).**
- Every file with meaningful crate-level content (not just re-exports) opens
  with a `//!` module-level doc comment summarizing what lives in it (see the
  top of `src/surfaces/mod.rs`, `src/surfaces/registry.rs`). The one exemption
  is a `tests.rs` sibling file under §1's directory-module split — test-only
  content already opted out of the doc lints by the bullet above, the same way
  an inline `mod tests` block carries no `//!` of its own. **Tier 2, enforced by
  `test_files_carry_module_doc_comment` in `src/lib.rs`.** Promoted from tier 3
  during `#201 [pre-recreation]`'s QA follow-up to `#133 [pre-recreation]`'s
  sweep: that PR's own audit omitted §3 entirely, and a QA review found the rule
  ~80% unmet across the tree (41 of 50 files at the time) — the exact
  silent-drift failure mode tier 2 exists to prevent, so this was pushed up
  rather than re-documented as still tier 3.
- A non-obvious architectural choice gets a comment explaining _why_, not just
  _what_ — e.g. the `Arc`-sharing rationale on `ExecutionContext` (§4 below), or
  the comment above `src/lib.rs`'s two remaining crate-root re-exports
  explaining why each is genuinely reached that way rather than a leftover
  alias. A comment restating what the next line of code already says is not
  this. **Tier 3 (promote to tier 2 if a mechanical check is found)** —
  "non-obvious" isn't mechanically detectable without deeper analysis than a
  text scan gives.
- **Doc comments on `JsonSchema`-derived types (tier 2 / tier 3):** A doc
  comment on a type or field that derives `JsonSchema` (such as `LangConfig` and
  configuration structs in `src/config/mod.rs`) is **published output**, not an
  internal note: `schemars` lifts it verbatim into
  `schema/formality.schema.json`'s `description` properties, where users and IDE
  tooltips read it directly. Editing one is a schema change: it changes the
  generated schema JSON, triggers a failure in `tests/schema_drift.rs`, and once
  regenerated forces a `SCHEMA_VERSION` progression (`src/config/schema.rs`).
  Per `docs/release.md`, cutting a new `sX.Y` schema release marks every
  existing user configuration pinned to an earlier schema version (e.g.
  `#:schema .../s1.1/...`) as **Stale**. Never reword or edit a doc comment on a
  `JsonSchema` type incidentally — batch prose corrections onto a schema bump
  that is already occurring for a functional reason. Internal implementation
  rationales, notes, and issue-tracker syntax (`(Fixes #N)`, `TODO`, internal
  shorthand) belong in regular code comments (`//`) at call sites or inside
  function bodies, **never** in `///` doc comments on schema types. When
  `tests/schema_drift.rs` fails on a doc-comment edit, that failure is the gate
  working as intended, not a false positive. **Motivating case:** In `#150` / PR
  `#194`, an initial commit reworded the doc comments on
  `LangConfig::extra_args` and `ResolvedLangConfig::extra_args` to explain
  multi-tool flag forwarding, inadvertently embedding an internal `(Fixes #150)`
  reference into public schema tooltip descriptions and failing
  `schema_drift.rs`. Reverting the doc-comment changes and moving the rationale
  to call-site code comments kept the fix strictly scoped without forcing an
  unintended schema release.
- **External tool behavior claims and citations (tier 3, promote to tier 2 if a
  mechanical check is found):** A doc comment, ADR, code rationale, or
  user-facing diagnostic that asserts how an external tool behaves (e.g. exit
  codes, flag syntax, duplicate flag handling, error formatting) must cite a
  concrete reproduction actually run against the version this repo pins in
  `Cargo.toml` or `docs/language-surfaces.md`. "The tool does X" is an
  empirical, testable claim, not background intuition. This obligation applies
  equally to premises inherited from issue descriptions: restating an unverified
  premise in code or documentation adopts it as fact. Where the reproduction is
  cited depends on scope: record the exact command line, tool version, and
  output in the PR body and review comments, in the relevant ADR for
  cross-cutting design decisions, or directly in a test or code comment for
  local guards. **Motivating case:** In `#173` / PR `#197`, the issue asserted
  that passing `--linter-enabled` via `extra_args` would override `fml`'s flag
  and re-enable Biome's linter on the format path, causing lint diagnostics to
  be misreported as execution errors (`[ERR]`). The PR adopted that premise in
  its code comments, user-facing diagnostic, and draft ADR. A QA review ran
  Biome 2.5.10 (the pinned version) directly and disproved the entire premise:
  Biome strictly rejects duplicate `--linter-enabled` flags with an immediate
  error before parsing any value, so the scenario was unreproducible and no lint
  finding was ever misclassified. In the same review round, markdownlint-cli2's
  flag handling was assumed to fail loudly on unknown flags, whereas testing
  against v0.23.2 proved unknown flags are consumed as globs. Testing against
  the pinned version replaces plausible assumptions with empirical facts.

---

## 4. Architectural patterns

### `ExecutionContext` and `Arc`-sharing

`ExecutionContext` (`src/surfaces/mod.rs`) is built once per surface, per
invocation, and the `Runner` (`src/engine/runner/mod.rs`) dispatches all matched
surfaces in parallel via `rayon::par_iter`. Three of its fields —
`root: Arc<PathBuf>`, `paths: Arc<Vec<PathBuf>>`, and
`global_config: Arc<ResolvedGlobalConfig>` — are wrapped in `Arc` because every
surface in that parallel dispatch sees the _same_ values. For `paths` and
`global_config` this avoids a real cost: without the `Arc`, each of the
(currently) 12 surfaces would deep-clone the full candidate path list and the
global config on every invocation, instead of a cheap refcount bump. `root` is
wrapped for consistency with those two fields, not for a comparable saving —
it's one short `PathBuf`, so the copy it avoids is small; don't cite `root` as
precedent for `Arc`-wrapping the next small `Copy`-ish field on this struct,
only for a field with a real per-surface cost like `paths`/`global_config`.
`Arc<PathBuf>`, not `Arc<Path>`, is the deliberate choice for all three
`Arc`-wrapped fields: each wraps the type's natural owned form
(`Arc<Vec<PathBuf>>`, `Arc<ResolvedGlobalConfig>`), not the
`Arc<[T]>`/`Arc<str>`-style unsized-coercion pattern, so `root` follows suit
rather than special-casing to `Arc<Path>`. `lang_config`, by contrast, is a
plain owned `ResolvedLangConfig` — it's genuinely per-surface
(`config.resolve_for_lang(surface.name())`), so there's nothing shared to `Arc`
there. (`root` was converted from a plain `PathBuf` to `Arc<PathBuf>` during
`#133 [pre-recreation]`'s sweep, once an audit confirmed it fit this exact
pattern — every production read is `Path`-like usage reached through
`Deref`/`AsRef`, so call sites needed only `ctx.root.as_path()` in place of
`&ctx.root`.)

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

### Manifest probes vs. surface detection

**Rule (tier 3, promote to tier 2 if a mechanical check is found):** Probing for
a project build manifest (`Cargo.toml`, `go.mod`, `package.json`, etc.) must use
`.is_file()`, never `.exists()` — a directory can legitimately share the
manifest's name (e.g. a directory named `Cargo.toml`), which `.exists()` falsely
accepts.

Furthermore, the distinction between tool execution preflights and surface
detection is load-bearing:

1. **Tool execution preflights / workspace member guards:** When a surface
   decides whether a tool can run (such as `RustSurface::lint` checking for
   `Cargo.toml` or `GoSurface::lint` checking for `go.mod`), it must walk
   ancestor directories using `crate::surfaces::glob::find_manifest_upwards`. In
   a Cargo workspace member or Go submodule, the build manifest legitimately
   lives above `ctx.root`, and the underlying CLI tool (`cargo clippy`,
   `go test`, etc.) walks parent directories to find it. Checking only
   `ctx.root` produces false `ExecutionError` failures when `fml` is invoked
   from a subcrate.
2. **Surface detection (`LanguageSurface::detect`):** Probing in `detect(&Path)`
   **must remain root-only** (`root.join(...)`). `detect` answers "is this root
   a project of language X?" If `detect` walked parent directories, running
   `fml` against any nested subdirectory or subcrate of a repository would
   falsely activate surfaces for every ancestor project type in the tree.

**Motivating case:** In `#185` / PR `#193`, `RustSurface::lint` checked only
`ctx.root.join("Cargo.toml").exists()`, despite its user error message claiming
to check parent directories. Running `fml lint` from a subdirectory of a Cargo
workspace failed with an `ExecutionError`, even though `cargo clippy` walked
ancestors and ran successfully. `find_manifest_upwards` was extracted to walk
ancestors using `.is_file()`, while `detect` was kept strictly root-only to
prevent false cross-surface activations.

### Raw vs. ANSI-stripped text and offsets

**Rule (tier 3, promote to tier 2 if a mechanical check is found):** A byte
offset derived from raw, ANSI-bearing text may **never** index or slice
ANSI-stripped text, and a byte offset derived from ANSI-stripped text may
**never** index or slice raw text. Where an operation spans both representations
— such as eligibility or classification decided on stripped text, followed by
splicing or rewriting performed on raw text — the coordinate translation must be
handled by a shared, dedicated, unit-tested helper (such as
`src/ui/paths::char_before_ansi`), never by ad hoc inline slicing or secondary
stripping at the point of use.

**OSC sequence payload caveat:** Stripping ANSI escapes via `strip_ansi_escapes`
or similar filters strips escape _payloads_ as well as formatting bytes. For
example, an Operating System Command sequence like an OSC-8 hyperlink
(`\x1b]8;;url\x1b\...`) contains meaningful text (the `file://` or `http://`
URI) inside its escape sequence. Blindly stripping escapes discards that text
entirely; asserting that "no characters precede an offset" in stripped text does
not mean no characters precede it in raw text. Splicing raw text based on a
stripped-prefix boundary check can land squarely inside an escape payload,
corrupting URLs and escape sequences.

Text scanners and the type system cannot readily distinguish a raw `&str` from
an ANSI-stripped `&str` without a newtype abstraction. Reviewers must verify
that any string manipulation in UI or diagnostic rendering preserves coordinate
integrity across ANSI boundaries.

**Motivating cases:** This exact boundary produced two successive bugs in
`src/ui/paths.rs` during `#182` / PR `#191`:

1. In `#182`, `relativize_text` classified line eligibility using ANSI-stripped
   text, but `relativize_line` performed token-boundary checks on raw text. Its
   `is_path_char` check inspected the byte immediately preceding a path
   candidate, which for colored text was the trailing `m` of an ANSI SGR
   sequence (e.g. `\x1b[31m`). Because `m` is an alphanumeric path character,
   the boundary check failed, and colored diagnostic lines were silently left
   un-relativized.
2. The initial fix for `#182` stripped ANSI from the entire prefix preceding the
   match before inspecting the boundary character. However, because
   `strip_ansi_escapes` discards OSC-8 hyperlink payloads, a candidate path
   inside a `file://...` hyperlink URL stripped the preceding URI to empty text,
   falsely passing the boundary check and splicing the hyperlink target. PR
   `#191` resolved this by replacing the blind strip with `char_before_ansi`, a
   backward scan that steps over only complete, adjacent terminal escape
   sequences without consuming OSC payloads.

### Speculative API surface and unused variants/fields

**Rule (tier 3, promote to tier 2 if a mechanical check is found):** Any enum
variant, struct field, or function parameter added in anticipation of a future
issue must have an active **production user in the same PR**, or else be
deferred to the PR that actually implements that future issue. **Unit tests are
not a production user.**

When a PR justifies new API surface or data modeling by claiming that a
downstream issue will need it, that claim is part of the diff's design contract.
Reviewers and authors must verify the claim against the downstream issue's
actual acceptance criteria and technical requirements before introducing the
surface. Speculative machinery designed ahead of its consumer frequently makes
incorrect assumptions about what the consumer needs, leaving behind dead code
that must be reworked when the real consumer arrives.

The compiler's `dead_code` lint catches unconstructed types or unused private
fields across the crate, but it is completely blind to variants, fields, or
parameters that are constructed or accessed solely within `#[cfg(test)]` test
blocks. Reviewers must ensure every new variant or field has a non-test call
site in the crate.

**Motivating case:** In `#177` / PR `#195`, an unused `version_args` field was
refactored into an explicit `VersionProbe` strategy enum so that no registry
entry carried unread values. However, the PR introduced `ProbeArg::ToolPath` on
the speculative premise that `#178` (probing `goimports` via
`go version -m <path>`) would be a registry-only change. When QA audited `#178`,
the premise was false: `go version -m` outputs the Go toolchain version on line
1 and the module version on line 3, requiring custom line-extraction logic
regardless of `ProbeArg`. `ProbeArg::ToolPath` was inert machinery tested only
by unit tests, in the very PR meant to eliminate inert machinery. It was
reverted, leaving `#178` to introduce the parameter alongside its real
production consumer and extractor.

**Speculative documentation (tier 3):** The same rule applies to prose, not just
code. Doc comments, `--help` text, and `docs/` files must describe behavior that
exists today; a planned capability belongs in an issue, written in future tense,
not in present-tense documentation for behavior that has no implementation.
Speculative prose is worse than speculative code because the compiler cannot
flag it — nothing forces stale claims back into sync once the described behavior
changes or never ships. It must never be propped up by a CI-enforced check (a
drift test, a generated table, a schema pin) that has no production consumer of
its own; that check just teaches reviewers to trust the prose instead of the
code. **Motivating case:** `#123` — `fml lsp`'s docs described a child-LSP
router that was never built, kept plausible only by a `--help` string and a
table with no reader but the docs themselves.

### `Runner` dispatch

`Runner::run` (`src/engine/runner/mod.rs`) is the single dispatch point for
every subcommand that acts across surfaces (`fmt`, `lint`, `sync`, `fix`) — it
takes the already-filtered `Vec<Box<dyn LanguageSurface>>`, builds one
`ExecutionContext` per surface, and fans out via `rayon::par_iter`. `Fix` is the
one multi-stage action: it runs `lint(fix: true)` across every surface first,
then `format(check: false)`, then a check-only re-lint of just the surfaces
whose lint pass still reported violations — three separate parallel stages
rather than interleaving lint-then-format per surface, so a fix pass is
lint-fix-everything, then format-everything, then recheck-the-still-dirty, not
format(surface A) before lint(surface B) has even started. The third stage
exists so the reported status and exit code reflect the tree _after_ formatting:
a violation the linter could not auto-fix but the format pass then resolved
(e.g. a long line prettier rewrapped) must not still report `[FAIL]`. A new
subcommand that needs to act across surfaces goes through `Runner::run` with a
new or existing `RunnerAction` variant, rather than writing its own dispatch
loop.

---

## 5. Error handling conventions

Landed via `#119 [pre-recreation]` ("crate-wide error type hierarchy &
standardized exit code / diagnostic pipeline"), this issue's other blocker
having already resolved by the time this document was written. `src/errors.rs`
is the single source of truth:

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

**Tier 2 (enforced by `test_all_inner_error_enums_implement_std_error` in
`src/errors.rs`):** every `FormalityError` variant's inner type implements
`std::error::Error`, so `?`-conversion via `From` stays ergonomic at call sites.

---

## 6. Testing conventions

Beyond test organization (§1) and naming (§2), tests in `fml` must adhere to the
following authoring and verification rules:

### Test specificity: assert what only the tested path produces

**Rule (tier 3, promote to tier 2 if a mechanical check is found):** A test
asserting that a specific code path was reached must assert on an attribute,
message, or artifact that **only that specific path produces**.

Asserting a generic error _variant_ (e.g.
`matches!(status, SurfaceStatus::ExecutionError { .. })`) is vacuous whenever
the surface or subsystem can reach that same variant through other failure
modes. This trap is especially dangerous on **multi-tool surfaces** (such as
Markdown running `markdownlint` and `prettier`, or Python running `ruff` and
`isort`), where multiple underlying CLI tools can fail on the same invalid flag
or broken configuration. If tool B produces `ExecutionError` on the test input
independently of tool A, an assertion matching only `ExecutionError` will pass
even if tool A never receives the input or its forwarding logic is completely
deleted. Tests must assert on distinct error message text (e.g. markdownlint's
unique config error string), inspect the assembled command argv directly, or
unit-test argument builders in isolation.

**Verification discipline:** Verify every path-specific test by temporarily
reverting or commenting out the code under test and confirming the test
**fails**. A test that has never been observed to fail has not been proven to
test anything (see also §6's source-scan and file-matching verification rules
below; these are three instances of the same "break it and watch it fail"
discipline).

**Motivating cases:** In `#150` / PR `#194`, `ctx.lang_config.extra_args` was
forwarded to `markdownlint-cli2 --fix` on the format path. The PR added two
tests asserting `matches!(status, ExecutionError { .. })` when an invalid
`--config` was passed in `extra_args`. However, `prettier` also received
`extra_args` and already failed with `ExecutionError` on that same invalid flag.
When QA temporarily deleted the markdownlint forwarding lines, both new tests
remained 100% green. The trap had even been named one day earlier in PR `#197`
("matching the variant alone would be vacuous"), yet reoccurred in `#194`. The
tests were replaced with deterministic argv builder assertions and a
message-specific check asserting on markdownlint's unique output string.

### Source-scan tests: assert absence, bound the window, prefer runtime assertions

**Rule (tier 3, promote to tier 2 if a mechanical check is found):** A
source-scan test (a tier-2 test that uses `include_str!` to inspect Rust source
code and assert structural properties that the type system cannot express) must:

1. **Assert the absence** of what it guards against, not merely the presence of
   the fix. An assertion checking only that a correct helper or function call is
   present can be satisfied even if the buggy pattern is reintroduced right next
   to it.
2. **Bound its scan window** strictly to the function, method, or block under
   test (for instance, slicing between the function signature and its closing
   brace), never scanning to end-of-file (EOF). Scanning to EOF allows helper
   functions, unrelated methods, or future code located further down the file to
   accidentally satisfy (or falsely trip) the scan.
3. **Be verified by reintroducing the defect** and confirming that the test
   **fails**.

**Prefer runtime assertions:** Source-scan tests are inherently fragile against
benign formatting and refactoring changes. Where a property can be expressed via
a runtime assertion (such as inspecting an execution report, return value, or
side-effect), always prefer the runtime assertion. Reach for `include_str!`
source scans only when the invariant is strictly structural (e.g. execution
order within an untestable I/O loop) and cannot be exercised in a hermetic unit
test. Always document the rationale for the scan in the test's doc comment.

**Motivating case:** In `#106` / PR `#196`,
`test_run_doctor_folds_install_into_tally_before_rendering_footer` was
introduced to ensure `fml doctor` folded installer results into its tool tally
before rendering the summary footer. The initial guard checked that
`tally.apply_install_run` appeared before `tally.render`, and asserted that the
substring `scan.` was absent between them. However, the scan ran to EOF, and
when a reviewer reintroduced the bug verbatim with
`let tally = ToolTally::from_scan(&scan);` right before `render`, the test
**still passed**: `&scan)` does not contain the substring `scan.`. The test was
updated to bound its slice strictly to `run_doctor`'s closing brace, strip
comment lines, and assert the absence of both `scan.` and `from_scan`.

### File-matching invariant tests: strip comments before matching

**Rule (tier 3, promote to tier 2 if a mechanical check is found):** A
repo-invariant test that asserts on the contents of a file (such as checking
workflow YAML, config files, or source code) must **strip comments before
matching**, whenever that file contains comments or documentation describing the
very pattern being asserted. Otherwise, explanatory comments quoting the guarded
text will satisfy the assertion even when the live configuration or code has
been deleted or reverted.

This applies broadly across the codebase: to workflow guards (e.g.
`tests/release_workflow_local_edits.rs`), source code scans (e.g.
`src/commands/doctor/tests.rs`), and any test scanning files that contain
self-documenting prose.

**Verification discipline:** Always verify file-matching guards by temporarily
deleting or corrupting the live line in the target file and confirming that the
test **fails**. Like §6's test specificity and source-scan rules above, an
invariant test that has never failed in the presence of the defect provides no
protection.

**Motivating case:** In `#164` / PR `#199`,
`tests/release_workflow_local_edits.rs` was added to guard three local edits in
`.github/workflows/release.yml` against silent reversion by
`cargo-dist generate`. Each edit was documented by a `# LOCAL EDIT (issue #134)`
comment that quoted what it replaced. When comment-stripping was tested,
deleting the live `fetch-depth: 0` line from the workflow produced zero test
failures: the comment explaining the edit quoted `fetch-depth: 0` verbatim,
satisfying the naive substring check. Stripping comment lines before evaluation
ensured the test only inspected live configuration.

---

## Amending this document

Per `.agents/orchestrate.md` §4: if a QA review finds a real violation this
document doesn't cover, that PR's sign-off isn't complete until the rule is
either promoted into this document (tier 1/2 if mechanically checkable, in the
same PR) or filed as a small, scoped follow-up issue to encode it. Don't leave a
newly-found standard undocumented, and don't turn the follow-up into another
open-ended sweep.
