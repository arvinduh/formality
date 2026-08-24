use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, diff_check_via_tempcopy, find_files_with_ext,
  tool_missing_result,
};
use std::path::Path;
use std::time::Instant;

/// Kotlin language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct KotlinSurface;

impl DeclaresFacets for KotlinSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      // ktlint's default (official) code style enforces 4-space indentation
      // and does not offer a tab-based mode.
      Facet::IndentTabs => FacetSupport::Fixed("spaces"),
      // ktlint's standard ruleset enforces double-quoted strings.
      Facet::QuoteStyle => FacetSupport::Fixed("double"),
      // "Smart Format": `ktlint -F` organizes/sorts imports as part of the
      // same pass that reformats code, so this always runs together with
      // format().
      Facet::IndentWidth
      | Facet::LineLength
      | Facet::TrailingComma
      | Facet::ImportSort => FacetSupport::Configurable,
      Facet::ProseWrap | Facet::Edition | Facet::Standard => {
        FacetSupport::Unsupported
      }
    }
  }
}

/// Standard file extensions recognized for Kotlin source files.
pub const KOTLIN_EXTENSIONS: &[&str] = &["kt", "kts"];

/// Builds the argument list for an in-place ktlint format ("Smart Format")
/// invocation: `-F` fixes both style violations and import order in one pass.
#[must_use]
pub fn build_ktlint_format_args(
  files: &[std::path::PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["-F".to_string()];
  if files.is_empty() {
    args.push("**/*.kt".to_string());
    args.push("**/*.kts".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds the argument list for a ktlint lint invocation. When `fix` is set
/// this is equivalent to the "Smart Format" pass (`-F`), since ktlint has no
/// separate autofix mode distinct from formatting.
#[must_use]
pub fn build_ktlint_lint_args(
  files: &[std::path::PathBuf],
  fix: bool,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = Vec::new();
  if fix {
    args.push("-F".to_string());
  }
  if files.is_empty() {
    args.push("**/*.kt".to_string());
    args.push("**/*.kts".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds argument vector for a machine-readable `ktlint` invocation, used
/// by the LSP server (`fml lsp`, Fixes #159, #165) to translate individual
/// violations into per-file `Diagnostic`s instead of one generic warning.
/// Mirrors [`build_ktlint_lint_args`] but requests `--reporter=json` output
/// instead of `-F` (this is a read-only diagnostics pass, never a fix).
/// Verified against a real ktlint 1.8.0 run — its JSON reporter writes to
/// stdout, but ktlint's own SLF4J logger can also print a `WARN ...`
/// banner line to stdout *before* the JSON array when violations are
/// autocorrectable (observed; not documented behavior), so the parser must
/// locate the JSON array's start rather than assume stdout is JSON from
/// byte zero.
#[must_use]
pub fn build_ktlint_json_args(
  files: &[std::path::PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["--reporter=json".to_string()];
  if files.is_empty() {
    args.push("**/*.kt".to_string());
    args.push("**/*.kts".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

impl LanguageSurface for KotlinSurface {
  fn name(&self) -> &'static str {
    "kotlin"
  }

  fn aliases(&self) -> &[&'static str] {
    &["kt"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    KOTLIN_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("build.gradle.kts").is_file()
      || root.join("settings.gradle.kts").is_file()
      || !find_files_with_ext(root, KOTLIN_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![ToolInfo {
      binary: "ktlint",
      description: "Kotlin linter and formatter (Smart Format: style + import organization in one pass)",
      install_hint: "Install via: brew install ktlint (or scoop install ktlint, see https://github.com/pinterest/ktlint for other options)",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("ktlint") {
      return tool_missing_result(
        self.name(),
        start,
        "ktlint",
        "brew install ktlint",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      KOTLIN_EXTENSIONS,
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
          let mut cmd = create_tool_command("ktlint");
          cmd.arg("-F").arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(&ctx.root);
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let files_to_pass = if !ctx.paths.is_empty()
      || !ctx.lang_config.files.is_empty()
      || !ctx.lang_config.exclude.is_empty()
    {
      files
    } else {
      Vec::new()
    };

    let mut cmd = create_tool_command("ktlint");
    cmd.args(build_ktlint_format_args(
      &files_to_pass,
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
          let msg = if !stderr.trim().is_empty() {
            stderr
          } else if !stdout.trim().is_empty() {
            stdout
          } else {
            "Formatting issues found in Kotlin files".to_string()
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
          message: format!("Failed to execute ktlint -F: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("ktlint") {
      return tool_missing_result(
        self.name(),
        start,
        "ktlint",
        "brew install ktlint",
      );
    }

    let files = find_files_with_ext(
      &ctx.root,
      KOTLIN_EXTENSIONS,
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

    let mut cmd = create_tool_command("ktlint");
    cmd.args(build_ktlint_lint_args(
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
          message: format!("Failed to execute ktlint: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  fn sync_config(
    &self,
    _ctx: &ExecutionContext,
    _check: bool,
  ) -> SurfaceResult {
    // ktlint reads its layout configuration (indent size, max line length,
    // code style, disabled rules, ...) exclusively from `.editorconfig`,
    // which formality already synthesizes centrally for every surface (see
    // `crate::surfaces::editorconfig::sync_editorconfig`). There is no separate
    // native ktlint config file to generate, so this is a no-op.
    SurfaceResult {
      surface_name: self.name(),
      status: SurfaceStatus::Passed,
      duration: std::time::Duration::default(),
    }
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
  use std::path::PathBuf;
  use std::sync::Arc;
  use tempfile::TempDir;

  #[test]
  fn test_kotlin_surface_facets() {
    let surface = KotlinSurface;
    assert_eq!(
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("spaces")
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
      FacetSupport::Fixed("double")
    );
    assert_eq!(
      surface.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::ProseWrap),
      FacetSupport::Unsupported
    );
  }

  #[test]
  fn test_kotlin_surface_name_aliases_and_extensions() {
    let surface = KotlinSurface;
    assert_eq!(surface.name(), "kotlin");
    assert_eq!(surface.aliases(), &["kt"]);
    assert_eq!(surface.file_extensions(), &["kt", "kts"]);
    assert!(surface.supports_lint_fix());
  }

  #[test]
  fn test_kotlin_surface_tool_info() {
    let surface = KotlinSurface;
    let cfg = ResolvedLangConfig::new("kotlin");
    let tools = surface.tool_info(&cfg);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "ktlint");
    assert!(tools[0].is_required_for_fmt);
    assert!(tools[0].is_required_for_lint);
  }

  #[test]
  fn test_kotlin_surface_detect() {
    let surface = KotlinSurface;
    let temp = TempDir::new().unwrap();
    assert!(!surface.detect(temp.path()));

    let kt_file = temp.path().join("Main.kt");
    std::fs::write(&kt_file, "fun main() {}\n").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_kotlin_surface_detect_gradle_kts() {
    let surface = KotlinSurface;
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("build.gradle.kts"), "").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_build_ktlint_format_args() {
    let no_files = build_ktlint_format_args(&[], &[]);
    assert_eq!(
      no_files,
      vec![
        "-F".to_string(),
        "**/*.kt".to_string(),
        "**/*.kts".to_string()
      ]
    );

    let files = vec![PathBuf::from("Main.kt"), PathBuf::from("Util.kt")];
    let extra = vec!["--relative".to_string()];
    let with_files = build_ktlint_format_args(&files, &extra);
    assert_eq!(
      with_files,
      vec![
        "-F".to_string(),
        "Main.kt".to_string(),
        "Util.kt".to_string(),
        "--relative".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_ktlint_lint_args_with_and_without_fix() {
    let no_fix = build_ktlint_lint_args(&[], false, &[]);
    assert_eq!(no_fix, vec!["**/*.kt".to_string(), "**/*.kts".to_string()]);

    let files = vec![PathBuf::from("Main.kt")];
    let with_fix = build_ktlint_lint_args(&files, true, &[]);
    assert_eq!(with_fix, vec!["-F".to_string(), "Main.kt".to_string()]);
  }

  #[test]
  fn test_build_ktlint_json_args() {
    let no_files = build_ktlint_json_args(&[], &[]);
    assert_eq!(
      no_files,
      vec![
        "--reporter=json".to_string(),
        "**/*.kt".to_string(),
        "**/*.kts".to_string(),
      ]
    );

    let files = vec![PathBuf::from("Main.kt")];
    let extra = vec!["--relative".to_string()];
    let with_files = build_ktlint_json_args(&files, &extra);
    assert_eq!(
      with_files,
      vec![
        "--reporter=json".to_string(),
        "Main.kt".to_string(),
        "--relative".to_string(),
      ]
    );
  }

  #[test]
  fn test_kotlin_format_and_lint_empty_project_passes() {
    // An empty project has no Kotlin files to act on, but ktlint's binary
    // presence is still checked first (matching every other surface's
    // convention, e.g. Python/ruff) — so this only asserts Passed when the
    // tool is actually installed; otherwise it should report ToolMissing.
    let temp = TempDir::new().unwrap();
    let surface = KotlinSurface;
    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("kotlin"),
      check_only: false,
    };

    let fmt_res = surface.format(&ctx);
    let lint_res = surface.lint(&ctx, false);
    if check_binary_exists("ktlint") {
      assert!(matches!(fmt_res.status, SurfaceStatus::Passed));
      assert!(matches!(lint_res.status, SurfaceStatus::Passed));
    } else {
      assert!(matches!(fmt_res.status, SurfaceStatus::ToolMissing { .. }));
      assert!(matches!(lint_res.status, SurfaceStatus::ToolMissing { .. }));
    }
  }

  #[test]
  fn test_kotlin_sync_config_is_noop() {
    let temp = TempDir::new().unwrap();
    let surface = KotlinSurface;
    let ctx = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("kotlin"),
      check_only: false,
    };

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_kotlin_format_with_real_ktlint() {
    if !check_binary_exists("ktlint") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("Main.kt");
    let unformatted = "import kotlin.math.min\nimport kotlin.math.max\n\nfun main() {\nval x=1\nprintln(x)\n}\n";
    std::fs::write(&file, unformatted).unwrap();

    let surface = KotlinSurface;
    let ctx_check = ExecutionContext {
      root: temp.path().to_path_buf(),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("kotlin"),
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
      lang_config: ResolvedLangConfig::new("kotlin"),
      check_only: false,
    };
    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, SurfaceStatus::Passed));

    let lint_res = surface.lint(&ctx_check, false);
    assert!(matches!(lint_res.status, SurfaceStatus::Passed));
  }
}
