//! Python language surface: formats and lints via `ruff` (format, import
//! sort, and lint), syncing the managed `ruff.toml` from `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, create_tool_command,
  diff_check_via_tempcopy, find_files_with_ext, render_native_config,
  run_tool_command, sync_native_config, tool_missing_guard,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Format configuration subsection for `ruff.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct RuffFormatConfig {
  /// Indent style (`"space"` or `"tab"`).
  pub indent_style: String,
  /// Preferred quote style (`"single"` or `"double"`).
  pub quote_style: String,
  /// Line ending style (`"auto"`, `"lf"`, `"crlf"`).
  pub line_ending: String,
}

/// Lint configuration subsection for `ruff.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuffLintConfig {
  /// Selected rule codes to enable.
  pub select: Vec<String>,
  /// Ignored rule codes.
  pub ignore: Vec<String>,
}

/// Native `ruff.toml` configuration representation for Python formatting and linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct RuffConfig {
  /// Line length limit.
  pub line_length: usize,
  /// Indentation spaces width.
  pub indent_width: usize,
  /// Target Python version.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target_version: Option<String>,
  /// Format configuration subsection.
  pub format: RuffFormatConfig,
  /// Lint configuration subsection.
  pub lint: RuffLintConfig,
}

impl NativeConfig for RuffConfig {
  const FILE_NAME: &'static str = "ruff.toml";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let indent_style = if ctx.lang_config.use_tabs {
      "tab"
    } else {
      "space"
    };
    let line_ending =
      match ctx.global_config.end_of_line.to_lowercase().as_str() {
        "crlf" => "crlf",
        _ => "lf",
      };

    let quote_style = ctx
      .lang_config
      .python
      .as_ref()
      .and_then(|p| p.quote_style.clone())
      .unwrap_or_else(|| "double".to_string());

    let target_version = ctx
      .lang_config
      .python
      .as_ref()
      .and_then(|p| p.target_version.clone());

    let ignore = ctx
      .lang_config
      .python
      .as_ref()
      .and_then(|p| p.ignore_rules.clone())
      .unwrap_or_default();

    Self {
      line_length: ctx.lang_config.line_length,
      indent_width: ctx.lang_config.indent_size,
      target_version,
      format: RuffFormatConfig {
        indent_style: indent_style.to_string(),
        quote_style,
        line_ending: line_ending.to_string(),
      },
      lint: RuffLintConfig {
        select: vec![
          "E".to_string(),
          "F".to_string(),
          "I".to_string(),
          "UP".to_string(),
          "B".to_string(),
          "SIM".to_string(),
        ],
        ignore,
      },
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Python language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct PythonSurface;

impl DeclaresFacets for PythonSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs
      | Facet::IndentWidth
      | Facet::LineLength
      | Facet::QuoteStyle
      | Facet::ImportSort => FacetSupport::Configurable,
      Facet::TrailingComma
      | Facet::ProseWrap
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

/// Standard file extensions recognized for Python source files.
pub const PYTHON_EXTENSIONS: &[&str] = &["py", "pyi"];

/// Builds argument vector for ruff import sorting invocation (`ruff check --select I --fix`).
#[must_use]
pub fn build_ruff_import_sort_args(
  files: &[PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec![
    "check".to_string(),
    "--select".to_string(),
    "I".to_string(),
    "--fix".to_string(),
  ];
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds argument vector for ruff lint check invocation.
#[must_use]
pub fn build_ruff_check_args(
  files: &[PathBuf],
  fix: bool,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["check".to_string()];
  if fix {
    args.push("--fix".to_string());
  }
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds argument vector for a machine-readable `ruff check` invocation,
/// used by the LSP server (`fml lsp`, Fixes #159) to translate individual
/// violations into per-file `Diagnostic`s instead of one generic warning.
/// Mirrors [`build_ruff_check_args`] but requests `--output-format=json`
/// output instead of `--fix`.
#[must_use]
pub fn build_ruff_check_json_args(
  files: &[PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["check".to_string(), "--output-format=json".to_string()];
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Renders the resolved [`RuffConfig`] as the inline `--config "<key> =
/// <value>"` overrides `ruff format`/`ruff check` accept, so `fml
/// fmt`/`fml lint` can apply formality.toml's settings without writing
/// `ruff.toml` to disk (Fixes #151). Only `fml sync` writes that file now
/// (see [`PythonSurface::sync_config`]).
#[must_use]
pub fn build_ruff_inline_config_args(cfg: &RuffConfig) -> Vec<String> {
  let mut args = vec![
    "--config".to_string(),
    format!("line-length={}", cfg.line_length),
    "--config".to_string(),
    format!("indent-width={}", cfg.indent_width),
    "--config".to_string(),
    format!("format.indent-style='{}'", cfg.format.indent_style),
    "--config".to_string(),
    format!("format.quote-style='{}'", cfg.format.quote_style),
    "--config".to_string(),
    format!("format.line-ending='{}'", cfg.format.line_ending),
  ];
  if let Some(target_version) = &cfg.target_version {
    args.push("--config".to_string());
    args.push(format!("target-version='{target_version}'"));
  }
  args
}

/// Renders the resolved [`RuffConfig`]'s lint-relevant settings as inline
/// `--config` overrides for `ruff check` (Fixes #151, sibling of
/// [`build_ruff_inline_config_args`] above).
#[must_use]
pub fn build_ruff_inline_lint_config_args(cfg: &RuffConfig) -> Vec<String> {
  let select = cfg
    .lint
    .select
    .iter()
    .map(|s| format!("'{s}'"))
    .collect::<Vec<_>>()
    .join(",");
  let mut args = vec![
    "--config".to_string(),
    format!("line-length={}", cfg.line_length),
    "--config".to_string(),
    format!("lint.select=[{select}]"),
  ];
  if !cfg.lint.ignore.is_empty() {
    let ignore = cfg
      .lint
      .ignore
      .iter()
      .map(|s| format!("'{s}'"))
      .collect::<Vec<_>>()
      .join(",");
    args.push("--config".to_string());
    args.push(format!("lint.ignore=[{ignore}]"));
  }
  args
}

impl LanguageSurface for PythonSurface {
  fn name(&self) -> &'static str {
    "python"
  }

  fn aliases(&self) -> &[&'static str] {
    &["py"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    PYTHON_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("pyproject.toml").is_file()
      || root.join("requirements.txt").is_file()
      || root.join("setup.py").is_file()
      || root.join("Pipfile").is_file()
      || root.join("ruff.toml").is_file()
      || root.join(".ruff.toml").is_file()
      || !find_files_with_ext(root, PYTHON_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "ruff",
      description: "Fast Python linter and code formatter",
      install_hint: "Install via: uv tool install ruff (or pip install ruff / brew install ruff / cargo binstall ruff)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  // Orchestrates Ruff formatting across check, diff, and in-place write modes with target path resolution.
  #[allow(clippy::too_many_lines)]
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) =
      tool_missing_guard(self.name(), "ruff", start, Some("pip install ruff"))
    {
      return res;
    }

    let files = ctx.matched_files(PYTHON_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `--config key=value` instead of writing `ruff.toml` to disk —
    // see `build_ruff_inline_config_args` (Fixes #151). `fml sync` remains
    // the only path that materializes the file.
    let inline_config =
      build_ruff_inline_config_args(&RuffConfig::from_context(ctx));

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut isort_cmd = create_tool_command("ruff");
          isort_cmd
            .arg("check")
            .arg("--select")
            .arg("I")
            .arg("--fix")
            .args(&inline_config)
            .arg(scratch);
          isort_cmd.args(&ctx.lang_config.extra_args);
          isort_cmd.current_dir(ctx.root.as_path());
          let isort_out = isort_cmd.output()?;
          if !isort_out.status.success() {
            return Ok(isort_out);
          }

          let mut fmt_cmd = create_tool_command("ruff");
          fmt_cmd.arg("format").args(&inline_config).arg(scratch);
          fmt_cmd.args(&ctx.lang_config.extra_args);
          fmt_cmd.current_dir(ctx.root.as_path());
          fmt_cmd.output()
        },
        self.name(),
        start,
      );
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut isort_cmd = create_tool_command("ruff");
    isort_cmd.args(build_ruff_import_sort_args(
      &files_to_pass,
      &ctx.lang_config.extra_args,
    ));
    isort_cmd.args(&inline_config);
    isort_cmd.current_dir(ctx.root.as_path());

    match isort_cmd.output() {
      Ok(output) => {
        if !output.status.success() {
          let stderr = String::from_utf8_lossy(&output.stderr).to_string();
          let stdout = String::from_utf8_lossy(&output.stdout).to_string();
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else if !stdout.trim().is_empty() {
            stdout
          } else {
            "Import sorting issues found in Python files".to_string()
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
            message: format!("Failed to execute ruff import sorting: {e}"),
          },
          duration: start.elapsed(),
        };
      }
    }

    let mut cmd = create_tool_command("ruff");
    cmd.arg("format");
    cmd.args(&inline_config);

    if files_to_pass.is_empty() {
      cmd.arg(".");
    } else {
      for f in &files_to_pass {
        cmd.arg(f);
      }
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) =
      tool_missing_guard(self.name(), "ruff", start, Some("pip install ruff"))
    {
      return res;
    }

    let files = ctx.matched_files(PYTHON_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let files_to_pass = ctx.files_to_pass(files);

    let lint_config =
      build_ruff_inline_lint_config_args(&RuffConfig::from_context(ctx));

    let mut cmd = create_tool_command("ruff");
    cmd.args(build_ruff_check_args(
      &files_to_pass,
      fix,
      &ctx.lang_config.extra_args,
    ));
    cmd.args(&lint_config);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #151): they
  // pass the resolved config to ruff inline via repeated `--config key=val`
  // flags (see `build_ruff_inline_config_args` /
  // `build_ruff_inline_lint_config_args`, used in `format()`/`lint()`
  // above). This method is now reached only by `fml sync`, for users who
  // explicitly want `ruff.toml` materialized on disk.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<RuffConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::{
    PythonOptions, ResolvedGlobalConfig, ResolvedLangConfig,
  };
  use crate::surfaces::{check_binary_exists, test_ctx};
  use std::sync::Arc;
  use tempfile::TempDir;

  #[test]
  fn test_build_ruff_check_args_with_and_without_fix() {
    let no_fix = build_ruff_check_args(&[], false, &[]);
    assert_eq!(no_fix, vec!["check".to_string(), ".".to_string()]);

    let files = vec![PathBuf::from("a.py"), PathBuf::from("b.py")];
    let extra = vec!["--isolated".to_string()];
    let with_fix = build_ruff_check_args(&files, true, &extra);
    assert_eq!(
      with_fix,
      vec![
        "check".to_string(),
        "--fix".to_string(),
        "a.py".to_string(),
        "b.py".to_string(),
        "--isolated".to_string(),
      ]
    );
  }

  #[test]
  fn test_python_sync_config_lint_table_and_quote_style() {
    let temp = TempDir::new().unwrap();
    let surface = PythonSurface;
    let mut lang_cfg = ResolvedLangConfig::new("python");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.python = Some(PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: Some("py312".to_string()),
      ignore_rules: Some(vec!["E501".to_string(), "F401".to_string()]),
    });

    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join("ruff.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[lint]"));
    assert!(content.contains("select = ["));
    assert!(content.contains("\"E\""));
    assert!(content.contains("\"SIM\""));
    assert!(content.contains("ignore = ["));
    assert!(content.contains("\"E501\""));
    assert!(content.contains("\"F401\""));
    assert!(content.contains("quote-style = \"single\""));
    assert!(content.contains("target-version = \"py312\""));
    assert!(content.contains("line-length = 100"));
    assert!(content.contains("indent-width = 4"));
  }

  #[test]
  fn test_python_sync_config_default_omitted_ignore_rules() {
    let temp = TempDir::new().unwrap();
    let surface = PythonSurface;
    let mut lang_cfg = ResolvedLangConfig::new("python");
    lang_cfg.python = Some(PythonOptions {
      quote_style: Some("double".to_string()),
      target_version: None,
      ignore_rules: None,
    });

    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(
      res.status,
      SurfaceStatus::ConfigSynced { created: true, .. }
    ));

    let config_path = temp.path().join("ruff.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[lint]"));
    assert!(content.contains("ignore = []"));
  }

  #[test]
  fn test_ruff_config_typed_serialization() {
    let cfg = RuffConfig {
      line_length: 100,
      indent_width: 4,
      target_version: Some("py311".to_string()),
      format: RuffFormatConfig {
        indent_style: "space".to_string(),
        quote_style: "single".to_string(),
        line_ending: "lf".to_string(),
      },
      lint: RuffLintConfig {
        select: vec!["E".to_string(), "F".to_string()],
        ignore: vec!["E501".to_string()],
      },
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("line-length = 100"));
    assert!(rendered.contains("indent-width = 4"));
    assert!(rendered.contains("target-version = \"py311\""));
    assert!(rendered.contains("[format]"));
    assert!(rendered.contains("quote-style = \"single\""));
    assert!(rendered.contains("[lint]"));
    assert!(rendered.contains("ignore = [\"E501\"]"));
  }
  #[test]
  fn test_python_surface_file_extensions_and_pyi_detection() {
    let surface = PythonSurface;
    assert_eq!(surface.file_extensions(), &["py", "pyi"]);

    let temp = TempDir::new().unwrap();
    assert!(!surface.detect(temp.path()));

    // Create a .pyi stub file
    let pyi_file = temp.path().join("types.pyi");
    std::fs::write(&pyi_file, "def foo(x: int) -> str: ...").unwrap();
    assert!(surface.detect(temp.path()));
  }
  #[test]
  fn test_build_ruff_import_sort_args() {
    let no_files = build_ruff_import_sort_args(&[], &[]);
    assert_eq!(
      no_files,
      vec![
        "check".to_string(),
        "--select".to_string(),
        "I".to_string(),
        "--fix".to_string(),
        ".".to_string(),
      ]
    );

    let files = vec![PathBuf::from("a.py"), PathBuf::from("b.py")];
    let extra = vec!["--isolated".to_string()];
    let with_files = build_ruff_import_sort_args(&files, &extra);
    assert_eq!(
      with_files,
      vec![
        "check".to_string(),
        "--select".to_string(),
        "I".to_string(),
        "--fix".to_string(),
        "a.py".to_string(),
        "b.py".to_string(),
        "--isolated".to_string(),
      ]
    );
  }

  #[test]
  fn test_python_format_with_import_sorting() {
    if !check_binary_exists("ruff") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("test.py");
    let unformatted = "import sys\nimport os\n\ndef   foo( ):\n  pass\n";
    std::fs::write(&file, unformatted).unwrap();

    let surface = PythonSurface;
    let mut ctx_check =
      test_ctx(temp.path(), ResolvedLangConfig::new("python"));
    ctx_check.check_only = true;

    let check_res = surface.format(&ctx_check);
    assert!(matches!(
      check_res.status,
      SurfaceStatus::ViolationsFound { .. }
    ));

    let ctx_fix = test_ctx(temp.path(), ResolvedLangConfig::new("python"));

    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, SurfaceStatus::Passed));

    let formatted = std::fs::read_to_string(&file).unwrap();
    let os_idx = formatted.find("import os").unwrap();
    let sys_idx = formatted.find("import sys").unwrap();
    assert!(os_idx < sys_idx);

    let check_clean = surface.format(&ctx_check);
    assert!(matches!(check_clean.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_build_ruff_inline_config_args_shape() {
    let cfg = RuffConfig {
      line_length: 100,
      indent_width: 4,
      target_version: Some("py311".to_string()),
      format: RuffFormatConfig {
        indent_style: "space".to_string(),
        quote_style: "single".to_string(),
        line_ending: "lf".to_string(),
      },
      lint: RuffLintConfig {
        select: vec!["E".to_string(), "F".to_string()],
        ignore: vec![],
      },
    };
    let args = build_ruff_inline_config_args(&cfg);
    assert!(args.contains(&"line-length=100".to_string()));
    assert!(args.contains(&"indent-width=4".to_string()));
    assert!(args.contains(&"format.quote-style='single'".to_string()));
    assert!(args.contains(&"target-version='py311'".to_string()));

    let lint_args = build_ruff_inline_lint_config_args(&cfg);
    assert!(lint_args.contains(&"lint.select=['E','F']".to_string()));
    assert!(!lint_args.iter().any(|a| a.starts_with("lint.ignore=")));

    let cfg_with_ignore = RuffConfig {
      lint: RuffLintConfig {
        select: vec!["E".to_string()],
        ignore: vec!["E501".to_string(), "F401".to_string()],
      },
      ..cfg
    };
    let lint_args_with_ignore =
      build_ruff_inline_lint_config_args(&cfg_with_ignore);
    assert!(
      lint_args_with_ignore
        .contains(&"lint.ignore=['E501','F401']".to_string())
    );
  }

  #[test]
  fn test_ruff_config_from_context_ignore_rules() {
    let mut lang_cfg = ResolvedLangConfig::new("python");
    lang_cfg.python = Some(PythonOptions {
      quote_style: Some("double".to_string()),
      target_version: Some("py311".to_string()),
      ignore_rules: Some(vec!["E501".to_string(), "SIM101".to_string()]),
    });
    let ctx = test_ctx(Path::new("."), lang_cfg);
    let cfg = RuffConfig::from_context(&ctx);
    assert_eq!(cfg.lint.ignore, vec!["E501", "SIM101"]);

    let ctx_default =
      test_ctx(Path::new("."), ResolvedLangConfig::new("python"));
    let cfg_default = RuffConfig::from_context(&ctx_default);
    assert!(cfg_default.lint.ignore.is_empty());
  }

  #[test]
  fn test_python_format_and_lint_do_not_write_ruff_toml() {
    // Fixes #151: `fml fmt`/`fml lint` must not write `ruff.toml` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !check_binary_exists("ruff") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.py"), "x=1\n").unwrap();

    let surface = PythonSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("python"));

    let _ = surface.format(&ctx);
    let _ = surface.lint(&ctx, false);

    assert!(!temp.path().join("ruff.toml").exists());
    assert!(!temp.path().join(".ruff.toml").exists());
  }

  #[test]
  fn test_ruff_config_line_ending_cr_fallback() {
    let global = ResolvedGlobalConfig {
      end_of_line: "cr".to_string(),
      ..Default::default()
    };
    let mut ctx = test_ctx(Path::new("."), ResolvedLangConfig::new("python"));
    ctx.global_config = Arc::new(global);
    let cfg = RuffConfig::from_context(&ctx);
    assert_eq!(cfg.format.line_ending, "lf");
  }
}
