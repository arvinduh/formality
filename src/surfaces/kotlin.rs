//! Kotlin language surface: formats and lints via `ktlint`. Kotlin has no
//! managed native config file — `ktlint` reads its own `.editorconfig`
//! conventions directly, so there is no `NativeConfig` to sync here.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  SurfaceResult, SurfaceStatus, ToolInfo, classify_exit_one_as_violation,
  create_tool_command, diff_check_via_tempcopy_classified, find_files_with_ext,
  run_tool_command, run_tool_command_classified, tool_missing_guard,
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

  fn detect(&self, root: &Path) -> bool {
    root.join("build.gradle.kts").is_file()
      || root.join("settings.gradle.kts").is_file()
      || !find_files_with_ext(root, KOTLIN_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn supports_lint_fix(&self) -> bool {
    true
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

    if let Some(res) = tool_missing_guard(
      self.name(),
      "ktlint",
      start,
      Some("brew install ktlint"),
    ) {
      return res;
    }

    let files = ctx.matched_files(KOTLIN_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    if ctx.check_only {
      return diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("ktlint");
          cmd.arg("-F").arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
        // ktlint is the one prettier-adjacent formatter here whose fix mode
        // (`-F`) *does* have a non-zero exit that is not purely operational:
        // it exits `1` both when it auto-corrected some issues but others
        // remain (e.g. a non-autofixable rule like `enum-entry-name-case`)
        // *and* on a genuine failure (a `KotlinParseException`, a bad rule
        // config, a JVM error) — there is no distinct exit `2` to tell them
        // apart. So `classify_all_nonzero_as_error` would be wrong here: it
        // would relabel a run that merely found an unfixable style nit as an
        // `[ERR] Execution error`, and it would diverge from the unclassified
        // write path, which still renders ktlint's exit `1` as `[FAIL]
        // Violations found`. `classify_exit_one_as_violation` keeps that exit
        // `1` behaviour identical to today while still routing any *other*
        // non-zero exit (a signal kill, or a future ktlint that adopts a
        // dedicated operational-error code) to `ExecutionError` (Fixes #151).
        // The write branch below applies the identical classifier for the
        // identical reason (Fixes #155): it is already the "unclassified
        // write path" this comment refers to, made explicit rather than
        // relying on `run_tool_command`'s default all-nonzero-is-violation
        // behavior, which happened to agree on exit 1 but silently mapped
        // any other non-zero exit to `ViolationsFound` too.
        classify_exit_one_as_violation,
      );
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut cmd = create_tool_command("ktlint");
    cmd.args(build_ktlint_format_args(
      &files_to_pass,
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(ctx.root.as_path());

    run_tool_command_classified(
      self.name(),
      &mut cmd,
      classify_exit_one_as_violation,
    )
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "ktlint",
      start,
      Some("brew install ktlint"),
    ) {
      return res;
    }

    let files = ctx.matched_files(KOTLIN_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut cmd = create_tool_command("ktlint");
    cmd.args(build_ktlint_lint_args(
      &files_to_pass,
      fix,
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
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
  use crate::config::ResolvedLangConfig;
  use crate::surfaces::{check_binary_exists, test_ctx};
  use std::path::PathBuf;
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
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("kotlin"));

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
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("kotlin"));

    let res = surface.sync_config(&ctx, false);
    assert!(matches!(res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_kotlin_format_with_real_ktlint() {
    if !check_binary_exists("ktlint")
      || !create_tool_command("ktlint")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
      return;
    }
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("Main.kt");
    let unformatted = "import kotlin.math.min\nimport kotlin.math.max\n\nfun main() {\nval x=1\nprintln(x)\n}\n";
    std::fs::write(&file, unformatted).unwrap();

    let surface = KotlinSurface;
    let mut ctx_check =
      test_ctx(temp.path(), ResolvedLangConfig::new("kotlin"));
    ctx_check.check_only = true;
    let check_res = surface.format(&ctx_check);
    assert!(matches!(
      check_res.status,
      SurfaceStatus::ViolationsFound { .. }
    ));

    let ctx_fix = test_ctx(temp.path(), ResolvedLangConfig::new("kotlin"));
    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, SurfaceStatus::Passed));

    let lint_res = surface.lint(&ctx_check, false);
    assert!(matches!(lint_res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_kotlin_check_exit_one_stays_violation_not_execution_error() {
    // Issue #151: unlike the other prettier-adjacent surfaces, ktlint `-F`
    // has no operational-failure exit code distinct from "found an unfixable
    // violation" — both are exit `1`. This surface is therefore wired to
    // `classify_exit_one_as_violation`, NOT `classify_all_nonzero_as_error`:
    // a ktlint exit `1` on `fml fmt --check` must stay `ViolationsFound`
    // (`[FAIL]`), matching the unclassified write path, and only a non-`1`
    // non-zero exit (signal kill, or a hypothetical future operational code)
    // becomes `ExecutionError`. An unparseable file drives ktlint to exit
    // `1`, so it must not flip to `[ERR]`.
    if !check_binary_exists("ktlint") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("Broken.kt"), "fun main( { val x = }\n")
      .unwrap();

    let surface = KotlinSurface;
    let mut ctx = test_ctx(temp.path(), ResolvedLangConfig::new("kotlin"));
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      !matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "ktlint exit 1 must not be reclassified as ExecutionError, got: {:?}",
      res.status
    );
    assert!(matches!(res.status, SurfaceStatus::ViolationsFound { .. }));
  }

  #[test]
  fn test_kotlin_write_exit_one_stays_violation_not_execution_error() {
    // Fixes #155: the non-`--check` write path now explicitly runs through
    // `classify_exit_one_as_violation` too (previously the unclassified
    // `run_tool_command`, which happened to treat every non-zero exit as
    // `ViolationsFound` and so agreed with this classifier only on exit 1).
    // Same case as
    // `test_kotlin_check_exit_one_stays_violation_not_execution_error`
    // above: an unparseable file drives ktlint `-F` to exit `1`, which must
    // stay `ViolationsFound` (`[FAIL]`), not flip to `ExecutionError`
    // (`[ERR]`).
    if !check_binary_exists("ktlint") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("Broken.kt"), "fun main( { val x = }\n")
      .unwrap();

    let surface = KotlinSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("kotlin"));

    let res = surface.format(&ctx);
    assert!(
      !matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "ktlint exit 1 on the write path must not be reclassified as ExecutionError, got: {:?}",
      res.status
    );
    assert!(matches!(res.status, SurfaceStatus::ViolationsFound { .. }));
  }
}
