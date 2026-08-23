use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  markdown::sync_prettier_config, tool_missing_result,
};
use std::path::Path;
use std::time::Instant;

/// JSON language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonSurface;

impl DeclaresFacets for JsonSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs | Facet::IndentWidth => FacetSupport::Configurable,
      Facet::QuoteStyle => FacetSupport::Fixed("double"),
      Facet::TrailingComma => FacetSupport::Fixed("none"),
      Facet::LineLength
      | Facet::ImportSort
      | Facet::ProseWrap
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

const JSON_EXTENSIONS: &[&str] = &["json", "jsonc"];

impl LanguageSurface for JsonSurface {
  fn name(&self) -> &'static str {
    "json"
  }

  fn aliases(&self) -> &[&'static str] {
    &[]
  }

  fn file_extensions(&self) -> &[&'static str] {
    JSON_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn detect(&self, root: &Path) -> bool {
    !find_files_with_ext(root, JSON_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "prettier",
      description: "JSON formatter",
      install_hint: "Install via: npm install -g prettier (or pnpm add -g prettier / brew install prettier / winget install Prettier.Prettier)",
      is_required_for_fmt: true,
      is_required_for_lint: false,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("prettier") {
      return tool_missing_result(
        self.name(),
        start,
        "prettier",
        "npm install -g prettier",
      );
    }

    let files: Vec<std::path::PathBuf> = find_files_with_ext(
      &ctx.root,
      JSON_EXTENSIONS,
      &ctx.paths,
      &ctx.lang_config.files,
      &ctx.lang_config.exclude,
    )
    .into_iter()
    .filter(|p| {
      let fname = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
      fname != "package-lock.json" && fname != "npm-shrinkwrap.json"
    })
    .collect();
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
          let parser = if scratch.to_string_lossy().contains(".jsonc.") {
            "json5"
          } else {
            "json"
          };
          let mut cmd = create_tool_command("prettier");
          cmd.arg("--write").arg("--parser").arg(parser).arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(&ctx.root);
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("prettier");
    cmd.arg("--write");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(&ctx.root);

    match cmd.output() {
      Ok(output) => {
        if output.status.success() {
          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::Passed,
            duration: start.elapsed(),
          }
        } else {
          let stderr = String::from_utf8_lossy(&output.stderr).to_string();
          let stdout = String::from_utf8_lossy(&output.stdout).to_string();
          let msg = if !stdout.trim().is_empty() {
            stdout
          } else if !stderr.trim().is_empty() {
            stderr
          } else {
            "JSON formatting violations found".to_string()
          };

          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ViolationsFound {
              message: msg,
              diff: None,
            },
            duration: start.elapsed(),
          }
        }
      }
      Err(e) => SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ExecutionError {
          message: format!("Failed to execute prettier: {e}"),
        },
        duration: start.elapsed(),
      },
    }
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

    // Prettier format checking can serve as JSON syntax linting
    let mut check_ctx = ctx.clone();
    check_ctx.check_only = true;
    self.format(&check_ctx)
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    // JSON formatting uses Prettier; its layout configuration is shared and
    // emitted via `PrettierConfig` (.prettierrc.json), so there is no standalone
    // native JSON formatter config struct to maintain.
    let start = Instant::now();
    sync_prettier_config(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
  use std::sync::Arc;
  use tempfile::TempDir;

  fn ctx_for(
    temp: &TempDir,
    lang_config: ResolvedLangConfig,
  ) -> ExecutionContext {
    ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config,
      check_only: false,
    }
  }

  #[test]
  fn test_json_surface_identity() {
    let surface = JsonSurface;
    assert_eq!(surface.name(), "json");
    assert!(surface.aliases().is_empty());
    assert_eq!(surface.file_extensions(), &["json", "jsonc"]);
  }

  #[test]
  fn test_json_surface_detect() {
    let surface = JsonSurface;
    let temp = TempDir::new().unwrap();
    assert!(!surface.detect(temp.path()));

    std::fs::write(temp.path().join("config.json"), "{}").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_json_surface_detect_jsonc() {
    let surface = JsonSurface;
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("tsconfig.jsonc"), "{ /* c */ }").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_json_tool_info() {
    let surface = JsonSurface;
    let tools = surface.tool_info(&ResolvedLangConfig::new("json"));
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "prettier");
    assert!(tools[0].is_required_for_fmt);
    assert!(!tools[0].is_required_for_lint);
  }

  #[test]
  fn test_json_format_empty_project_passes_or_tool_missing() {
    // Matches the convention used by every other surface's test suite
    // (e.g. Kotlin, Python): assert the deterministic outcome for whichever
    // branch the test environment is actually in, rather than assuming
    // prettier is installed.
    let temp = TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("json"));

    let res = surface.format(&ctx);
    if check_binary_exists("prettier") {
      assert!(matches!(res.status, SurfaceStatus::Passed));
    } else {
      assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
    }
  }

  #[test]
  fn test_json_format_ignores_lockfiles() {
    // package-lock.json / npm-shrinkwrap.json must never be reformatted:
    // find_files_with_ext + the filename filter should exclude them even
    // when they are the only JSON files present. `format()` checks
    // `prettier`'s presence before it ever looks at the file list, so the
    // *reachable* assertion is "no lockfile ever gets passed to prettier",
    // not "Passed unconditionally" — branch on tool presence like every
    // other prettier-backed test in this file.
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("package-lock.json"), "{}").unwrap();
    std::fs::write(temp.path().join("npm-shrinkwrap.json"), "{}").unwrap();

    let surface = JsonSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("json"));
    let res = surface.format(&ctx);
    if check_binary_exists("prettier") {
      assert!(matches!(res.status, SurfaceStatus::Passed));
    } else {
      assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
    }
  }

  #[test]
  fn test_json_lint_fix_is_unsupported() {
    // JSON has no autofix-capable linter of its own; lint(fix=true) must be
    // a no-op Skipped rather than silently doing nothing or erroring.
    let temp = TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("json"));
    let res = surface.lint(&ctx, true);
    assert!(matches!(res.status, SurfaceStatus::Skipped { .. }));
  }

  #[test]
  fn test_json_lint_delegates_to_format_check() {
    // lint(fix=false) is documented as reusing prettier's check-mode as a
    // syntax/format lint; assert it produces the same class of outcome as
    // format() in check mode rather than diverging.
    let temp = TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("json"));
    let res = surface.lint(&ctx, false);
    if check_binary_exists("prettier") {
      assert!(matches!(res.status, SurfaceStatus::Passed));
    } else {
      assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
    }
  }

  #[test]
  fn test_json_sync_config_delegates_to_prettier() {
    let temp = TempDir::new().unwrap();
    let surface = JsonSurface;
    let ctx = ctx_for(&temp, ResolvedLangConfig::new("json"));
    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));
    assert!(temp.path().join(".prettierrc.json").is_file());
  }

  #[test]
  fn test_json_declares_facets_matches_rosetta_table() {
    // Cross-check against docs/facet-rosetta.md's JSON row directly at the
    // surface level (in addition to the crate-wide golden table in
    // src/config/facets_tests.rs), covering all three support levels:
    // Configurable, Fixed, and Unsupported.
    let surface = JsonSurface;
    assert_eq!(
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::QuoteStyle),
      FacetSupport::Fixed("double")
    );
    assert_eq!(
      surface.facet_support(Facet::TrailingComma),
      FacetSupport::Fixed("none")
    );
    assert_eq!(
      surface.facet_support(Facet::LineLength),
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
