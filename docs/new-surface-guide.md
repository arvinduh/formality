# Adding a New Language Surface

This is the step-by-step walkthrough for adding a new `LanguageSurface` to
`fml`, following the architecture and patterns established across the fleet
(such as Go, JavaScript/TypeScript, Java, and Kotlin in `src/surfaces/`).

> **Note on "self-registration":** `fml` does not use a runtime plugin registry
> (no `inventory`/`linkme`/dynamic loading). "Registering" a surface means
> declaring the module in `src/surfaces/mod.rs` and registering the surface type
> in `SurfaceRegistry::default()` in `src/surfaces/registry.rs`.

---

## Author Checklist

Adding a language surface touches the following touchpoints across the
repository:

- [ ] **1. Surface implementation**: `src/surfaces/<lang>.rs` implementing
      `LanguageSurface` + `DeclaresFacets`, exposed via `pub mod <lang>;` in
      `src/surfaces/mod.rs`.
- [ ] **2. Tooling installer chains**: `src/surfaces/tooling.rs`
      (`InstallMethod` constant slice and match arm in `install_chain_for()`).
- [ ] **3. Per-language configuration**:
  - `src/config/options.rs`: Typed `FooOptions` struct (`merge()`,
    `is_empty()`).
  - `src/config/lang_table.rs`: Row in `lang_options_table!` X-macro.
  - `src/config/mod.rs`: `LangConfig` and `ResolvedLangConfig` struct fields.
- [ ] **4. Registry wiring**: `src/surfaces/registry.rs`
      (`SurfaceRegistry::default()` registration call).
- [ ] **5. Soft / optional tables**:
  - `src/commands/lsp.rs`: `CHILD_LSP_REGISTRY` entry for child language server
    (if applicable).
  - `src/commands/lsp_diagnostics.rs`: a `parse_<tool>_*` / `<tool>_diagnostics`
    pair plus a `diagnostics_runner_for_surface()` arm, so `fml lsp` publishes
    one `Diagnostic` per violation rather than a single generic warning. This is
    **not** optional for a surface that has a linter —
    `test_every_surface_except_json_has_a_structured_parser()` fails if a newly
    registered surface has no arm. A format-only surface with no linter at all
    (`json`) is the one sanctioned exception, named explicitly in that test.
  - `src/surfaces/editorconfig.rs`: `glob_for_surface()` match arm and
    `CANONICAL_FLEET_ORDER` entry.
  - Prose surface counts in doc comments and documentation (e.g.
    `SurfaceRegistry::new()` doc comment).
- [ ] **6. Test coverage** (see
      [Style Guide §1](style-guide.md#1-modulefile-hierarchy) for the
      inline-`mod tests`-vs-sibling-`tests.rs` convention):
  - Surface unit tests inline in `src/surfaces/<lang>.rs`
    (`#[cfg(test)] mod tests { ... }`).
  - Registry tests inline in `src/surfaces/registry.rs` (fleet count assertion,
    name list in `test_all_fleet_surfaces_present()`, alias & case-insensitive
    lookup test cases).
  - Fleet lint-fix test inline in `src/surfaces/mod.rs`
    (`test_surface_supports_lint_fix()`).
  - Facet Rosetta golden table inline in `src/config/facets.rs`
    (`test_surface_facet_declarations()` and surface count assertions).
- [ ] **7. JSON Schema & Documentation**:
  - `cargo run -q -- schema -o schema/formality.schema.json`
  - `docs/language-surfaces.md`
  - `docs/facet-rosetta.md`
  - `README.md`

---

## 1. Create `src/surfaces/<lang>.rs`

Implement two traits on a unit struct:
(`#[derive(Default, Clone)] pub struct FooSurface;` is the pattern every
existing surface follows). Expose the module in `src/surfaces/mod.rs` with
`pub mod <lang>;`.

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

There is no wildcard fallback arm — the `match` must be exhaustive over
`Facet::ALL`, by design, so a new surface cannot accidentally skip declaring a
position on any facet. Every arm needs an honest answer: does the real tool
support configuring this, does it enforce one fixed value (document _why_ in a
comment, the way `go.rs`/`java.rs`/`kotlin.rs` do), or is the concept simply
absent for this language. Update [docs/facet-rosetta.md](facet-rosetta.md) with
the same row once this is decided.

### `LanguageSurface`

```rust
impl LanguageSurface for FooSurface {
  fn name(&self) -> &'static str { "foo" }
  fn display_name(&self) -> &'static str { "Foo" } // optional, defaults to name()
  fn aliases(&self) -> &[&'static str] { &["foolang"] } // alternate names
  fn file_extensions(&self) -> &[&'static str] { FOO_EXTENSIONS }
  fn detect(&self, root: &Path) -> bool { /* any file_extensions() present under root? */ }
  fn tool_info(&self, config: &ResolvedLangConfig) -> Vec<ToolInfo> { /* binaries + install hints */ }
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult { /* Smart Format pass */ }
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult { /* linter invocation */ }
  fn supports_lint_fix(&self) -> bool { true } // true only if linter has an automated fix mode
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult { /* native config generation */ }
  fn clone_box(&self) -> Box<dyn LanguageSurface> { Box::new(self.clone()) }
}
```

Key implementation notes drawn from the existing fleet of surfaces:

- **Smart Format ordering (Rule #7)**: `format()` must leave files in a state
  that will not immediately fail a trivial structural lint check. If the tool
  ecosystem separates "mechanical fix" (import sorting, blank-line
  normalization) from "layout formatting", run the mechanical pass first, inside
  `format()` — see Python's `ruff check --select I --fix` → `ruff format`, or
  Markdown's `markdownlint-cli2 --fix` → `prettier --write`. If one tool does
  both in a single invocation (Go's `goimports -w`, Kotlin's `ktlint -F`,
  JS/TS's `biome check --write --linter-enabled=false`), a single call is fine —
  do not invent a fake two-stage split.
- **`fml fix` vs. `fml fmt`**: `format()` must never apply _semantic_ lint fixes
  (unused-import removal, rule-based rewrites) — that is what
  `lint(ctx, fix: true)` and `supports_lint_fix()` are for. If the tool has no
  real auto-fix mode for lint violations (Checkstyle, yamllint, taplo lint), set
  `supports_lint_fix()` to `false` (the trait default) and return
  `SurfaceStatus::Skipped` (e.g.
  `"Tool does not support autofix; run fml fmt instead"`) when `fix == true`.
- **`check_binary_exists("<binary>")` and `tool_missing_result(...)`**: guard
  tool invocations with `check_binary_exists("<binary>")`. If missing, return
  `tool_missing_result(self.name(), start, "<binary>", install_hint)`.
- **`tool_info()`**: feeds `fml doctor` and `fml install` — list every binary
  the surface depends on (formatter and linter separately if they are different
  binaries), each with `is_required_for_fmt`/`is_required_for_lint` set
  accurately so `fml doctor --all` reports gaps precisely.
- **Diff-check temp files**: if `format()` needs to check "would this change
  anything" without mutating the real file (used by `fmt --check`), route
  through the shared `diff_check_via_tempcopy` helper in `src/surfaces/sync.rs`
  — it preserves the original file extension so extension-sensitive tools
  (Biome, google-java-format, ktlint) do not reject the scratch file.

---

## 2. Tooling & Installer Chains (`src/surfaces/tooling.rs`)

`fml install` and `ToolInfo::get_auto_install_cmd()` discover how to install
missing CLI tools via preference chains defined in `src/surfaces/tooling.rs`.

Add an ordered slice of [`InstallMethod`](../src/surfaces/tooling.rs) variants
(preferring prebuilt binary managers first, falling back to source compilation
or package managers) and register it in `install_chain_for`:

```rust
const FOOFMT_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("foofmt"),
  InstallMethod::Brew("foofmt"),
  InstallMethod::Scoop("foofmt"),
  InstallMethod::WingetName("Foo.foofmt"),
  InstallMethod::Cargo {
    package: "foofmt",
    locked: true,
  },
];

pub(super) fn install_chain_for(
  binary: &str,
) -> Option<&'static [InstallMethod]> {
  match binary {
    // ...existing tools...
    "foofmt" => Some(FOOFMT_CHAIN),
    _ => None,
  }
}
```

---

## 3. Per-Language Options & Config Wiring

### 3.1. Add typed options struct in `src/config/options.rs`

Define a `FooOptions` struct deriving
`Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema`.
Implement `merge(&mut self, other: FooOptions)` and `is_empty(&self) -> bool`.

```rust
/// Typed formatting and linting options for Foo.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct FooOptions {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub max_width: Option<usize>,
}

impl FooOptions {
  pub fn merge(&mut self, other: FooOptions) {
    if other.max_width.is_some() {
      self.max_width = other.max_width;
    }
  }

  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.max_width.is_none()
  }
}
```

> **Note:** If the tool has no knobs beyond shared facets (like TOML, JSON,
> Typst, or Kotlin), still add an empty struct (`merge` does nothing, `is_empty`
> returns `true` or pass `|_: &FooOptions| false` in the table). This keeps all
> surfaces uniform in `LangConfig`/`ResolvedLangConfig` and provides a place for
> future knobs without breaking changes.

### 3.2. Register in `lang_options_table!` (`src/config/lang_table.rs`)

Add a row to the `lang_options_table!` X-macro in `src/config/lang_table.rs`:

```rust
foo { crate::config::options::FooOptions, foo_options, crate::config::options::FooOptions::is_empty, "foofmt", "foolint" }
```

Each row defines 5 elements:

- `$lang`: the field identifier on `LangConfig`/`ResolvedLangConfig` and
  `[lang.<name>]` TOML key (`foo`).
- `$ty`: fully qualified path to the typed options struct
  (`crate::config::options::FooOptions`).
- `$accessor`: method name for the generated `LangConfig` accessor
  (`foo_options`).
- `$is_empty`: emptiness check used during deserialization (e.g.
  `crate::config::options::FooOptions::is_empty` or `|_: &FooOptions| false`).
- `$fmt` / `$lint`: default formatting and linting tool names as string
  literals, or the `NONE` sentinel if no default tool exists.

The `lang_options_table!` macro automatically generates:

- `LangConfig::merge()` field merging logic
- `LangConfig::foo_options()` accessor methods
- `resolve_for_lang()` struct resolution with fallback defaults
- `default_tools_for_lang()` default tool lookup

### 3.3. Add struct fields to `LangConfig` and `ResolvedLangConfig` (`src/config/mod.rs`)

1. Export `FooOptions` in `src/config/mod.rs` (via `pub use options::*;`).
2. Add the field to `LangConfig`:

   ```rust
   /// Foo surface specific options.
   #[serde(skip_serializing_if = "Option::is_none")]
   pub foo: Option<FooOptions>,
   ```

3. Add the field to `ResolvedLangConfig`:

   ```rust
   /// Resolved Foo surface options.
   pub foo: Option<FooOptions>,
   ```

---

## 4. Register the Surface in `SurfaceRegistry`

Registering the surface requires adding it to the default registry in
`src/surfaces/registry.rs`:

1. Import the new surface module in `src/surfaces/registry.rs`:

   ```rust
   use super::{
     LanguageSurface, cpp, foo, go, java, javascript, json, kotlin, markdown,
     python, rust, toml, typst, yaml,
   };
   ```

2. Register the surface type in `SurfaceRegistry::default()`:

   ```rust
   impl Default for SurfaceRegistry {
     fn default() -> Self {
       let mut reg = Self::empty();
       reg.register_surface::<rust::RustSurface>();
       reg.register_surface::<python::PythonSurface>();
       // ...existing surfaces...
       reg.register_surface::<foo::FooSurface>();
       reg
     }
   }
   ```

`SurfaceRegistry::register_surface::<S>()` instantiates `Box::new(S::default())`
and appends it to the registry.

---

## 5. Native Config Generation (`fml sync`)

If the tool reads a persisted configuration file (`.foorc`, `foo.toml`,
`biome.json`, `.golangci.yml`, `checkstyle.xml`, …), implement `sync_config()`:

1. Render the canonical globals + resolved `FooOptions` into the target file
   format.
2. Prefix generated content with `AUTO_GENERATED_HEADER`
   (`src/surfaces/native.rs`) so `fml sync` detects drift and preserves
   user-managed files without overwriting (`[MANUAL]` diagnostic).
3. Use
   `sync_file_helper(&target_path, file_name, &rendered_content, check, start, "foo")`
   from `src/surfaces/sync.rs` to handle file creation, update, and drift check
   cleanly.

If the tool has no native config file (driven entirely by CLI flags or
`.editorconfig`), `sync_config()` can return `SurfaceStatus::Passed` or
`SurfaceStatus::Skipped`.

---

## 6. Soft / Optional Integrations

- **LSP Child Server (`src/commands/lsp.rs`)**: If the ecosystem provides a
  language server, add an entry to `CHILD_LSP_REGISTRY`:

  ```rust
  ChildLsp {
    surface: "foo",
    binary: "foo-lsp",
    args: &["--stdio"],
    install_hint: "npm install -g foo-lsp  OR  brew install foo-lsp",
  },
  ```

- **EditorConfig Generation (`src/surfaces/editorconfig.rs`)**:
  - Add section glob to `glob_for_surface()`:

    ```rust
    "foo" => "[*.foo]".to_string(),
    ```

  - Add `"foo"` to `CANONICAL_FLEET_ORDER`.

- **Prose surface counts**: Update doc comments and prose mentioning the fleet
  count (e.g. `SurfaceRegistry::new()` doc comment "default fleet of 12 language
  surfaces", `cli.rs`, `README.md`).

---

## 7. Tests & Validation

Add tests across the test suites:

1. **Per-surface unit tests**: Inline in `src/surfaces/<lang>.rs`
   (`#[cfg(test)] mod tests { ... }` — see
   [Style Guide §1](style-guide.md#1-modulefile-hierarchy)), test
   `facet_support()` across all `Facet::ALL`, `detect()` with positive/negative
   temp fixtures, `tool_info()`, and `supports_lint_fix()`.
2. **Registry tests (inline in `src/surfaces/registry.rs`)**:
   - In `test_all_fleet_surfaces_present()`: update
     `assert_eq!(surfaces.len(), N)` and add `"foo"` to the `expected` list.
   - In `test_get_surface_by_name_canonical_and_aliases()`: add canonical and
     alias test cases.
   - In `test_get_surface_by_name_case_insensitive()`: add case-insensitive
     variations.
3. **Lint-fix assertion (inline in `src/surfaces/mod.rs`)**:
   - Add `assert!(foo::FooSurface.supports_lint_fix())` (or `!`) to
     `test_surface_supports_lint_fix()`.
4. **Facet Rosetta golden table (inline in `src/config/facets.rs`)**:
   - Add the surface's expected facet row to
     `test_surface_facet_declarations()`.
   - Update `assert_eq!(surfaces.len(), N)` and `assert_eq!(golden.len(), N)`.

Run presubmit verification:

```bash
cargo test --lib -q
cargo clippy -q
cargo run -q -- fmt --check
```

---

## 8. Regenerate Schema

Whenever `LangConfig`, `ResolvedLangConfig`, or per-language options structs are
modified, regenerate the schema:

```bash
cargo run -q -- schema -o schema/formality.schema.json
```

Commit the updated `schema/formality.schema.json`. CI verifies that the
repository schema matches the binary generation.

---

## 9. Documentation

- Add a dedicated section to [docs/language-surfaces.md](language-surfaces.md)
  documenting the surface, its tools, Smart Format behavior, options, and config
  generation.
- Add the surface's row to the table in
  [docs/facet-rosetta.md](facet-rosetta.md).
- Update the supported language table in `README.md`.

---

## Reference Examples

- [`src/surfaces/kotlin.rs`](../src/surfaces/kotlin.rs): Minimal single-tool
  surface (`ktlint` for formatting and linting, editorconfig-based
  configuration).
- [`src/surfaces/javascript.rs`](../src/surfaces/javascript.rs): Multi-extension
  surface with typed options (`JavaScriptOptions`) and native JSON config
  generation (`biome.json`).
- [`src/surfaces/go.rs`](../src/surfaces/go.rs): Multi-tool surface
  (`goimports` + `golangci-lint`) with options and YAML config generation
  (`.golangci.yml`).
- [`src/surfaces/python.rs`](../src/surfaces/python.rs): Multi-stage formatting
  surface (`ruff check --fix` + `ruff format`) with options and TOML config
  generation (`ruff.toml`).
