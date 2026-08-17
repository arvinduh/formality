use super::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus, ToolInfo,
  check_binary_exists, create_tool_command, diff_check_via_tempcopy,
  find_files_with_ext, sync_file_helper,
};
use std::path::Path;
use std::time::Instant;

pub struct TomlSurface;

const TOML_EXTENSIONS: &[&str] = &["toml"];

impl LanguageSurface for TomlSurface {
  fn name(&self) -> &'static str {
    "toml"
  }

  fn aliases(&self) -> &[&'static str] {
    &[]
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("taplo.toml").is_file()
      || root.join(".taplo.toml").is_file()
      || !find_files_with_ext(root, TOML_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "taplo",
      description: "TOML toolkit, formatter and linter",
      install_hint: "Install via: cargo binstall taplo-cli (or npm install -g @taplo/cli / brew install taplo / cargo install taplo-cli --locked)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("taplo") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "taplo".to_string(),
          install_hint:
            "cargo binstall taplo-cli / npm install -g @taplo/cli / brew install taplo / cargo install taplo-cli --locked"
              .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(
      &ctx.root,
      TOML_EXTENSIONS,
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
          let content = std::fs::read(scratch)?;
          let mut cmd = create_tool_command("taplo");
          cmd.arg("format").arg("-");
          cmd.current_dir(&ctx.root);
          cmd.stdin(std::process::Stdio::piped());
          cmd.stdout(std::process::Stdio::piped());
          cmd.stderr(std::process::Stdio::piped());
          let mut child = cmd.spawn()?;
          if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(&content)?;
          }
          let output = child.wait_with_output()?;
          if output.status.success() {
            std::fs::write(scratch, &output.stdout)?;
          }
          Ok(output)
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("taplo");
    cmd.arg("format");

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
            "TOML formatting violations found".to_string()
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
          message: format!("Failed to execute taplo: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, _fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("taplo") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "taplo".to_string(),
          install_hint:
            "cargo binstall taplo-cli / npm install -g @taplo/cli / brew install taplo / cargo install taplo-cli --locked"
              .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(
      &ctx.root,
      TOML_EXTENSIONS,
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

    let mut cmd = create_tool_command("taplo");
    cmd.arg("lint");

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
          message: format!("Failed to execute taplo lint: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let target = ctx.root.join("taplo.toml");

    let indent_spaces = if ctx.lang_config.use_tabs {
      "\t".to_string()
    } else {
      " ".repeat(ctx.lang_config.indent_size)
    };

    let crlf = ctx.global_config.end_of_line.eq_ignore_ascii_case("crlf");

    let content = format!(
      "# ==============================================================================\n\
       # WARNING: DO NOT EDIT THIS FILE DIRECTLY!\n\
       # This file is automatically generated and managed by formality (fml).\n\
       # Canonical configuration source of truth: formality.toml (or .formality.toml)\n\
       # To make changes, edit formality.toml and run: fml sync\n\
       # ==============================================================================\n\
       [formatting]\n\
       align_entries = false\n\
       column_width = {}\n\
       indent_entries = false\n\
       indent_string = \"{}\"\n\
       indent_tables = false\n\
       crlf = {}\n",
      ctx.lang_config.line_length, indent_spaces, crlf
    );

    sync_file_helper(&target, "taplo.toml", &content, check, start, self.name())
  }
}
