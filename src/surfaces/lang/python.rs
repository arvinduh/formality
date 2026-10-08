//! Python language surface: formats and lints via Ruff.
//!
//! Implements `super::LanguageSurface` for Python, syncing `ruff.toml`.
//! Fleet registration is owned by `super::registry`.

use std::path;
use std::time;

use crate::config;
use crate::config::facets;
use crate::surfaces;
use crate::surfaces::sync;
use crate::surfaces::sync::native;
use crate::surfaces::tooling;

/// The `ruff.toml` settings `ctx` resolves to.
fn ruff_config(ctx: &surfaces::ExecutionContext) -> native::ToolConfig {
  let python = ctx.lang_config.python.as_ref();
  let line_ending =
    if ctx.global_config.end_of_line.eq_ignore_ascii_case("crlf") {
      "crlf"
    } else {
      "lf"
    };
  let mut cfg = native::ToolConfig::new("ruff.toml")
    .set("line-length", native::int(ctx.lang_config.line_length))
    .set("indent-width", native::int(ctx.lang_config.indent_size));
  if let Some(target) = python.and_then(|p| p.target_version.as_deref()) {
    cfg = cfg.set("target-version", target);
  }
  cfg
    .set(
      "format.indent-style",
      if ctx.lang_config.use_tabs {
        "tab"
      } else {
        "space"
      },
    )
    .set(
      "format.quote-style",
      python
        .and_then(|p| p.quote_style.as_deref())
        .unwrap_or("double"),
    )
    .set("format.line-ending", line_ending)
    .set("lint.select", vec!["E", "F", "I", "UP", "B", "SIM"])
    .set(
      "lint.ignore",
      python
        .and_then(|p| p.ignore_rules.clone())
        .unwrap_or_default(),
    )
}

/// The settings `ruff format` takes inline through repeated `--config`.
const RUFF_FORMAT_INLINE_KEYS: &[&str] = &[
  "line-length",
  "indent-width",
  "format.indent-style",
  "format.quote-style",
  "format.line-ending",
  "target-version",
];

/// The inline `--config` overrides for `ruff check`. An empty ignore list is
/// left out, so a project's own `ruff.toml` ignores still apply.
fn ruff_lint_args(cfg: &native::ToolConfig) -> Vec<String> {
  let ignores = cfg
    .get("lint.ignore")
    .as_array()
    .is_some_and(|a| !a.is_empty());
  let keys: &[&str] = if ignores {
    &["line-length", "lint.select", "lint.ignore"]
  } else {
    &["line-length", "lint.select"]
  };
  cfg.flagged("--config", keys, native::Quote::Single)
}

/// Python language surface implementation.
#[derive(Debug, Default)]
pub struct PythonSurface;

impl facets::DeclaresFacets for PythonSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs
      | facets::Facet::IndentWidth
      | facets::Facet::LineLength
      | facets::Facet::QuoteStyle
      | facets::Facet::ImportSort => facets::FacetSupport::Configurable,
      facets::Facet::TrailingComma
      | facets::Facet::ProseWrap
      | facets::Facet::Edition
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

/// Standard file extensions recognized for Python source files.
const PYTHON_EXTENSIONS: &[&str] = &["py", "pyi"];

/// `[lang.python.extra_args]` key for both `ruff check` invocations: the
/// `--select I --fix` import-sort pass of `fml fmt`, and `fml lint`.
const RUFF_CHECK: &str = "ruff-check";

/// `[lang.python.extra_args]` key for the `ruff format` pass of `fml fmt`.
const RUFF_FORMAT: &str = "ruff-format";

/// Returns true if `extra_args` includes flags that widen or override rule
/// selection in Ruff (e.g. `--extend-select` or `--select`).
#[must_use]
fn extra_args_widen_selection(extra_args: &[String]) -> bool {
  extra_args.iter().any(|arg| {
    arg == "--extend-select"
      || arg.starts_with("--extend-select=")
      || arg == "--select"
      || arg.starts_with("--select=")
  })
}

/// Returns true if Ruff output indicates lint rule violations rather than
/// purely an operational syntax error (`invalid-syntax:` or `E999`).
#[must_use]
fn ruff_output_has_lint_findings(stdout: &str, stderr: &str) -> bool {
  if !stderr.trim().is_empty() && stdout.trim().is_empty() {
    return false;
  }
  let has_syntax_error =
    stdout.contains("invalid-syntax:") || stdout.contains("E999");
  let has_findings = stdout.lines().any(|line| {
    let trimmed = line.trim();
    !trimmed.starts_with("invalid-syntax:")
      && !trimmed.starts_with("error:")
      && !trimmed.is_empty()
      && (trimmed.starts_with("Found ")
        || trimmed.split_once(' ').is_some_and(|(code, _)| {
          code.chars().all(|c| c.is_ascii_alphanumeric())
        }))
  });
  !has_syntax_error && has_findings
}

/// Classifies non-zero exit from `ruff check --select I --fix`.
///
/// Exit 1 represents lint violations found if rule selection was widened
/// (e.g. via `--extend-select` in `extra_args`, Fixes #208) or if the tool
/// output contains lint findings. Otherwise (e.g. syntax errors or exit 2
/// operational errors), it represents an operational failure.
#[must_use]
fn is_ruff_import_sort_violation(
  code: Option<i32>,
  stdout: &str,
  stderr: &str,
  extra_args: &[String],
) -> bool {
  if code != Some(1) {
    return false;
  }
  if extra_args_widen_selection(extra_args) {
    return true;
  }
  ruff_output_has_lint_findings(stdout, stderr)
}

/// Builds argument vector for ruff import sorting invocation (`ruff check --select I --fix`),
/// ending with the `ruff-check` extra args.
#[must_use]
fn build_ruff_import_sort_args(
  files: &[path::PathBuf],
  lang: &config::ResolvedLangConfig,
) -> Vec<String> {
  let extra_args = lang.tool_args(RUFF_CHECK);
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

/// Builds argument vector for ruff lint check invocation, ending with the
/// `ruff-check` extra args.
#[must_use]
fn build_ruff_check_args(
  files: &[path::PathBuf],
  fix: bool,
  lang: &config::ResolvedLangConfig,
) -> Vec<String> {
  let extra_args = lang.tool_args(RUFF_CHECK);
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

/// Builds argument vector for the `ruff format` pass: `inline_config`, then
/// the files (`.` when none), then the `ruff-format` extra args.
#[must_use]
fn build_ruff_format_args(
  inline_config: &[String],
  files: &[path::PathBuf],
  lang: &config::ResolvedLangConfig,
) -> Vec<String> {
  let mut args = vec!["format".to_string()];
  args.extend(inline_config.iter().cloned());
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(lang.tool_args(RUFF_FORMAT).iter().cloned());
  args
}

/// Builds argument vector for a machine-readable `ruff check` invocation,
/// used by the LSP server (`fml lsp`, Fixes #159 [pre-recreation]) to translate individual
/// violations into per-file `Diagnostic`s instead of one generic warning.
/// Mirrors [`build_ruff_check_args`] but requests `--output-format=json`
/// output instead of `--fix`.
#[must_use]
pub fn build_ruff_check_json_args(
  files: &[path::PathBuf],
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

impl surfaces::LanguageSurface for PythonSurface {
  fn name(&self) -> &'static str {
    "python"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &[RUFF_CHECK, RUFF_FORMAT]
  }

  fn aliases(&self) -> &[&'static str] {
    &["py"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    PYTHON_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(Self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn marker_files(&self) -> &[&'static str] {
    &[
      "pyproject.toml",
      "requirements.txt",
      "setup.py",
      "Pipfile",
      "ruff.toml",
      ".ruff.toml",
    ]
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![surfaces::ToolInfo {
      binary: "ruff",
      description: "Fast Python linter and code formatter",
      install_hint: None,
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  // Orchestrates Ruff formatting across check, diff, and in-place write modes with target path resolution.
  #[expect(
    clippy::too_many_lines,
    reason = "orchestrates Ruff formatting across check, diff, and in-place write modes"
  )]
  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "ruff", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(PYTHON_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `--config key=value` instead of writing `ruff.toml` to disk —
    // see `RUFF_FORMAT_INLINE_KEYS` (Fixes #151 [pre-recreation]). `fml sync` remains
    // the only path that materializes the file.
    let inline_config = ruff_config(ctx).flagged(
      "--config",
      RUFF_FORMAT_INLINE_KEYS,
      native::Quote::Single,
    );

    let check_args = ctx.lang_config.tool_args(RUFF_CHECK);
    let widens_selection = extra_args_widen_selection(check_args);

    if ctx.check_only {
      return sync::diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let scratch = [scratch.to_path_buf()];
          let mut isort_cmd = tooling::create_tool_command("ruff");
          isort_cmd
            .args(build_ruff_import_sort_args(&scratch, &ctx.lang_config))
            .args(&inline_config);
          isort_cmd.current_dir(ctx.root.as_path());
          let isort_out = isort_cmd.output()?;
          if !isort_out.status.success() {
            return Ok(isort_out);
          }

          let mut fmt_cmd = tooling::create_tool_command("ruff");
          fmt_cmd.args(build_ruff_format_args(
            &inline_config,
            &scratch,
            &ctx.lang_config,
          ));
          fmt_cmd.current_dir(ctx.root.as_path());
          fmt_cmd.output()
        },
        self.name(),
        start,
        // The isort pass runs `ruff check --select I --fix`. Drift is fixed
        // in place. A non-zero exit means either violations ruff could not fix
        // or an operational failure. Without widened selection, exit 1 indicates an
        // operational failure (e.g. an `invalid-syntax` or `E999` syntax error, which
        // prevents formatting). However, when `extra_args` widens selection (e.g.
        // `--extend-select`), exit 1 indicates leftover lint violations and must be
        // classified as `ViolationsFound`, not `ExecutionError` (Fixes #208).
        // Exit 2 from ruff erroring always remains `ExecutionError`.
        move |code| {
          if widens_selection && code == Some(1) {
            tooling::ExitClass::ViolationsFound
          } else {
            tooling::ExitClass::ExecutionError
          }
        },
      );
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut isort_cmd = tooling::create_tool_command("ruff");
    isort_cmd.args(build_ruff_import_sort_args(
      &files_to_pass,
      &ctx.lang_config,
    ));
    isort_cmd.args(&inline_config);
    isort_cmd.current_dir(ctx.root.as_path());

    match isort_cmd.output() {
      Ok(output) => {
        if !output.status.success() {
          let stderr = String::from_utf8_lossy(&output.stderr).to_string();
          let stdout = String::from_utf8_lossy(&output.stdout).to_string();
          let msg = tooling::merge_tool_streams(
            &stdout,
            &stderr,
            "Import sorting issues found in Python files",
          );

          // `ruff check --select I --fix` fixes any import-sort drift in
          // place. When `extra_args` widens selection (e.g. `--extend-select`)
          // or the output contains lint findings, exit 1 represents lint violations
          // found, not an execution failure (Fixes #208). Otherwise (e.g. syntax
          // errors or exit 2 operational errors), classify as `ExecutionError` (Fixes #155).
          let is_violation = is_ruff_import_sort_violation(
            output.status.code(),
            &stdout,
            &stderr,
            check_args,
          );

          return surfaces::SurfaceResult {
            surface_name: self.name(),
            status: if is_violation {
              surfaces::SurfaceStatus::ViolationsFound {
                message: msg,
                diff: None,
              }
            } else {
              surfaces::SurfaceStatus::ExecutionError { message: msg }
            },
            duration: start.elapsed(),
          };
        }
      }
      Err(e) => {
        return surfaces::SurfaceResult {
          surface_name: self.name(),
          status: surfaces::SurfaceStatus::ExecutionError {
            message: format!("Failed to execute ruff import sorting: {e}"),
          },
          duration: start.elapsed(),
        };
      }
    }

    let mut cmd = tooling::create_tool_command("ruff");
    cmd.args(build_ruff_format_args(
      &inline_config,
      &files_to_pass,
      &ctx.lang_config,
    ));
    cmd.current_dir(ctx.root.as_path());

    // `ruff format` (no `--check`) exits 0 formatted-or-not and only exits 2
    // on a parse/IO/config error, so every non-zero exit here is operational
    // too (Fixes #155).
    tooling::run_tool_command_classified(
      self.name(),
      &mut cmd,
      tooling::classify_all_nonzero_as_error,
    )
  }

  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "ruff", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(PYTHON_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    let files_to_pass = ctx.files_to_pass(files);

    let lint_config = ruff_lint_args(&ruff_config(ctx));

    let mut cmd = tooling::create_tool_command("ruff");
    cmd.args(build_ruff_check_args(&files_to_pass, fix, &ctx.lang_config));
    cmd.args(&lint_config);
    cmd.current_dir(ctx.root.as_path());

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #151 [pre-recreation]): they
  // pass the resolved config to ruff inline via repeated `--config key=val`
  // flags (see `RUFF_FORMAT_INLINE_KEYS` /
  // `ruff_lint_args`, used in `format()`/`lint()`
  // above). This method is now reached only by `fml sync`, for users who
  // explicitly want `ruff.toml` materialized on disk.
  fn sync_config(
    &self,
    ctx: &surfaces::ExecutionContext,
    check: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();
    ruff_config(ctx).sync(ctx, check, start, self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::surfaces;
  use crate::surfaces::LanguageSurface;
  use std::path;

  /// A python config whose `extra_args` sets `args` for `tool` only.
  fn lang_with(tool: &str, args: &[&str]) -> config::ResolvedLangConfig {
    let mut lang = config::ResolvedLangConfig::new("python");
    lang.extra_args = [(
      tool.to_string(),
      args.iter().map(ToString::to_string).collect(),
    )]
    .into();
    lang
  }

  #[test]
  fn python_extra_args_reach_only_their_pass() {
    // Fixes #210: `ruff-check` args reach both `ruff check` invocations and
    // never `ruff format`, and `ruff-format` args the reverse.
    let mut lang = lang_with(RUFF_CHECK, &["--extend-select", "F"]);
    lang
      .extra_args
      .insert(RUFF_FORMAT.to_string(), vec!["--preview".to_string()]);
    let files = [path::PathBuf::from("a.py")];
    let inline = ["--config".to_string(), "line-length=100".to_string()];

    assert_eq!(
      build_ruff_import_sort_args(&files, &lang),
      [
        "check",
        "--select",
        "I",
        "--fix",
        "a.py",
        "--extend-select",
        "F"
      ]
    );
    assert_eq!(
      build_ruff_check_args(&files, false, &lang),
      ["check", "a.py", "--extend-select", "F"]
    );
    assert_eq!(
      build_ruff_format_args(&inline, &files, &lang),
      ["format", "--config", "line-length=100", "a.py", "--preview"]
    );
  }

  #[test]
  fn build_ruff_check_args_with_and_without_fix() {
    let no_fix = build_ruff_check_args(
      &[],
      false,
      &config::ResolvedLangConfig::new("python"),
    );
    assert_eq!(no_fix, vec!["check".to_string(), ".".to_string()]);

    let files = vec![path::PathBuf::from("a.py"), path::PathBuf::from("b.py")];
    let extra = lang_with(RUFF_CHECK, &["--isolated"]);
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
  fn python_sync_config_lint_table_and_quote_style() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = PythonSurface;
    let mut lang_cfg = config::ResolvedLangConfig::new("python");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.python = Some(config::options::PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: Some("py312".to_string()),
      ignore_rules: Some(vec!["E501".to_string(), "F401".to_string()]),
    });

    let ctx = surfaces::test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert_eq!(res.status.created_file_names(), ["ruff.toml"]);

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
  fn python_sync_config_default_omitted_ignore_rules() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = PythonSurface;
    let mut lang_cfg = config::ResolvedLangConfig::new("python");
    lang_cfg.python = Some(config::options::PythonOptions {
      quote_style: Some("double".to_string()),
      target_version: None,
      ignore_rules: None,
    });

    let ctx = surfaces::test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert_eq!(res.status.created_file_names(), ["ruff.toml"]);

    let config_path = temp.path().join("ruff.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[lint]"));
    assert!(content.contains("ignore = []"));
  }

  /// The file, the format overrides and the lint overrides all come from one
  /// settings value: options flow through, CR falls back to LF, and an empty
  /// ignore list is left out of the lint overrides.
  #[test]
  fn ruff_config_table() {
    let options = |ignore: &[&str]| config::options::PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: Some("py311".to_string()),
      ignore_rules: Some(ignore.iter().map(|s| (*s).to_string()).collect()),
    };
    // (options, end_of_line, file lines, format arg, lint arg present?)
    let cases = [
      (
        None,
        "lf",
        vec![
          "line-length = 80",
          "quote-style = \"double\"",
          "ignore = []",
        ],
        "format.quote-style='double'",
        ("lint.ignore=", false),
      ),
      (
        Some(options(&["E501", "F401"])),
        "cr",
        vec!["target-version = \"py311\"", "line-ending = \"lf\""],
        "target-version='py311'",
        ("lint.ignore=['E501','F401']", true),
      ),
      (
        Some(options(&[])),
        "crlf",
        vec!["line-ending = \"crlf\"", "quote-style = \"single\""],
        "format.line-ending='crlf'",
        ("lint.ignore=", false),
      ),
    ];
    for (python, eol, file_lines, format_arg, (lint_arg, lint_present)) in cases
    {
      let mut lang = config::ResolvedLangConfig::new("python");
      lang.python = python;
      let mut ctx = surfaces::test_ctx(path::Path::new("."), lang);
      ctx.global_config = std::sync::Arc::new(config::ResolvedGlobalConfig {
        end_of_line: eol.to_string(),
        ..Default::default()
      });
      let cfg = ruff_config(&ctx);
      let file = cfg.render();
      for line in file_lines {
        assert!(file.contains(line), "{eol}: {line} missing:\n{file}");
      }
      let format_args =
        cfg.flagged("--config", RUFF_FORMAT_INLINE_KEYS, native::Quote::Single);
      assert!(
        format_args.iter().any(|a| a == format_arg),
        "{format_args:?}"
      );
      let lint_args = ruff_lint_args(&cfg);
      assert!(
        lint_args
          .contains(&"lint.select=['E','F','I','UP','B','SIM']".to_string())
      );
      assert_eq!(
        lint_args.iter().any(|a| a.starts_with(lint_arg)),
        lint_present,
        "{lint_args:?}"
      );
    }
  }

  #[test]
  fn python_surface_file_extensions_and_pyi_detection() {
    let surface = PythonSurface;
    assert_eq!(surface.file_extensions(), &["py", "pyi"]);

    let temp = tempfile::TempDir::new().unwrap();
    assert!(!surfaces::detect_in(&surface, temp.path()));

    // Create a .pyi stub file
    let pyi_file = temp.path().join("types.pyi");
    std::fs::write(&pyi_file, "def foo(x: int) -> str: ...").unwrap();
    assert!(surfaces::detect_in(&surface, temp.path()));
  }
  #[test]
  fn test_build_ruff_import_sort_args() {
    let no_files = build_ruff_import_sort_args(
      &[],
      &config::ResolvedLangConfig::new("python"),
    );
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

    let files = vec![path::PathBuf::from("a.py"), path::PathBuf::from("b.py")];
    let extra = lang_with(RUFF_CHECK, &["--isolated"]);
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
  fn python_format_with_import_sorting() {
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("test.py");
    let unformatted = "import sys\nimport os\n\ndef   foo( ):\n  pass\n";
    std::fs::write(&file, unformatted).unwrap();

    let surface = PythonSurface;
    let mut ctx_check = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("python"),
    );
    ctx_check.check_only = true;

    let check_res = surface.format(&ctx_check);
    assert!(matches!(
      check_res.status,
      surfaces::SurfaceStatus::ViolationsFound { .. }
    ));

    let ctx_fix = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("python"),
    );

    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, surfaces::SurfaceStatus::Passed));

    let formatted = std::fs::read_to_string(&file).unwrap();
    let os_idx = formatted.find("import os").unwrap();
    let sys_idx = formatted.find("import sys").unwrap();
    assert!(os_idx < sys_idx);

    let check_clean = surface.format(&ctx_check);
    assert!(matches!(
      check_clean.status,
      surfaces::SurfaceStatus::Passed
    ));
  }

  #[test]
  fn python_format_and_lint_do_not_write_ruff_toml() {
    // Fixes #151 [pre-recreation]: `fml fmt`/`fml lint` must not write `ruff.toml` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.py"), "x=1\n").unwrap();

    let surface = PythonSurface;
    let ctx = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("python"),
    );

    let _ = surface.format(&ctx);
    let _ = surface.lint(&ctx, false);

    assert!(!temp.path().join("ruff.toml").exists());
    assert!(!temp.path().join(".ruff.toml").exists());
  }

  #[test]
  fn python_check_reports_execution_error_on_formatter_failure() {
    // Fixes #151: an unparseable file on `fml fmt --check` makes both ruff
    // passes fail (the isort `ruff check` pass reports `E999` and exits
    // non-zero, `ruff format` exits 2). That must classify as
    // `ExecutionError` (`[ERR]`), not a lint-style `ViolationsFound`
    // (`[FAIL]`) — nothing on this path is a formatting result.
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("broken.py"), "def (:\n    return\n")
      .unwrap();

    let surface = PythonSurface;
    let mut ctx = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("python"),
    );
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on --check must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn python_write_reports_execution_error_on_formatter_failure() {
    // Fixes #155: the non-`--check` write path must classify operational
    // ruff failures as `ExecutionError`, not `ViolationsFound`.
    // On a syntax-error file, the isort pass (`ruff check --select I --fix`)
    // fails first and exercises the isort return branch of the write path.
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("broken.py"), "def (:\n    return\n")
      .unwrap();

    let surface = PythonSurface;
    let ctx = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("python"),
    );

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on the write path must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn python_write_reports_execution_error_on_ruff_format_failure() {
    // Fixes #174: the isort pass (`ruff check --select I --fix`) succeeds on
    // valid syntax with `--ignore E501`, allowing the write path to proceed
    // to the trailing `ruff format` pass. `ruff format` rejects `--ignore`
    // (exiting 2), asserting that `run_tool_command_classified` at the tail
    // of the write path classifies it as `ExecutionError`.
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("valid.py"), "x = 1\n").unwrap();

    let surface = PythonSurface;
    // `ruff format` rejects `--ignore`, so the format pass specifically fails.
    let config = lang_with(RUFF_FORMAT, &["--ignore", "E501"]);
    let ctx = surfaces::test_ctx(temp.path(), config);

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a ruff format failure on the write path must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn test_extra_args_widen_selection() {
    assert!(!extra_args_widen_selection(&[]));
    assert!(!extra_args_widen_selection(&[
      "--ignore".to_string(),
      "E501".to_string(),
    ]));
    assert!(extra_args_widen_selection(&[
      "--extend-select".to_string(),
      "F821".to_string(),
    ]));
    assert!(extra_args_widen_selection(&[
      "--extend-select=F".to_string(),
    ]));
    assert!(extra_args_widen_selection(&[
      "--select".to_string(),
      "ALL".to_string(),
    ]));
    assert!(extra_args_widen_selection(&["--select=ALL".to_string(),]));
  }

  #[test]
  fn test_ruff_output_has_lint_findings() {
    // Pure syntax error output should not be treated as lint findings.
    let syntax_err = "invalid-syntax: Expected an identifier\n --> test.py:1:5\nFound 1 error.\n";
    assert!(!ruff_output_has_lint_findings(syntax_err, ""));

    let e999_err = "test.py:1:1: E999 SyntaxError: Expected an identifier\n";
    assert!(!ruff_output_has_lint_findings(e999_err, ""));

    // Lint findings (e.g. F821) should be recognized.
    let lint_finding =
      "F821 Undefined name `undefined_var`\n --> test.py:1:5\nFound 1 error.\n";
    assert!(ruff_output_has_lint_findings(lint_finding, ""));

    // Stderr-only tool failure should not be treated as lint findings.
    assert!(!ruff_output_has_lint_findings(
      "",
      "error: unexpected argument"
    ));
  }

  #[test]
  fn python_write_extend_select_reports_violations_not_execution_error() {
    // Fixes #208: `--extend-select <rule>` in extra_args widens rule selection during
    // the `ruff check --select I --fix` import pass. When violations are found, ruff exits 1.
    // This must be classified as `ViolationsFound` (`[FAIL]`), not `ExecutionError` (`[ERR]`).
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    // Valid Python syntax, but triggers F821 Undefined name
    std::fs::write(temp.path().join("naming.py"), "x = undefined_var\n")
      .unwrap();

    let surface = PythonSurface;
    let config = lang_with(RUFF_CHECK, &["--extend-select", "F"]);
    let ctx = surfaces::test_ctx(temp.path(), config);

    let res = surface.format(&ctx);
    assert!(
      !matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "ruff exit 1 with --extend-select must not be ExecutionError, got: {:?}",
      res.status
    );
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ViolationsFound { .. }),
      "ruff exit 1 with --extend-select must be ViolationsFound, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn python_check_extend_select_reports_violations_not_execution_error() {
    // Fixes #208: Same reproduction on the `--check` path (`ctx.check_only = true`).
    if !tooling::check_binary_exists("ruff") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("naming.py"), "x = undefined_var\n")
      .unwrap();

    let surface = PythonSurface;
    let config = lang_with(RUFF_CHECK, &["--extend-select", "F"]);
    let mut ctx = surfaces::test_ctx(temp.path(), config);
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      !matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "ruff exit 1 with --extend-select on --check must not be ExecutionError, got: {:?}",
      res.status
    );
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ViolationsFound { .. }),
      "ruff exit 1 with --extend-select on --check must be ViolationsFound, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }
}
