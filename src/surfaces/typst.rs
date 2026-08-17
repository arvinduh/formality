use super::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus, ToolInfo,
  check_binary_exists, create_tool_command, find_files_with_ext,
};
use std::path::Path;
use std::time::Instant;

pub struct TypstSurface;

const TYPST_EXTENSIONS: &[&str] = &["typ"];

impl LanguageSurface for TypstSurface {
  fn name(&self) -> &'static str {
    "typst"
  }

  fn aliases(&self) -> &[&'static str] {
    &["typ"]
  }

  fn detect(&self, root: &Path) -> bool {
    !find_files_with_ext(root, TYPST_EXTENSIONS, &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "typstyle",
      description: "Beautiful and reliable code formatter for Typst",
      install_hint: "Install via: cargo install typstyle --locked (or brew install typstyle)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("typstyle") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "typstyle".to_string(),
          install_hint:
            "cargo install typstyle --locked / brew install typstyle"
              .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(&ctx.root, TYPST_EXTENSIONS, &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let mut cmd = create_tool_command("typstyle");
    cmd
      .arg("--column")
      .arg(ctx.lang_config.line_length.to_string());
    if ctx.check_only {
      cmd.arg("--check");
    } else {
      cmd.arg("-i");
    }

    for f in &files {
      cmd.arg(f);
    }

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
          message: format!("Failed to execute typstyle: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, _fix: bool) -> SurfaceResult {
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
