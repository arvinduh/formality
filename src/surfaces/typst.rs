//! Typst language surface: formats via `typstyle`.
//!
//! Implements [`super::LanguageSurface`] for Typst. Structured LSP diagnostics
//! are owned by [`crate::commands::lsp_diagnostics`].

use std::path;
use std::time;

use crate::config;
use crate::config::facets;
use crate::config::facets::DeclaresFacets;
use crate::surfaces;
use crate::surfaces::LanguageSurface;
use crate::surfaces::tooling;

/// Typst language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct TypstSurface;

impl DeclaresFacets for TypstSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs => facets::FacetSupport::Fixed("spaces"),
      facets::Facet::IndentWidth | facets::Facet::LineLength => {
        facets::FacetSupport::Configurable
      }
      facets::Facet::QuoteStyle
      | facets::Facet::TrailingComma
      | facets::Facet::ImportSort
      | facets::Facet::ProseWrap
      | facets::Facet::Edition
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

const TYPST_EXTENSIONS: &[&str] = &["typ"];

/// Builds argument vector for a `typst compile` invocation whose stderr is
/// safe to parse for the LSP server (`fml lsp`, Fixes #159 [pre-recreation], #165 [pre-recreation]). Typst has
/// no dedicated `check`/`lint` subcommand distinct from `compile` — this
/// surface's own [`TypstSurface::lint`] already delegates to `typstyle`'s
/// format-check for that reason. `typst compile` itself does carry real
/// diagnostics (undefined variables, type errors, missing fonts) that
/// typstyle never sees, so this targets `compile` directly with
/// `--diagnostic-format short`, verified against a real typst 0.15.1 run to
/// print `path:line:col: severity: message` on stderr, one line per
/// diagnostic (`human`, the default, additionally prints a source excerpt
/// per diagnostic that `short` omits). `output` should be a throwaway
/// scratch path (the caller discards it) since `compile` always needs
/// somewhere to write, even when only the diagnostics are wanted.
#[must_use]
pub fn build_typst_check_args(
  file: &path::Path,
  output: &path::Path,
) -> Vec<String> {
  vec![
    "compile".to_string(),
    "--diagnostic-format".to_string(),
    "short".to_string(),
    "-f".to_string(),
    "pdf".to_string(),
    file.to_string_lossy().to_string(),
    output.to_string_lossy().to_string(),
  ]
}

impl LanguageSurface for TypstSurface {
  fn name(&self) -> &'static str {
    "typst"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["typstyle"]
  }

  fn aliases(&self) -> &[&'static str] {
    &["typ"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    TYPST_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![surfaces::ToolInfo {
      binary: "typstyle",
      description: "Beautiful and reliable code formatter for Typst",
      install_hint: None,
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) =
      surfaces::tool_missing_guard(self.name(), "typstyle", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(TYPST_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    if ctx.check_only {
      return surfaces::diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut cmd = surfaces::create_tool_command("typstyle");
          cmd
            .arg("--column")
            .arg(ctx.lang_config.line_length.to_string())
            .arg("-i")
            .arg(scratch);
          cmd.args(ctx.lang_config.tool_args("typstyle"));
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = surfaces::create_tool_command("typstyle");
    cmd
      .arg("--column")
      .arg(ctx.lang_config.line_length.to_string())
      .arg("-i");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(ctx.lang_config.tool_args("typstyle"));
    cmd.current_dir(ctx.root.as_path());

    surfaces::run_tool_command(self.name(), &mut cmd)
  }

  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if fix {
      return surfaces::lint_fix_unsupported(self.name(), start);
    }

    // Typstyle check serves as format validation & syntax check
    let mut check_ctx = ctx.clone();
    check_ctx.check_only = true;
    self.format(&check_ctx)
  }

  fn sync_config(
    &self,
    _ctx: &surfaces::ExecutionContext,
    _check: bool,
  ) -> surfaces::SurfaceResult {
    // typstyle is configured via CLI flags (--column) at invocation time;
    // there is no separate config file to generate or verify.
    tooling::no_native_config(
      self.name(),
      "No config file (settings applied via CLI flags)",
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::surfaces;

  #[test]
  fn test_build_typst_check_args() {
    let args = build_typst_check_args(
      path::Path::new("bad.typ"),
      path::Path::new("/tmp/scratch/out.pdf"),
    );
    assert_eq!(
      args,
      vec![
        "compile".to_string(),
        "--diagnostic-format".to_string(),
        "short".to_string(),
        "-f".to_string(),
        "pdf".to_string(),
        "bad.typ".to_string(),
        "/tmp/scratch/out.pdf".to_string(),
      ]
    );
  }

  #[test]
  fn test_typst_surface_identity() {
    let surface = TypstSurface;
    assert_eq!(surface.name(), "typst");
    assert_eq!(surface.aliases(), &["typ"]);
    assert_eq!(surface.file_extensions(), &["typ"]);
  }

  #[test]
  fn test_typst_surface_detect() {
    let surface = TypstSurface;
    let temp = tempfile::TempDir::new().unwrap();
    assert!(!surfaces::detect_in(&surface, temp.path()));

    std::fs::write(temp.path().join("main.typ"), "= Title").unwrap();
    assert!(surfaces::detect_in(&surface, temp.path()));
  }

  #[test]
  fn test_typst_tool_info() {
    let surface = TypstSurface;
    let tools = surface.tool_info(&config::ResolvedLangConfig::new("typst"));
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "typstyle");
    assert!(tools[0].is_required_for_fmt);
    assert!(tools[0].is_required_for_lint);
  }

  #[test]
  fn test_typst_format_empty_project_passes_or_tool_missing() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("typst"));

    let res = surface.format(&ctx);
    if surfaces::check_binary_exists("typstyle") {
      assert!(matches!(res.status, surfaces::SurfaceStatus::Passed));
    } else {
      assert!(matches!(
        res.status,
        surfaces::SurfaceStatus::ToolMissing { .. }
      ));
    }
  }

  #[test]
  fn test_typst_lint_fix_is_unsupported() {
    // typstyle has no separate autofix-capable linter; lint(fix=true) must
    // be a no-op Skipped, matching every other CLI-only formatter surface
    // (JSON, and typstyle's own "no autofix linter" contract).
    let temp = tempfile::TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("typst"));
    let res = surface.lint(&ctx, true);
    assert!(matches!(
      res.status,
      surfaces::SurfaceStatus::Skipped { .. }
    ));
  }

  #[test]
  fn test_typst_lint_delegates_to_format_check() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("typst"));
    let res = surface.lint(&ctx, false);
    if surfaces::check_binary_exists("typstyle") {
      assert!(matches!(res.status, surfaces::SurfaceStatus::Passed));
    } else {
      assert!(matches!(
        res.status,
        surfaces::SurfaceStatus::ToolMissing { .. }
      ));
    }
  }

  #[test]
  fn test_typst_sync_config_is_noop_skipped() {
    // Unlike every other surface, Typst has no native config file to sync
    // (typstyle takes its settings as CLI flags) — sync_config must report
    // Skipped rather than ConfigSynced, and must not write any file.
    let temp = tempfile::TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("typst"));
    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      surfaces::SurfaceStatus::Skipped { .. }
    ));

    let entries: Vec<_> = std::fs::read_dir(temp.path()).unwrap().collect();
    assert!(entries.is_empty(), "sync_config must not write any file");
  }

  #[test]
  fn test_typst_declares_facets_matches_rosetta_table() {
    // Cross-check against docs/facet-rosetta.md's Typst row directly at the
    // surface level, covering all three support levels present in that row:
    // Fixed (indent_tabs), Configurable (indent_width, line_length), and
    // Unsupported (everything else).
    let surface = TypstSurface;
    assert_eq!(
      surface.facet_support(facets::Facet::IndentTabs),
      facets::FacetSupport::Fixed("spaces")
    );
    assert_eq!(
      surface.facet_support(facets::Facet::IndentWidth),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::LineLength),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::QuoteStyle),
      facets::FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(facets::Facet::TrailingComma),
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
}
