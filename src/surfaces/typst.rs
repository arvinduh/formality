use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  tool_missing_result,
};
use std::path::Path;
use std::time::Instant;

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

    let files = find_files_with_ext(
      &ctx.root,
      TYPST_EXTENSIONS,
      &ctx.paths,
      &ctx.lang_config.files,
      &ctx.lang_config.exclude,
    );
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
          cmd.current_dir(&ctx.root);
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
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else if !stdout.trim().is_empty() {
            stdout
          } else {
            "Typst formatting violations found".to_string()
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
          message: format!("Failed to execute typstyle: {e}"),
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
