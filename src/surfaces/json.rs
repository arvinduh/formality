use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  markdown::sync_prettier_config, tool_missing_result,
};
use std::path::Path;
use std::time::Instant;

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
    let start = Instant::now();
    sync_prettier_config(ctx, check, start, self.name())
  }
}
