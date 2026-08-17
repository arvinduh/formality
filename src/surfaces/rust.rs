use super::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus, ToolInfo,
  check_binary_exists, create_tool_command, diff_check_via_tempcopy,
  find_files_with_ext, sync_file_helper,
};
use std::path::Path;
use std::time::Instant;

pub struct RustSurface;

impl LanguageSurface for RustSurface {
  fn name(&self) -> &'static str {
    "rust"
  }

  fn aliases(&self) -> &[&'static str] {
    &["rs"]
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
      || !find_files_with_ext(root, &["rs"], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "cargo",
        description: "Rust package manager & build tool",
        install_hint: "Install Rust via rustup: https://rustup.rs",
        is_required_for_fmt: true,
        is_required_for_lint: true,
      },
      ToolInfo {
        binary: "rustfmt",
        description: "Rust code formatter",
        install_hint: "Run: rustup component add rustfmt",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "clippy-driver",
        description: "Rust linter (cargo clippy)",
        install_hint: "Run: rustup component add clippy",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("cargo") && !check_binary_exists("rustfmt") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "cargo / rustfmt".to_string(),
          install_hint: "Run: rustup component add rustfmt".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(&ctx.root, &["rs"], &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let edition = if let Ok(manifest) =
      std::fs::read_to_string(ctx.root.join("Cargo.toml"))
    {
      if manifest.contains("edition = \"2024\"") {
        "2024"
      } else if manifest.contains("edition = \"2018\"") {
        "2018"
      } else {
        "2021"
      }
    } else {
      "2024"
    };

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut c = if check_binary_exists("rustfmt") {
            let mut cmd = create_tool_command("rustfmt");
            cmd.arg("--edition").arg(edition);
            cmd
          } else {
            let mut c = create_tool_command("cargo");
            c.arg("fmt").arg("--").arg("--edition").arg(edition);
            c
          };
          c.arg(scratch);
          c.current_dir(&ctx.root);
          c.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd =
      if check_binary_exists("cargo") && ctx.root.join("Cargo.toml").exists() {
        let mut c = create_tool_command("cargo");
        c.arg("fmt");
        if !ctx.paths.is_empty() {
          c.arg("--");
          for f in &files {
            c.arg(f);
          }
        }
        c
      } else {
        let mut c = create_tool_command("rustfmt");
        for f in &files {
          c.arg(f);
        }
        c
      };

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
            "Formatting issues found in Rust files".to_string()
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
          message: format!("Failed to execute cargo fmt / rustfmt: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("cargo") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "cargo".to_string(),
          install_hint: "Install Rust via https://rustup.rs".to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let mut cmd = create_tool_command("cargo");
    cmd.arg("clippy");
    if fix {
      cmd.arg("--fix").arg("--allow-dirty").arg("--allow-staged");
    }
    cmd.arg("--all-targets").arg("--").arg("-D").arg("warnings");
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
          message: format!("Failed to execute cargo clippy: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let target = ctx.root.join(".rustfmt.toml");

    let newline_style = match ctx.lang_config.indent_size {
      _ if ctx.global_config.end_of_line.eq_ignore_ascii_case("crlf") => {
        "Windows"
      }
      _ if ctx.global_config.end_of_line.eq_ignore_ascii_case("cr") => "Auto",
      _ => "Unix",
    };

    let content = format!(
      "# ==============================================================================\n\
       # WARNING: DO NOT EDIT THIS FILE DIRECTLY!\n\
       # This file is automatically generated and managed by formality (fml).\n\
       # Canonical configuration source of truth: formality.toml (or .formality.toml)\n\
       # To make changes, edit formality.toml and run: fml sync\n\
       # ==============================================================================\n\
       tab_spaces = {}\n\
       max_width = {}\n\
       newline_style = \"{}\"\n\
       use_small_heuristics = \"Default\"\n",
      ctx.lang_config.indent_size, ctx.lang_config.line_length, newline_style
    );

    sync_file_helper(
      &target,
      ".rustfmt.toml",
      &content,
      check,
      start,
      self.name(),
    )
  }
}
