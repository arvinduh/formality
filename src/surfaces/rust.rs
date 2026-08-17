use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  sync_file_helper,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub struct RustSurface;

impl DeclaresFacets for RustSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Fixed("spaces"),
      Facet::IndentWidth => FacetSupport::Configurable,
      Facet::LineLength => FacetSupport::Configurable,
      Facet::QuoteStyle => FacetSupport::Unsupported,
      Facet::TrailingComma => FacetSupport::Unsupported,
      Facet::ImportSort => FacetSupport::Configurable,
      Facet::ProseWrap => FacetSupport::Unsupported,
      Facet::Edition => FacetSupport::Configurable,
      Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

pub(crate) fn build_rustfmt_fallback_cmd(
  edition: &str,
  check_only: bool,
  files: &[PathBuf],
) -> Command {
  let mut c = create_tool_command("rustfmt");
  c.arg("--edition").arg(edition);
  if check_only {
    c.arg("--check");
  }
  for f in files {
    c.arg(f);
  }
  c
}

impl LanguageSurface for RustSurface {
  fn name(&self) -> &'static str {
    "rust"
  }

  fn aliases(&self) -> &[&'static str] {
    &["rs"]
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
      || !find_files_with_ext(root, &["rs"], &[], &[], &[]).is_empty()
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

    let files = find_files_with_ext(
      &ctx.root,
      &["rs"],
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
          c.args(&ctx.lang_config.extra_args);
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
        if !ctx.paths.is_empty()
          || !ctx.lang_config.files.is_empty()
          || !ctx.lang_config.exclude.is_empty()
        {
          c.arg("--");
          for f in &files {
            c.arg(f);
          }
        }
        c
      } else {
        build_rustfmt_fallback_cmd(edition, ctx.check_only, &files)
      };

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
          message: format!("Failed to execute clippy: {}", e),
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

    let edition = ctx
      .lang_config
      .rust
      .as_ref()
      .and_then(|r| r.edition.as_deref())
      .unwrap_or("2024");

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
       use_small_heuristics = \"Default\"\n\
       edition = \"{}\"\n",
      ctx.lang_config.indent_size,
      ctx.lang_config.line_length,
      newline_style,
      edition
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
  use tempfile::TempDir;

  fn dummy_execution_context(
    root: &Path,
    check_only: bool,
  ) -> ExecutionContext {
    ExecutionContext {
      root: root.to_path_buf(),
      paths: vec![],
      global_config: ResolvedGlobalConfig::default(),
      lang_config: ResolvedLangConfig::new("rust"),
      check_only,
    }
  }

  #[test]
  fn test_sync_config_generates_edition_2024() {
    let temp = TempDir::new().unwrap();
    let surface = RustSurface;
    let ctx = dummy_execution_context(temp.path(), false);

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join(".rustfmt.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("edition = \"2024\""));
    assert!(content.contains("tab_spaces = 2"));
    assert!(content.contains("max_width = 80"));
    assert!(content.contains("newline_style = \"Unix\""));

    // Check mode should pass when file is up-to-date
    let check_ctx = dummy_execution_context(temp.path(), true);
    let check_res = surface.sync_config(&check_ctx, true);
    assert!(matches!(check_res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_rustfmt_fallback_command_args() {
    let files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];

    // check_only = false
    let cmd = build_rustfmt_fallback_cmd("2024", false, &files);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();

    let edition_idx = args.iter().position(|a| a == "--edition");
    assert!(
      edition_idx.is_some(),
      "--edition flag must be passed to rustfmt"
    );
    assert_eq!(
      args.get(edition_idx.unwrap() + 1).map(|s| s.as_str()),
      Some("2024"),
      "edition value must be 2024"
    );
    assert!(!args.contains(&"--check".to_string()));
    assert!(
      args.contains(&"src/main.rs".to_string())
        || args.contains(&"src\\main.rs".to_string())
    );

    // check_only = true
    let cmd_check = build_rustfmt_fallback_cmd("2021", true, &files);
    let check_args: Vec<String> = cmd_check
      .get_args()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();

    let check_edition_idx = check_args.iter().position(|a| a == "--edition");
    assert!(check_edition_idx.is_some());
    assert_eq!(
      check_args
        .get(check_edition_idx.unwrap() + 1)
        .map(|s| s.as_str()),
      Some("2021")
    );
    assert!(check_args.contains(&"--check".to_string()));
  }
}
