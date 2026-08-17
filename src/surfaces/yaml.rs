use super::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus, ToolInfo,
  check_binary_exists, find_files_with_ext, markdown::sync_prettier_config,
};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub struct YamlSurface;

const YAML_EXTENSIONS: &[&str] = &["yaml", "yml"];

impl LanguageSurface for YamlSurface {
  fn name(&self) -> &'static str {
    "yaml"
  }

  fn aliases(&self) -> &[&'static str] {
    &["yml"]
  }

  fn detect(&self, root: &Path) -> bool {
    root.join(".yamllint").is_file()
      || root.join(".yamllint.yaml").is_file()
      || root.join(".yamllint.yml").is_file()
      || !find_files_with_ext(root, YAML_EXTENSIONS, &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "prettier",
        description: "YAML formatter",
        install_hint: "Install via: npm install -g prettier",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "yamllint",
        description: "YAML linter",
        install_hint: "Install via: pip install yamllint (or brew install yamllint)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
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

    let files = find_files_with_ext(&ctx.root, YAML_EXTENSIONS, &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let mut cmd = Command::new("prettier");
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
            "YAML formatting violations found".to_string()
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
    let start = Instant::now();

    if !check_binary_exists("yamllint") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "yamllint".to_string(),
          install_hint: "pip install yamllint".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(&ctx.root, YAML_EXTENSIONS, &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let mut cmd = Command::new("yamllint");
    if !ctx.paths.is_empty() {
      for f in &files {
        cmd.arg(f);
      }
    } else {
      cmd.arg(".");
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
          } else {
            stdout
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
          message: format!("Failed to execute yamllint: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_prettier_config(ctx, check, start, self.name())
  }
}
