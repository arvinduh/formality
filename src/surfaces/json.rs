use super::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus, ToolInfo,
  check_binary_exists, create_tool_command, find_files_with_ext,
  markdown::sync_prettier_config,
};
use std::path::Path;
use std::time::Instant;

pub struct JsonSurface;

const JSON_EXTENSIONS: &[&str] = &["json", "jsonc"];

impl LanguageSurface for JsonSurface {
  fn name(&self) -> &'static str {
    "json"
  }

  fn aliases(&self) -> &[&'static str] {
    &[]
  }

  fn extensions(&self) -> &[&'static str] {
    JSON_EXTENSIONS
  }

  fn detect(&self, root: &Path) -> bool {
    !find_files_with_ext(root, JSON_EXTENSIONS, &[]).is_empty()
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
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "prettier".to_string(),
          install_hint: "npm install -g prettier".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(&ctx.root, JSON_EXTENSIONS, &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let mut cmd = create_tool_command("prettier");
    if ctx.check_only {
      cmd.arg("--check");
    } else {
      cmd.arg("--write");
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
          message: format!("Failed to execute prettier: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, _fix: bool) -> SurfaceResult {
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
