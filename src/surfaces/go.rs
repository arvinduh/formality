use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  render_native_config, sync_native_config, tool_missing_result,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

/// Default set of linters enabled in the generated `.golangci.yml` — matches
/// golangci-lint's own well-known default set so `fml`-managed projects don't
/// silently diverge from what most Go developers already expect.
#[must_use]
pub fn default_go_linters() -> Vec<String> {
  vec![
    "errcheck".to_string(),
    "govet".to_string(),
    "ineffassign".to_string(),
    "staticcheck".to_string(),
    "unused".to_string(),
  ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GolangciLintersConfig {
  pub enable: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GolangciLintConfig {
  pub version: String,
  pub linters: GolangciLintersConfig,
}

impl NativeConfig for GolangciLintConfig {
  const FILE_NAME: &'static str = ".golangci.yml";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let enable = ctx
      .lang_config
      .go
      .as_ref()
      .and_then(|g| g.linters.clone())
      .unwrap_or_else(default_go_linters);

    Self {
      version: "2".to_string(),
      linters: GolangciLintersConfig { enable },
    }
  }

  fn render(&self) -> Result<String, String> {
    render_native_config(self)
  }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GoSurface;

impl DeclaresFacets for GoSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      // Go is always tab-indented; gofmt/goimports do not accept a
      // space-indentation mode. This is a non-negotiable language rule, not
      // a per-project style choice.
      Facet::IndentTabs => FacetSupport::Fixed("tab"),
      Facet::ImportSort => FacetSupport::Configurable,
      Facet::IndentWidth
      | Facet::LineLength
      | Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ProseWrap
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

pub const GO_EXTENSIONS: &[&str] = &["go"];

/// Builds the argument list for `golangci-lint run`. Mirrors the
/// `build_ruff_check_args` pattern: pass explicit files only when the caller
/// scoped the run (specific paths, a `files` allowlist, or an `exclude`
/// list); otherwise defer to golangci-lint's own project-wide package
/// resolution via `./...`.
#[must_use]
pub fn build_golangci_lint_args(
  files: &[PathBuf],
  fix: bool,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["run".to_string()];
  if fix {
    args.push("--fix".to_string());
  }
  if files.is_empty() {
    args.push("./...".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

impl LanguageSurface for GoSurface {
  fn name(&self) -> &'static str {
    "go"
  }

  fn aliases(&self) -> &[&'static str] {
    &["golang"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    GO_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn detect(&self, root: &std::path::Path) -> bool {
    root.join("go.mod").is_file()
      || root.join("go.sum").is_file()
      || root.join(".golangci.yml").is_file()
      || root.join(".golangci.yaml").is_file()
      || !find_files_with_ext(root, GO_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "gofmt",
        description: "Go code formatter (simplifies code with -s)",
        install_hint: "Ships with the Go toolchain: install Go from https://go.dev/dl/",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "goimports",
        description: "Go formatter that also groups and sorts imports",
        install_hint: "Install via: go install golang.org/x/tools/cmd/goimports@latest",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "golangci-lint",
        description: "Fast Go linters runner aggregating multiple static analyzers",
        install_hint: "Install via: brew install golangci-lint (or go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  // Orchestrates two-stage Go formatting (gofmt + goimports) with check and in-place modes.
  #[allow(clippy::too_many_lines)]
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("gofmt") {
      return tool_missing_result(
        self.name(),
        start,
        "gofmt",
        "Ships with the Go toolchain: install Go from https://go.dev/dl/",
      );
    }

    if !check_binary_exists("goimports") {
      return tool_missing_result(
        self.name(),
        start,
        "goimports",
        "go install golang.org/x/tools/cmd/goimports@latest",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      GO_EXTENSIONS,
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

    let local_prefix = ctx
      .lang_config
      .go
      .as_ref()
      .and_then(|g| g.local_prefixes.clone());

    // "Smart Format": gofmt -s handles layout/simplification and goimports
    // handles import grouping/sorting, chained in a single `fml fmt` pass so
    // files land ready to pass `fml lint` immediately afterward.
    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut gofmt_cmd = create_tool_command("gofmt");
          gofmt_cmd.arg("-s").arg("-w").arg(scratch);
          gofmt_cmd.current_dir(&ctx.root);
          let gofmt_out = gofmt_cmd.output()?;
          if !gofmt_out.status.success() {
            return Ok(gofmt_out);
          }

          let mut goimports_cmd = create_tool_command("goimports");
          goimports_cmd.arg("-w");
          if let Some(ref prefix) = local_prefix {
            goimports_cmd.arg("-local").arg(prefix);
          }
          goimports_cmd.arg(scratch);
          goimports_cmd.args(&ctx.lang_config.extra_args);
          goimports_cmd.current_dir(&ctx.root);
          goimports_cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut gofmt_cmd = create_tool_command("gofmt");
    gofmt_cmd.arg("-s").arg("-w");
    for f in &files {
      gofmt_cmd.arg(f);
    }
    gofmt_cmd.current_dir(&ctx.root);

    match gofmt_cmd.output() {
      Ok(output) => {
        if !output.status.success() {
          let stderr = String::from_utf8_lossy(&output.stderr).to_string();
          let stdout = String::from_utf8_lossy(&output.stdout).to_string();
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else if !stdout.trim().is_empty() {
            stdout
          } else {
            "Formatting issues found in Go files".to_string()
          };

          return SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ViolationsFound {
              message: msg,
              diff: None,
            },
            duration: start.elapsed(),
          };
        }
      }
      Err(e) => {
        return SurfaceResult {
          surface_name: self.name(),
          status: SurfaceStatus::ExecutionError {
            message: format!("Failed to execute gofmt: {e}"),
          },
          duration: start.elapsed(),
        };
      }
    }

    let mut goimports_cmd = create_tool_command("goimports");
    goimports_cmd.arg("-w");
    if let Some(ref prefix) = local_prefix {
      goimports_cmd.arg("-local").arg(prefix);
    }
    for f in &files {
      goimports_cmd.arg(f);
    }
    goimports_cmd.args(&ctx.lang_config.extra_args);
    goimports_cmd.current_dir(&ctx.root);

    match goimports_cmd.output() {
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
            "Import organization issues found in Go files".to_string()
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
          message: format!("Failed to execute goimports: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("golangci-lint") {
      return tool_missing_result(
        self.name(),
        start,
        "golangci-lint",
        "brew install golangci-lint / go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      GO_EXTENSIONS,
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

    let files_to_pass = if !ctx.paths.is_empty()
      || !ctx.lang_config.files.is_empty()
      || !ctx.lang_config.exclude.is_empty()
    {
      files
    } else {
      Vec::new()
    };

    let mut cmd = create_tool_command("golangci-lint");
    cmd.args(build_golangci_lint_args(
      &files_to_pass,
      fix,
      &ctx.lang_config.extra_args,
    ));
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
          let msg = if stdout.trim().is_empty() {
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
          message: format!("Failed to execute golangci-lint: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<GolangciLintConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::{GoOptions, ResolvedGlobalConfig, ResolvedLangConfig};
  use std::sync::Arc;
  use tempfile::TempDir;

  #[test]
  fn test_go_surface_basics() {
    let surface = GoSurface;
    assert_eq!(surface.name(), "go");
    assert_eq!(surface.aliases(), &["golang"]);
    assert_eq!(surface.file_extensions(), &["go"]);
    assert!(surface.supports_lint_fix());
  }

  #[test]
  fn test_go_facet_support() {
    let surface = GoSurface;
    assert_eq!(
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("tab")
    );
    assert_eq!(
      surface.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::LineLength),
      FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );
  }

  #[test]
  fn test_go_detect() {
    let surface = GoSurface;
    let temp = TempDir::new().unwrap();
    assert!(!surface.detect(temp.path()));

    std::fs::write(temp.path().join("go.mod"), "module example.com/foo\n")
      .unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_go_detect_via_source_file() {
    let surface = GoSurface;
    let temp = TempDir::new().unwrap();
    assert!(!surface.detect(temp.path()));

    std::fs::write(temp.path().join("main.go"), "package main\n").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_build_golangci_lint_args_default_scope() {
    let args = build_golangci_lint_args(&[], false, &[]);
    assert_eq!(args, vec!["run".to_string(), "./...".to_string()]);
  }

  #[test]
  fn test_build_golangci_lint_args_with_fix_and_files() {
    let files = vec![PathBuf::from("main.go"), PathBuf::from("util.go")];
    let extra = vec!["--timeout=5m".to_string()];
    let args = build_golangci_lint_args(&files, true, &extra);
    assert_eq!(
      args,
      vec![
        "run".to_string(),
        "--fix".to_string(),
        "main.go".to_string(),
        "util.go".to_string(),
        "--timeout=5m".to_string(),
      ]
    );
  }

  #[test]
  fn test_default_go_linters() {
    let linters = default_go_linters();
    assert!(linters.contains(&"errcheck".to_string()));
    assert!(linters.contains(&"govet".to_string()));
    assert!(linters.contains(&"staticcheck".to_string()));
    assert!(linters.contains(&"unused".to_string()));
  }

  #[test]
  fn test_golangci_lint_config_typed_serialization() {
    let cfg = GolangciLintConfig {
      version: "2".to_string(),
      linters: GolangciLintersConfig {
        enable: vec!["errcheck".to_string(), "govet".to_string()],
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(
      rendered.contains("version: '2'") || rendered.contains("version: \"2\"")
    );
    assert!(rendered.contains("errcheck"));
    assert!(rendered.contains("govet"));
  }

  #[test]
  fn test_go_sync_config_default_linters() {
    let temp = TempDir::new().unwrap();
    let surface = GoSurface;
    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("go"),
      check_only: false,
    };

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join(".golangci.yml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("errcheck"));

    let check_res = surface.sync_config(&ctx, true);
    assert!(matches!(check_res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_go_sync_config_custom_linters() {
    let temp = TempDir::new().unwrap();
    let surface = GoSurface;
    let mut lang_cfg = ResolvedLangConfig::new("go");
    lang_cfg.go = Some(GoOptions {
      local_prefixes: Some("example.com/myorg".to_string()),
      linters: Some(vec!["revive".to_string(), "gocritic".to_string()]),
    });

    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: lang_cfg,
      check_only: false,
    };

    let res = surface.sync_config(&ctx, false);
    assert!(res.is_success());

    let content =
      std::fs::read_to_string(temp.path().join(".golangci.yml")).unwrap();
    assert!(content.contains("revive"));
    assert!(content.contains("gocritic"));
    assert!(!content.contains("errcheck"));
  }

  #[test]
  fn test_go_format_and_lint_with_real_tools() {
    if !check_binary_exists("gofmt") || !check_binary_exists("goimports") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("main.go");
    let unformatted = "package main\n\nimport (\n\"fmt\"\n)\n\nfunc main(){\nfmt.Println(\"hi\")\n}\n";
    std::fs::write(&file, unformatted).unwrap();

    let surface = GoSurface;
    let ctx_check = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("go"),
      check_only: true,
    };

    let check_res = surface.format(&ctx_check);
    assert!(matches!(
      check_res.status,
      SurfaceStatus::ViolationsFound { .. }
    ));

    let ctx_fix = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("go"),
      check_only: false,
    };

    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, SurfaceStatus::Passed));

    let formatted = std::fs::read_to_string(&file).unwrap();
    assert!(formatted.contains("\tfmt.Println(\"hi\")"));

    let check_clean = surface.format(&ctx_check);
    assert!(matches!(check_clean.status, SurfaceStatus::Passed));
  }
}
