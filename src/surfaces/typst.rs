//! Typst language surface: formats via `typstyle`. Typst has no lint tool or
//! managed native config file in this crate today, so this surface only
//! implements formatting.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  run_tool_command, tool_missing_result,
};
use std::path::Path;
use std::time::Instant;

/// Typst language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct TypstSurface;

impl DeclaresFacets for TypstSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Fixed("spaces"),
      Facet::IndentWidth | Facet::LineLength => FacetSupport::Configurable,
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ImportSort
      | Facet::ProseWrap
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

const TYPST_EXTENSIONS: &[&str] = &["typ"];

/// Builds argument vector for a `typst compile` invocation whose stderr is
/// safe to parse for the LSP server (`fml lsp`, Fixes #159, #165). Typst has
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
pub fn build_typst_check_args(file: &Path, output: &Path) -> Vec<String> {
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

  fn aliases(&self) -> &[&'static str] {
    &["typ"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    TYPST_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn detect(&self, root: &Path) -> bool {
    !find_files_with_ext(root, TYPST_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "typstyle",
      description: "Beautiful and reliable code formatter for Typst",
      install_hint: "Install via: cargo binstall typstyle (or brew install typstyle / winget install typstyle / cargo install typstyle --locked)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("typstyle") {
      return tool_missing_result(
        self.name(),
        start,
        "typstyle",
        "cargo binstall typstyle / brew install typstyle / winget install typstyle / cargo install typstyle --locked",
      );
    }

    let files = ctx.matched_files(TYPST_EXTENSIONS);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("typstyle");
          cmd
            .arg("--column")
            .arg(ctx.lang_config.line_length.to_string())
            .arg("-i")
            .arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("typstyle");
    cmd
      .arg("--column")
      .arg(ctx.lang_config.line_length.to_string())
      .arg("-i");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if fix {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Skipped {
          reason: "Tool does not support autofix; run fml fmt instead"
            .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    // Typstyle check serves as format validation & syntax check
    let mut check_ctx = ctx.clone();
    check_ctx.check_only = true;
    self.format(&check_ctx)
  }

  fn sync_config(
    &self,
    _ctx: &ExecutionContext,
    _check: bool,
  ) -> SurfaceResult {
    // typstyle is configured via CLI flags (--column) at invocation time;
    // there is no separate config file to generate or verify.
    SurfaceResult {
      surface_name: self.name(),
      status: SurfaceStatus::Skipped {
        reason: "No config file (settings applied via CLI flags)".to_string(),
      },
      duration: std::time::Duration::from_millis(0),
    }
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
  use std::sync::Arc;
  use tempfile::TempDir;

  #[test]
  fn test_build_typst_check_args() {
    let args = build_typst_check_args(
      Path::new("bad.typ"),
      Path::new("/tmp/scratch/out.pdf"),
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

  fn ctx_for(
    temp: &TempDir,
    lang_config: ResolvedLangConfig,
  ) -> ExecutionContext {
    ExecutionContext {
      root: Arc::new(temp.path().to_path_buf()),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config,
      check_only: false,
      candidate_files: None,
    }
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
    let temp = TempDir::new().unwrap();
    assert!(!surface.detect(temp.path()));

    std::fs::write(temp.path().join("main.typ"), "= Title").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_typst_tool_info() {
    let surface = TypstSurface;
    let tools = surface.tool_info(&ResolvedLangConfig::new("typst"));
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "typstyle");
    assert!(tools[0].is_required_for_fmt);
    assert!(tools[0].is_required_for_lint);
  }

  #[test]
  fn test_typst_format_empty_project_passes_or_tool_missing() {
    let temp = TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("typst"));

    let res = surface.format(&ctx);
    if check_binary_exists("typstyle") {
      assert!(matches!(res.status, SurfaceStatus::Passed));
    } else {
      assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
    }
  }

  #[test]
  fn test_typst_lint_fix_is_unsupported() {
    // typstyle has no separate autofix-capable linter; lint(fix=true) must
    // be a no-op Skipped, matching every other CLI-only formatter surface
    // (JSON, and typstyle's own "no autofix linter" contract).
    let temp = TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("typst"));
    let res = surface.lint(&ctx, true);
    assert!(matches!(res.status, SurfaceStatus::Skipped { .. }));
  }

  #[test]
  fn test_typst_lint_delegates_to_format_check() {
    let temp = TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("typst"));
    let res = surface.lint(&ctx, false);
    if check_binary_exists("typstyle") {
      assert!(matches!(res.status, SurfaceStatus::Passed));
    } else {
      assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
    }
  }

  #[test]
  fn test_typst_sync_config_is_noop_skipped() {
    // Unlike every other surface, Typst has no native config file to sync
    // (typstyle takes its settings as CLI flags) — sync_config must report
    // Skipped rather than ConfigSynced, and must not write any file.
    let temp = TempDir::new().unwrap();
    let surface = TypstSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("typst"));
    let res = surface.sync_config(&ctx, false);
    assert!(matches!(res.status, SurfaceStatus::Skipped { .. }));

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
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("spaces")
    );
    assert_eq!(
      surface.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::TrailingComma),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::ImportSort),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::ProseWrap),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::Edition),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::Standard),
      FacetSupport::Unsupported
    );
  }
}
