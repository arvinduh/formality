//! TOML language surface: formats and lints via `taplo`, syncing the managed
//! `taplo.toml` from `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  render_native_config, sync_native_config, tool_missing_result,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

// Directly mirrors Taplo's upstream native schema formatting flags.
#[allow(clippy::struct_excessive_bools)]
/// Formatting section for `taplo.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaploFormattingConfig {
  /// Whether to align entries across lines.
  pub align_entries: bool,
  /// Target column width for wrapping.
  pub column_width: usize,
  /// Whether to indent table entry keys.
  pub indent_entries: bool,
  /// String sequence used for indentation (spaces or tabs).
  pub indent_string: String,
  /// Whether to indent table contents.
  pub indent_tables: bool,
  /// Whether to use CRLF line endings.
  pub crlf: bool,
}

/// Native `taplo.toml` configuration representation for TOML formatting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaploConfig {
  /// Formatting configuration subsection.
  pub formatting: TaploFormattingConfig,
}

impl NativeConfig for TaploConfig {
  const FILE_NAME: &'static str = "taplo.toml";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let indent_spaces = if ctx.lang_config.use_tabs {
      "\t".to_string()
    } else {
      " ".repeat(ctx.lang_config.indent_size)
    };

    let crlf = ctx.global_config.end_of_line.eq_ignore_ascii_case("crlf");

    Self {
      formatting: TaploFormattingConfig {
        align_entries: false,
        column_width: ctx.lang_config.line_length,
        indent_entries: false,
        indent_string: indent_spaces,
        indent_tables: false,
        crlf,
      },
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Renders a [`TaploConfig`] as the `-o key=value` flags taplo's `format`
/// subcommand accepts inline, so `fml fmt` can apply formality.toml's
/// settings without writing `taplo.toml` to disk (Fixes #151). Only `fml
/// sync` writes that file now (see [`TomlSurface::sync_config`]). taplo's
/// `lint` subcommand has no equivalent inline-override flag (only `-c/--config
/// <path>`), but lint doesn't consume these formatting-layout options anyway.
#[must_use]
pub fn build_taplo_inline_config_args(cfg: &TaploConfig) -> Vec<String> {
  vec![
    "-o".to_string(),
    format!("column_width={}", cfg.formatting.column_width),
    "-o".to_string(),
    format!("indent_string={}", cfg.formatting.indent_string),
    "-o".to_string(),
    format!("crlf={}", cfg.formatting.crlf),
    "-o".to_string(),
    format!("align_entries={}", cfg.formatting.align_entries),
    "-o".to_string(),
    format!("indent_entries={}", cfg.formatting.indent_entries),
    "-o".to_string(),
    format!("indent_tables={}", cfg.formatting.indent_tables),
  ]
}

/// Builds argument vector for a `taplo lint` invocation whose output is
/// safe to parse for the LSP server (`fml lsp`, Fixes #159, #165). taplo has
/// no JSON/structured reporter reachable by CLI flag — only a
/// codespan-reporting-style human diagnostic block (`error: <message>` then
/// a `┌─ path:line:col` location line, verified against a real taplo v0.10.0
/// run) — so `--colors never` is the only flag needed to make that text
/// output parseable (colored output otherwise interleaves ANSI escapes into
/// the location line).
#[must_use]
pub fn build_taplo_lsp_lint_args(
  files: &[std::path::PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec![
    "lint".to_string(),
    "--colors".to_string(),
    "never".to_string(),
  ];
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// TOML language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct TomlSurface;

impl DeclaresFacets for TomlSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs | Facet::IndentWidth | Facet::LineLength => {
        FacetSupport::Configurable
      }
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ImportSort
      | Facet::ProseWrap
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

const TOML_EXTENSIONS: &[&str] = &["toml"];

impl LanguageSurface for TomlSurface {
  fn name(&self) -> &'static str {
    "toml"
  }

  fn aliases(&self) -> &[&'static str] {
    &[]
  }

  fn file_extensions(&self) -> &[&'static str] {
    TOML_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
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
      return tool_missing_result(
        self.name(),
        start,
        "taplo",
        "cargo binstall taplo-cli / npm install -g @taplo/cli / brew install taplo / cargo install taplo-cli --locked",
      );
    }

    let files = find_files_with_ext(
      ctx.root.as_path(),
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

    // Inline `-o key=value` instead of writing `taplo.toml` to disk — see
    // `build_taplo_inline_config_args` (Fixes #151). `fml sync` remains the
    // only path that materializes the file.
    let inline_config =
      build_taplo_inline_config_args(&TaploConfig::from_context(ctx));

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("taplo");
          cmd.arg("format").args(&inline_config).arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("taplo");
    cmd.arg("format");
    cmd.args(&inline_config);

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

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
          message: format!("Failed to execute taplo: {e}"),
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

    if !check_binary_exists("taplo") {
      return tool_missing_result(
        self.name(),
        start,
        "taplo",
        "cargo binstall taplo-cli / npm install -g @taplo/cli / brew install taplo / cargo install taplo-cli --locked",
      );
    }

    let files = find_files_with_ext(
      ctx.root.as_path(),
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
    cmd.current_dir(ctx.root.as_path());

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
          let msg = if stderr.trim().is_empty() {
            stdout
          } else {
            stderr
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
          message: format!("Failed to execute taplo lint: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  // `fml fmt` no longer goes through this path (Fixes #151): it passes the
  // resolved config to taplo inline via repeated `-o key=value` flags (see
  // `build_taplo_inline_config_args`, used in `format()` above). This method
  // is now reached only by `fml sync`, for users who explicitly want
  // `taplo.toml` materialized on disk.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<TaploConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::ResolvedGlobalConfig;
  use std::sync::Arc;
  use tempfile::TempDir;

  #[test]
  fn test_toml_surface_facets() {
    let surface = TomlSurface;
    assert_eq!(
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );
  }

  #[test]
  fn test_toml_surface_tool_info() {
    let surface = TomlSurface;
    let cfg = crate::config::ResolvedLangConfig::new("toml");
    let tools = surface.tool_info(&cfg);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "taplo");
    assert!(tools[0].is_required_for_fmt);
    assert!(tools[0].is_required_for_lint);
  }

  #[test]
  fn test_taplo_config_typed_serialization() {
    let cfg = TaploConfig {
      formatting: TaploFormattingConfig {
        align_entries: false,
        column_width: 100,
        indent_entries: false,
        indent_string: "    ".to_string(),
        indent_tables: false,
        crlf: true,
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("[formatting]"));
    assert!(rendered.contains("column_width = 100"));
    assert!(rendered.contains("indent_string = \"    \""));
    assert!(rendered.contains("crlf = true"));
  }

  #[test]
  fn test_toml_sync_config() {
    let temp = TempDir::new().unwrap();
    let surface = TomlSurface;
    let mut lang_cfg = crate::config::ResolvedLangConfig::new("toml");
    lang_cfg.line_length = 80;
    lang_cfg.indent_size = 2;

    let ctx = ExecutionContext {
      root: Arc::new(temp.path().to_path_buf()),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: lang_cfg,
      check_only: false,
    };

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join("taplo.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("column_width = 80"));
    assert!(content.contains("indent_string = \"  \""));
  }

  #[test]
  fn test_toml_extra_args_propagation() {
    let mut cmd = create_tool_command("taplo");
    cmd.arg("format").arg("-");
    let extra_args = vec!["--colors".to_string(), "never".to_string()];
    cmd.args(&extra_args);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().to_string())
      .collect();
    assert_eq!(args, vec!["format", "-", "--colors", "never"]);
  }

  #[test]
  fn test_build_taplo_inline_config_args_shape() {
    let cfg = TaploConfig {
      formatting: TaploFormattingConfig {
        align_entries: false,
        column_width: 100,
        indent_entries: false,
        indent_string: "    ".to_string(),
        indent_tables: false,
        crlf: true,
      },
    };
    let args = build_taplo_inline_config_args(&cfg);
    assert!(args.contains(&"column_width=100".to_string()));
    assert!(args.contains(&"indent_string=    ".to_string()));
    assert!(args.contains(&"crlf=true".to_string()));
  }

  #[test]
  fn test_build_taplo_lsp_lint_args() {
    let files = vec![std::path::PathBuf::from("a.toml")];
    let args = build_taplo_lsp_lint_args(&files, &[]);
    assert_eq!(
      args,
      vec![
        "lint".to_string(),
        "--colors".to_string(),
        "never".to_string(),
        "a.toml".to_string(),
      ]
    );
  }

  #[test]
  fn test_toml_format_does_not_write_taplo_toml() {
    // Fixes #151: `fml fmt` must not write `taplo.toml` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !check_binary_exists("taplo") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.toml"), "a=1\n").unwrap();

    let surface = TomlSurface;
    let ctx = ExecutionContext {
      root: Arc::new(temp.path().to_path_buf()),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: crate::config::ResolvedLangConfig::new("toml"),
      check_only: false,
    };

    let _ = surface.format(&ctx);

    assert!(!temp.path().join("taplo.toml").exists());
    assert!(!temp.path().join(".taplo.toml").exists());
  }

  #[test]
  fn test_toml_format_check_large_file_no_deadlock() {
    // Fixes #22: formatting large TOML files (>128 KB) in check mode must not deadlock.
    if !check_binary_exists("taplo") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("large_unformatted.toml");

    let mut large_content = String::with_capacity(180_000);
    for i in 0..8000 {
      use std::fmt::Write;
      let _ = writeln!(large_content, "key_{i}=\"value_{i}\"");
    }
    assert!(large_content.len() > 128 * 1024);
    std::fs::write(&file_path, &large_content).unwrap();

    let surface = TomlSurface;
    let ctx = ExecutionContext {
      root: Arc::new(temp.path().to_path_buf()),
      paths: Arc::new(vec![file_path.clone()]),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: crate::config::ResolvedLangConfig::new("toml"),
      check_only: true,
    };

    let res = surface.format(&ctx);
    assert!(!res.is_error(), "format returned error: {:?}", res.status);
    assert!(
      res.is_violation(),
      "expected formatting violations for unformatted TOML, got {:?}",
      res.status
    );
    if let SurfaceStatus::ViolationsFound { diff, .. } = res.status {
      let diff_str = diff.expect("diff should be present");
      assert!(diff_str.contains("key_0"));
    }

    // Check mode on already-formatted >128 KB file must also complete cleanly and return Passed.
    let formatted_path = temp.path().join("large_formatted.toml");
    let mut formatted_content = String::with_capacity(180_000);
    for i in 0..8000 {
      use std::fmt::Write;
      let _ = writeln!(formatted_content, "key_{i} = \"value_{i}\"");
    }
    assert!(formatted_content.len() > 128 * 1024);
    std::fs::write(&formatted_path, &formatted_content).unwrap();

    let ctx_formatted = ExecutionContext {
      root: Arc::new(temp.path().to_path_buf()),
      paths: Arc::new(vec![formatted_path]),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: crate::config::ResolvedLangConfig::new("toml"),
      check_only: true,
    };

    let res_formatted = surface.format(&ctx_formatted);
    assert!(
      matches!(res_formatted.status, SurfaceStatus::Passed),
      "expected Passed for formatted TOML, got {:?}",
      res_formatted.status
    );
  }
}
