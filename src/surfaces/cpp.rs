use super::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus, ToolInfo,
  check_binary_exists, create_tool_command, find_files_with_ext,
  sync_file_helper,
};
use std::path::Path;
use std::time::Instant;

pub struct CppSurface;

const CPP_EXTENSIONS: &[&str] = &["c", "cpp", "cc", "cxx", "h", "hpp", "hxx"];

impl LanguageSurface for CppSurface {
  fn name(&self) -> &'static str {
    "cpp"
  }

  fn aliases(&self) -> &[&'static str] {
    &["c", "c++", "cxx"]
  }

  fn extensions(&self) -> &[&'static str] {
    CPP_EXTENSIONS
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("CMakeLists.txt").is_file()
      || root.join("Makefile").is_file()
      || root.join("meson.build").is_file()
      || root.join(".clang-format").is_file()
      || !find_files_with_ext(root, CPP_EXTENSIONS, &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "clang-format",
        description: "C/C++ code formatter",
        install_hint: "Install via: sudo apt install clang-format (or brew install clang-format / pip install clang-format / winget install LLVM.LLVM)",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "clang-tidy",
        description: "C/C++ linter and static analyzer",
        install_hint: "Install via: sudo apt install clang-tidy (or brew install llvm / winget install LLVM.LLVM)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("clang-format") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "clang-format".to_string(),
          install_hint:
            "sudo apt install clang-format / brew install clang-format / pip install clang-format / winget install LLVM.LLVM"
              .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(&ctx.root, CPP_EXTENSIONS, &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let mut cmd = create_tool_command("clang-format");
    if ctx.check_only {
      cmd.arg("--dry-run").arg("--Werror");
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
            "Formatting violations found in C/C++ files".to_string()
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
          message: format!("Failed to execute clang-format: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, _fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("clang-tidy") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ToolMissing {
          binary: "clang-tidy".to_string(),
          install_hint: "sudo apt install clang-tidy / brew install llvm / winget install LLVM.LLVM"
            .to_string(),
        },
        duration: start.elapsed(),
      };
    }

    let files = find_files_with_ext(&ctx.root, CPP_EXTENSIONS, &ctx.paths);
    if files.is_empty() {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      };
    }

    let mut cmd = create_tool_command("clang-tidy");
    for f in &files {
      cmd.arg(f);
    }
    cmd.arg("--").arg("-std=c++17");
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
          message: format!("Failed to execute clang-tidy: {}", e),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let target = ctx.root.join(".clang-format");

    let use_tab = if ctx.lang_config.use_tabs {
      "Always"
    } else {
      "Never"
    };
    let line_ending =
      match ctx.global_config.end_of_line.to_lowercase().as_str() {
        "crlf" => "CRLF",
        "cr" => "CR",
        _ => "LF",
      };

    let content = format!(
      "# ==============================================================================\n\
       # WARNING: DO NOT EDIT THIS FILE DIRECTLY!\n\
       # This file is automatically generated and managed by formality (fml).\n\
       # Canonical configuration source of truth: formality.toml (or .formality.toml)\n\
       # To make changes, edit formality.toml and run: fml sync\n\
       # ==============================================================================\n\
       ---\n\
       Language: Cpp\n\
       BasedOnStyle: LLVM\n\
       IndentWidth: {}\n\
       ColumnLimit: {}\n\
       UseTab: {}\n\
       LineEnding: {}\n",
      ctx.lang_config.indent_size,
      ctx.lang_config.line_length,
      use_tab,
      line_ending
    );

    sync_file_helper(
      &target,
      ".clang-format",
      &content,
      check,
      start,
      self.name(),
    )
  }
}
