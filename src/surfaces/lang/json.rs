//! JSON language surface: formats and validates via Prettier.
//!
//! Implements `super::LanguageSurface` for JSON, reusing `super::prettier`.
//! Fleet registration is owned by `super::registry`.

use std::path;
use std::time;

use crate::config;
use crate::config::facets;
use crate::config::facets::DeclaresFacets;
use crate::surfaces;
use crate::surfaces::LanguageSurface;
use crate::surfaces::sync;
use crate::surfaces::sync::prettier;
use crate::surfaces::tooling;

/// JSON language surface implementation.
#[derive(Debug, Default)]
pub struct JsonSurface;

impl DeclaresFacets for JsonSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs | facets::Facet::IndentWidth => {
        facets::FacetSupport::Configurable
      }
      facets::Facet::QuoteStyle => facets::FacetSupport::Fixed("double"),
      facets::Facet::TrailingComma => facets::FacetSupport::Fixed("none"),
      facets::Facet::LineLength
      | facets::Facet::ImportSort
      | facets::Facet::ProseWrap
      | facets::Facet::Edition
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

const JSON_EXTENSIONS: &[&str] = &["json", "jsonc"];

impl LanguageSurface for JsonSurface {
  fn name(&self) -> &'static str {
    "json"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["prettier"]
  }

  fn aliases(&self) -> &[&'static str] {
    &[]
  }

  fn file_extensions(&self) -> &[&'static str] {
    JSON_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(Self)
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![surfaces::ToolInfo {
      binary: "prettier",
      description: "JSON formatter",
      install_hint: None,
      is_required_for_fmt: true,
      is_required_for_lint: false,
    }]
  }

  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "prettier", start, None)
    {
      return res;
    }

    let files: Vec<path::PathBuf> = ctx
      .matched_files(JSON_EXTENSIONS)
      .into_iter()
      .filter(|p| {
        let fname = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
        fname != "package-lock.json" && fname != "npm-shrinkwrap.json"
      })
      .collect();
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `--tab-width`/`--print-width`/etc. instead of writing
    // `.prettierrc.json` to disk — see `prettier::prettier_args` (Fixes
    // #151 [pre-recreation]). `fml sync` remains the only path that materializes the file.
    let inline_config = prettier::prettier_args(ctx);

    if ctx.check_only {
      return sync::diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let parser = if scratch.to_string_lossy().contains(".jsonc.") {
            "json5"
          } else {
            "json"
          };
          let mut cmd = ctx.command("prettier");
          cmd
            .arg("--write")
            .arg("--parser")
            .arg(parser)
            .args(&inline_config)
            .arg(scratch);
          cmd.args(ctx.lang_config.tool_args("prettier"));
          cmd.output()
        },
        self.name(),
        start,
        tooling::classify_all_nonzero_as_error,
      );
    }

    let mut cmd = ctx.command("prettier");
    cmd.arg("--write");
    cmd.args(&inline_config);

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(ctx.lang_config.tool_args("prettier"));

    // `prettier --write` exits `0` regardless of whether it reformats and
    // only exits non-zero (`2`) on a parse error / bad config / unreadable
    // file — never `1` (that is `--check`-only). Every non-zero exit here
    // is a tool failure (`ExecutionError`), and the `--check` path above
    // classifies identically (Fixes #107).
    tooling::run_tool_command_classified(
      self.name(),
      &mut cmd,
      tooling::classify_all_nonzero_as_error,
    )
  }

  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if fix {
      return tooling::lint_fix_unsupported(self.name(), start);
    }

    // Prettier format checking can serve as JSON syntax linting
    let mut check_ctx = ctx.clone();
    check_ctx.check_only = true;
    self.format(&check_ctx)
  }

  fn uses_prettier(&self) -> bool {
    true
  }

  // JSON formats via prettier and has no native config of its own: its only
  // managed file is `.prettierrc.json`, which is shared with the Markdown
  // and YAML surfaces. That file is written once by the shared pass
  // (`sync_shared_prettier_config`), outside this parallel fan-out, because
  // three surfaces racing to write one path made the report
  // nondeterministic — see #130 and `uses_prettier` above. There is
  // therefore nothing left for this surface to sync on its own.
  fn sync_config(
    &self,
    _ctx: &surfaces::ExecutionContext,
    _check: bool,
  ) -> surfaces::SurfaceResult {
    tooling::no_native_config(
      self.name(),
      &format!("No config of its own (shares {})", prettier::FILE_NAME),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::surfaces;

  #[test]
  fn json_surface_identity() {
    let surface = JsonSurface;
    assert_eq!(surface.name(), "json");
    assert!(surface.aliases().is_empty());
    assert_eq!(surface.file_extensions(), &["json", "jsonc"]);
  }

  #[test]
  fn json_surface_detect() {
    let surface = JsonSurface;
    let temp = tempfile::TempDir::new().unwrap();
    assert!(!surfaces::detect_in(&surface, temp.path()));

    std::fs::write(temp.path().join("config.json"), "{}").unwrap();
    assert!(surfaces::detect_in(&surface, temp.path()));
  }

  #[test]
  fn json_surface_detect_jsonc() {
    let surface = JsonSurface;
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("tsconfig.jsonc"), "{ /* c */ }").unwrap();
    assert!(surfaces::detect_in(&surface, temp.path()));
  }

  #[test]
  fn json_tool_info() {
    let surface = JsonSurface;
    let tools = surface.tool_info(&config::ResolvedLangConfig::new("json"));
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "prettier");
    assert!(tools[0].is_required_for_fmt);
    assert!(!tools[0].is_required_for_lint);
  }

  #[test]
  fn json_format_empty_project_passes_or_tool_missing() {
    // Matches the convention used by every other surface's test suite
    // (e.g. Kotlin, Python): assert the deterministic outcome for whichever
    // branch the test environment is actually in, rather than assuming
    // prettier is installed.
    let temp = tempfile::TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("json"));

    let res = surface.format(&ctx);
    if tooling::check_binary_exists("prettier") {
      assert!(matches!(res.status, surfaces::SurfaceStatus::Passed));
    } else {
      assert!(matches!(
        res.status,
        surfaces::SurfaceStatus::ToolMissing { .. }
      ));
    }
  }

  #[test]
  fn json_format_ignores_lockfiles() {
    // package-lock.json / npm-shrinkwrap.json must never be reformatted:
    // find_files_with_ext + the filename filter should exclude them even
    // when they are the only JSON files present. `format()` checks
    // `prettier`'s presence before it ever looks at the file list, so the
    // *reachable* assertion is "no lockfile ever gets passed to prettier",
    // not "Passed unconditionally" — branch on tool presence like every
    // other prettier-backed test in this file.
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("package-lock.json"), "{}").unwrap();
    std::fs::write(temp.path().join("npm-shrinkwrap.json"), "{}").unwrap();

    let surface = JsonSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("json"));
    let res = surface.format(&ctx);
    if tooling::check_binary_exists("prettier") {
      assert!(matches!(res.status, surfaces::SurfaceStatus::Passed));
    } else {
      assert!(matches!(
        res.status,
        surfaces::SurfaceStatus::ToolMissing { .. }
      ));
    }
  }

  #[test]
  fn json_lint_fix_is_unsupported() {
    // JSON has no autofix-capable linter of its own; lint(fix=true) must be
    // a no-op Skipped rather than silently doing nothing or erroring.
    let temp = tempfile::TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("json"));
    let res = surface.lint(&ctx, true);
    assert!(matches!(
      res.status,
      surfaces::SurfaceStatus::Skipped { .. }
    ));
  }

  #[test]
  fn json_lint_delegates_to_format_check() {
    // lint(fix=false) is documented as reusing prettier's check-mode as a
    // syntax/format lint; assert it produces the same class of outcome as
    // format() in check mode rather than diverging.
    let temp = tempfile::TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("json"));
    let res = surface.lint(&ctx, false);
    if tooling::check_binary_exists("prettier") {
      assert!(matches!(res.status, surfaces::SurfaceStatus::Passed));
    } else {
      assert!(matches!(
        res.status,
        surfaces::SurfaceStatus::ToolMissing { .. }
      ));
    }
  }

  #[test]
  fn json_sync_config_does_not_claim_the_shared_prettierrc() {
    // Fixes #130: `.prettierrc.json` is shared with the markdown and yaml
    // surfaces, and all three used to sync it from their own `sync_config`,
    // which the runner calls under `par_iter()`. The file now has exactly
    // one writer — `sync_shared_prettier_config`, outside the fan-out — so
    // this surface writes nothing and says so.
    let temp = tempfile::TempDir::new().unwrap();
    let surface = JsonSurface;
    assert!(surface.uses_prettier());

    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("json"));
    let res = surface.sync_config(&ctx, false);

    assert!(matches!(
      res.status,
      surfaces::SurfaceStatus::Skipped { .. }
    ));
    assert!(
      !temp.path().join(".prettierrc.json").exists(),
      "the shared config must not be written from inside the fan-out"
    );
  }

  #[test]
  fn json_declares_facets_matches_rosetta_table() {
    // Cross-check against docs/facet-rosetta.md's JSON row directly at the
    // surface level (in addition to the crate-wide golden table in
    // src/config/facets_tests.rs), covering all three support levels:
    // Configurable, Fixed, and Unsupported.
    let surface = JsonSurface;
    assert_eq!(
      surface.facet_support(facets::Facet::IndentTabs),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::IndentWidth),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::QuoteStyle),
      facets::FacetSupport::Fixed("double")
    );
    assert_eq!(
      surface.facet_support(facets::Facet::TrailingComma),
      facets::FacetSupport::Fixed("none")
    );
    assert_eq!(
      surface.facet_support(facets::Facet::LineLength),
      facets::FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(facets::Facet::ImportSort),
      facets::FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(facets::Facet::ProseWrap),
      facets::FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(facets::Facet::Edition),
      facets::FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(facets::Facet::Standard),
      facets::FacetSupport::Unsupported
    );
  }

  #[test]
  fn json_format_does_not_write_prettierrc() {
    // Fixes #151 [pre-recreation]: `fml fmt` must not write `.prettierrc.json` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("prettier") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.json"), "{\"a\":1}").unwrap();

    let surface = JsonSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("json"));
    let _ = surface.format(&ctx);

    assert!(!temp.path().join(".prettierrc.json").exists());
  }
}
