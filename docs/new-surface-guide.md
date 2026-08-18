# Adding a New Language Surface

This is the step-by-step walkthrough for adding a 13th `LanguageSurface` to
`fml`, using the pattern established by Wave 5 (Go, JavaScript/TypeScript, Java,
Kotlin — `src/surfaces/{go,javascript,java,kotlin}.rs`, PRs #85/#87/#86/#84).

> **Note on "self-registration":** `fml` does not use a runtime plugin registry
> (no `inventory`/`linkme`/dynamic loading). "Self-registering" a surface means
> adding it to two static compile-time lists — a genuinely small, mechanical
> diff, not a new subsystem — described in step 4 below.

## 1. Create `src/surfaces/<lang>.rs`

Implement two traits on a unit struct
(`#[derive(Default, Clone)] pub struct FooSurface;` is the pattern every
existing surface follows):

### `DeclaresFacets`

```rust
impl DeclaresFacets for FooSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Configurable, // or Fixed("...") / Unsupported
      Facet::IndentWidth => FacetSupport::Configurable,
      Facet::LineLength => FacetSupport::Configurable,
      Facet::QuoteStyle => FacetSupport::Unsupported,
      Facet::TrailingComma => FacetSupport::Unsupported,
      Facet::ImportSort => FacetSupport::Configurable,
      Facet::ProseWrap => FacetSupport::Unsupported,
      Facet::Edition => FacetSupport::Unsupported,
      Facet::Standard => FacetSupport::Unsupported,
    }
  }
}
```

There's no default fallback arm — the `match` must be exhaustive over
`Facet::ALL`, by design, so a new surface can't accidentally skip declaring a
position on any facet. Every arm needs an honest answer: does the real tool
support configuring this, does it enforce one fixed value (document _why_ in a
comment, the way `go.rs`/`java.rs`/`kotlin.rs` do), or is the concept simply
absent for this language. Update [docs/facet-rosetta.md](facet-rosetta.md)'s
table with the same row once this is decided.

### `LanguageSurface`

```rust
impl LanguageSurface for FooSurface {
  fn name(&self) -> &'static str { "foo" }
  fn aliases(&self) -> &[&'static str] { &[] } // alternate names, e.g. markdown -> "md"
  fn file_extensions(&self) -> &[&'static str] { FOO_EXTENSIONS }
  fn detect(&self, root: &Path) -> bool { /* any file_extensions() present under root? */ }
  fn tool_info(&self, config: &ResolvedLangConfig) -> Vec<ToolInfo> { /* binaries + install hints */ }
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult { /* Smart Format pass */ }
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult { /* linter invocation */ }
  fn supports_lint_fix(&self) -> bool { true } // only if the linter has a real --fix mode
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult { /* native config generation */ }
  fn clone_box(&self) -> Box<dyn LanguageSurface> { Box::new(self.clone()) }
}
```

Key implementation notes drawn from the existing 12 surfaces:

- **Smart Format ordering (Rule #7)**: `format()` must leave files in a state
  that won't immediately fail a trivial structural lint check. If the tool
  ecosystem separates "mechanical fix" (import sorting, blank-line
  normalization) from "layout formatting", run the mechanical pass first, inside
  `format()` — see Python's `ruff check --select I --fix` → `ruff format`, or
  Markdown's `markdownlint-cli2 --fix` → `prettier --write`. If one tool does
  both in a single invocation (Go's `goimports -w`, Kotlin's `ktlint -F`,
  JS/TS's `biome check --write --linter-enabled=false`), a single call is fine —
  don't invent a fake two-stage split.
- **`fml lint --fix` vs. `fml fmt`**: `format()` must never apply _semantic_
  lint fixes (unused-import removal, rule-based rewrites) — that's what
  `lint(ctx, fix: true)` and `supports_lint_fix()` are for. If the tool has no
  real auto-fix mode for lint violations (Checkstyle, yamllint, taplo lint), set
  `supports_lint_fix()` to `false` (the trait default) and leave `lint()`'s
  `fix` parameter effectively a no-op for that surface.
- **`check_binary_exists("<binary>")`** guards every tool invocation and returns
  `SurfaceStatus::ToolMissing` with an actionable `install_hint` — copy the
  pattern from any existing surface's `format()`/`lint()` rather than inventing
  new error handling.
- **`tool_info()`** feeds `fml doctor` and `fml install` — list every binary the
  surface depends on (formatter and linter separately if they're different
  binaries), each with `is_required_for_fmt`/`is_required_for_lint` set
  accurately so `fml doctor --all` reports gaps precisely.
- **Diff-check temp files**: if `format()` needs to check "would this change
  anything" without mutating the real file (used by `fmt --check`), route
  through the shared tempcopy helper in `src/surfaces/mod.rs` — it preserves the
  original file extension (see #90/PR #86) so extension-sensitive tools like
  Biome/google-java-format/ktlint don't reject the scratch file.

## 2. Add typed per-language options (if the tool has real config knobs)

In `src/config/options.rs`, add a `FooOptions` struct following the existing
pattern
(`Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema`, a
`merge()` method, an `is_empty()` method). If the tool has no config knobs
beyond the shared facets (like TOML, JSON, Typst, Kotlin today), still add an
empty struct — it keeps every surface consistent in
`LangTable`/`ResolvedLangConfig` and gives you a home for future options without
a breaking config shape change later.

Wire the new struct into `src/config/mod.rs`:

- Add `pub foo: Option<FooOptions>` to both the top-level `LangTable` and the
  resolved-config struct.
- Add a `foo_options(&self) -> Option<FooOptions>` accessor alongside the
  existing `java_options()`/`go_options()`/`kotlin_options()` methods.
- Wire resolution (defaults → user config → project config merge) the same way
  the existing per-language accessors do.

## 3. Native config generation (`fml sync`)

If the tool reads a persisted config file (`.rustfmt.toml`, `ruff.toml`,
`biome.json`, `.golangci.yml`, `checkstyle.xml`, …), implement `sync_config()`
to:

1. Render the canonical globals + resolved `FooOptions` into that file's native
   format.
2. Prefix the generated file with the standard sentinel comment (see any
   existing `sync_config()` for the exact banner text) so `fml sync` can detect
   drift and distinguish a formality-managed file from a manually edited one
   (`[MANUAL]` diagnostic).
3. On `check: true`, compare against the file on disk instead of writing,
   returning `ConfigDrifted` if they differ.

If the tool has no config file and is driven entirely by CLI flags (Typst) or
reads from the shared `.editorconfig` instead of its own file (Kotlin/ktlint),
`sync_config()` can be a thin no-op/pass-through — follow whichever existing
surface matches your tool's actual config story most closely.

## 4. Register the surface

Two lists in `src/surfaces/mod.rs` must both include the new surface — this is
the entire "self-registration" step:

```rust
// 1. The canonical constructor table (used wherever a fresh registry is built):
pub static DEFAULT_SURFACE_CONSTRUCTORS: &[SurfaceConstructor] = &[
  // ...existing 12...
  create_surface::<foo::FooSurface>,
];

// 2. SurfaceRegistry::default() (kept in lockstep with the table above):
impl Default for SurfaceRegistry {
  fn default() -> Self {
    let mut reg = Self::empty();
    // ...existing 12 reg.register_surface::<...>() calls...
    reg.register_surface::<foo::FooSurface>();
    reg
  }
}
```

Also add `pub mod foo;` near the top of `src/surfaces/mod.rs` alongside the
other surface modules, and update any fleet-count comments/doc-strings that
mention "12 surfaces" (`SurfaceRegistry::new()`'s doc comment, `cli.rs`'s
`long_about`, this repo's README) — searching for the literal string `"12"` near
"surface" or "language" in `src/` and `README.md` will find them.

## 5. Tests

Every existing surface carries a `#[cfg(test)] mod tests` block in its own file
(or a co-located `_tests.rs`) covering at minimum: `facet_support()` for every
`Facet::ALL` entry, `detect()` against a fixture with/without matching files,
and `supports_lint_fix()`. `src/surfaces/mod.rs`'s own test suite
(`test_surface_supports_lint_fix`, fleet-count assertions) also needs a new
assertion line for the added surface. Run the full presubmit gate before opening
a PR:

```bash
cargo test --lib -q
cargo clippy -q
cargo run -q -- fmt --check   # dogfood: the new surface's own source should already be clean
```

## 6. Regenerate the schema

`formality.toml`'s JSON Schema (`schema/formality.schema.json`) is generated
from the Rust types via `schemars`, so a new `FooOptions` struct or `LangTable`
field must be reflected there:

```bash
cargo run -q -- schema -o schema/formality.schema.json
```

Commit the regenerated schema alongside the surface implementation — CI enforces
that the checked-in schema matches what the binary actually generates.

## 7. Update documentation

- Add a section to [docs/language-surfaces.md](language-surfaces.md) following
  the existing per-surface format (tools, Smart Format behavior, managed config,
  `[lang.foo]` options, facet table row, `supports_lint_fix`).
- Add the surface's row to [docs/facet-rosetta.md](facet-rosetta.md)'s table.
- Add the surface to README.md's supported-surfaces table and the
  `.artifacts/PLAN.md` formatting matrix (§3) if you're working from that
  orchestration plan.

## Reference: an existing surface end-to-end

`src/surfaces/kotlin.rs` is the smallest complete example in the fleet (one
tool, `ktlint`, doing both format and lint) — start there if you want the
shortest real implementation to model against. `src/surfaces/javascript.rs` is
the best example of a surface with real typed options (`JavaScriptOptions`) and
a native JSON config file (`biome.json`) if your new tool needs either.
