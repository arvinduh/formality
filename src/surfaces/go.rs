//! Go language surface: formats via `gofmt`/`goimports` and lints via
//! `golangci-lint`, syncing the managed `.golangci.yml` from
//! `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo,
  classify_all_nonzero_as_error, classify_exit_one_as_violation,
  create_tool_command, diff_check_via_tempcopy_classified, find_files_with_ext,
  render_native_config, run_tool_command_classified, sync_native_config,
  tool_missing_guard,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
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

/// Linters configuration block for `.golangci.yml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GolangciLintersConfig {
  /// Enabled linter names.
  pub enable: Vec<String>,
}

/// Native `.golangci.yml` configuration representation for Go linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GolangciLintConfig {
  /// Schema/version identifier string for golangci-lint.
  pub version: String,
  /// Linters configuration subsection.
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

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Go language surface implementation.
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

/// Standard file extensions recognized for Go source files.
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

/// Builds argument vector for a machine-readable `golangci-lint run`
/// invocation, used by the LSP server (`fml lsp`, Fixes #159, #165) to
/// translate individual violations into per-file `Diagnostic`s instead of
/// one generic warning. Mirrors [`build_golangci_lint_args`] but requests
/// `--output.json.path=stdout` output instead of `--fix`. Verified against
/// a real golangci-lint v2.5.0 run — v2 renamed the old (v1) `--out-format`
/// flag to the `--output.<format>.path` family; this module targets v2 only,
/// consistent with [`golangci_lint_supports_enable_only`] already assuming a
/// v2 install for `fml lint`'s own inline-linter-set path.
#[must_use]
pub fn build_golangci_lint_json_args(
  files: &[PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args =
    vec!["run".to_string(), "--output.json.path=stdout".to_string()];
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

/// Renders the resolved linter set as the `--enable-only <comma-list>` flag
/// golangci-lint v2 accepts inline, so `fml lint` can apply
/// formality.toml's configured linter set without writing `.golangci.yml`
/// to disk (Fixes #157). Unlike `--enable`/`--disable` (which toggle
/// individual linters against whatever the active config, or the tool's own
/// default set, already enables), `--enable-only` *replaces* the active
/// linter set outright — verified with golangci-lint v2.12.2 to produce
/// identical diagnostic output to a `.golangci.yml` with the same
/// `linters.enable` list (both correctly flag/omit an unchecked
/// `os.Open` return depending on whether `errcheck` is in the set). Only
/// `fml sync` writes that file now (see [`GoSurface::sync_config`]).
#[must_use]
pub fn build_golangci_lint_inline_args(linters: &[String]) -> Vec<String> {
  vec!["--enable-only".to_string(), linters.join(",")]
}

/// Returns whether the installed `golangci-lint` accepts `--enable-only`
/// (added in v2). While `tooling::GOLANGCI_LINT_CHAIN` installs v2, users
/// may already have an older v1 install on PATH — v1 doesn't have this flag,
/// and passing it unconditionally would break `fml lint` outright for anyone
/// on that version rather than just falling back to less-precise config.
/// Probing `run --help`'s actual output avoids guessing at a version-string
/// format that could itself drift.
#[must_use]
pub fn golangci_lint_supports_enable_only() -> bool {
  static CACHE: OnceLock<bool> = OnceLock::new();
  *CACHE.get_or_init(|| {
    create_tool_command("golangci-lint")
      .arg("run")
      .arg("--help")
      .output()
      .is_ok_and(|out| {
        String::from_utf8_lossy(&out.stdout).contains("--enable-only")
          || String::from_utf8_lossy(&out.stderr).contains("--enable-only")
      })
  })
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

    if let Some(res) = tool_missing_guard(
      self.name(),
      "gofmt",
      start,
      Some("Ships with the Go toolchain: install Go from https://go.dev/dl/"),
    ) {
      return res;
    }

    if let Some(res) = tool_missing_guard(
      self.name(),
      "goimports",
      start,
      Some("go install golang.org/x/tools/cmd/goimports@latest"),
    ) {
      return res;
    }

    let files = ctx.matched_files(GO_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
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
      return diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut gofmt_cmd = create_tool_command("gofmt");
          gofmt_cmd.arg("-s").arg("-w").arg(scratch);
          gofmt_cmd.current_dir(ctx.root.as_path());
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
          goimports_cmd.current_dir(ctx.root.as_path());
          goimports_cmd.output()
        },
        self.name(),
        start,
        // `gofmt -w` / `goimports -w` exit non-zero only on a parse
        // failure — never to signal reformatting — so any non-zero exit
        // is an `ExecutionError`, matching the non-`--check` write path.
        classify_all_nonzero_as_error,
      );
    }

    let mut gofmt_cmd = create_tool_command("gofmt");
    gofmt_cmd.arg("-s").arg("-w");
    for f in &files {
      gofmt_cmd.arg(f);
    }
    gofmt_cmd.current_dir(ctx.root.as_path());

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
    goimports_cmd.current_dir(ctx.root.as_path());

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

    if let Some(res) = tool_missing_guard(
      self.name(),
      "golangci-lint",
      start,
      Some(
        "brew install golangci-lint / go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest",
      ),
    ) {
      return res;
    }

    let files = ctx.matched_files(GO_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let files_to_pass = ctx.files_to_pass(files);

    // Inline `--enable-only linter1,linter2,...` instead of relying on
    // `.golangci.yml` being present on disk — see
    // `build_golangci_lint_inline_args` (Fixes #157). `fml sync` remains the
    // only path that materializes the file.
    let linters = ctx
      .lang_config
      .go
      .as_ref()
      .and_then(|g| g.linters.clone())
      .unwrap_or_else(default_go_linters);

    let mut cmd = create_tool_command("golangci-lint");
    if golangci_lint_supports_enable_only() {
      cmd.args(build_golangci_lint_inline_args(&linters));
    }
    // Older golangci-lint v1 installs (no `--enable-only`, see above) fall
    // through here with no inline linter-set flag, same as before #157 —
    // they still respect a synced `.golangci.yml` if one is on disk.
    cmd.args(build_golangci_lint_args(
      &files_to_pass,
      fix,
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(ctx.root.as_path());

    // golangci-lint exits `1` for "issues found" and non-`1` for "could not
    // run" (`7` = typecheck/config error, `2`/`3`/`5`/`6` = other internal
    // failures). Only `1` is a lint result; everything else is an
    // `ExecutionError` so `fml lint` renders `[ERR]` with the real cause
    // instead of a misleading `Violations found` (Fixes #107).
    run_tool_command_classified(
      self.name(),
      &mut cmd,
      classify_exit_one_as_violation,
    )
  }

  // `fml lint` no longer goes through this path (Fixes #157): it passes the
  // resolved linter set to golangci-lint inline via `--enable-only
  // linter1,linter2,...` (see `build_golangci_lint_inline_args`, used in
  // `lint()` above). Unlike `--enable`/`--disable` (which the #151-era
  // comment here correctly noted only toggle individual linters against
  // whatever's already active), golangci-lint v2's `--enable-only` *replaces*
  // the active linter set outright — verified with golangci-lint v2.12.2
  // actually installed: `--enable-only errcheck` against a project with an
  // unchecked `os.Open` call produces byte-identical diagnostic output to a
  // `.golangci.yml` with `linters.enable: [errcheck]`, and omitting
  // `errcheck` from the inline set (or the file) both correctly suppress the
  // finding. gofmt/goimports (the formatters `format()` calls) are
  // unaffected either way — they've never read `.golangci.yml`; only
  // `golangci-lint` (the linter) does, and never needed a config file of
  // their own to begin with (both take all their settings as CLI flags).
  // This method is now reached only by `fml sync`, for users who explicitly
  // want `.golangci.yml` materialized on disk (e.g. for editor/IDE
  // integration outside of `fml`, or CI that invokes golangci-lint
  // directly) — and for anyone still on golangci-lint v1, where
  // `golangci_lint_supports_enable_only()` returns false and `lint()`
  // gracefully falls back to whatever `.golangci.yml` is on disk instead of
  // passing an unrecognized flag.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<GolangciLintConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::{GoOptions, ResolvedLangConfig};
  use crate::surfaces::{check_binary_exists, test_ctx};
  use std::sync::{Mutex, MutexGuard, PoisonError};
  use tempfile::TempDir;

  /// `golangci-lint run` takes a machine-global lock and aborts with
  /// `parallel golangci-lint is running` if a second invocation overlaps.
  /// libtest runs these tests on parallel threads, so every test that
  /// actually invokes `golangci-lint run` (via `GoSurface::lint`) must hold
  /// this guard for the duration of the call.
  static GOLANGCI_LINT_GUARD: Mutex<()> = Mutex::new(());

  fn golangci_lint_lock() -> MutexGuard<'static, ()> {
    GOLANGCI_LINT_GUARD
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
  }

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
  fn test_build_golangci_lint_json_args_default_scope() {
    let args = build_golangci_lint_json_args(&[], &[]);
    assert_eq!(
      args,
      vec![
        "run".to_string(),
        "--output.json.path=stdout".to_string(),
        "./...".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_golangci_lint_json_args_with_files() {
    let files = vec![PathBuf::from("main.go")];
    let args = build_golangci_lint_json_args(&files, &[]);
    assert_eq!(
      args,
      vec![
        "run".to_string(),
        "--output.json.path=stdout".to_string(),
        "main.go".to_string(),
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
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("go"));

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

    let ctx = test_ctx(temp.path(), lang_cfg);

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
    let mut ctx_check = test_ctx(temp.path(), ResolvedLangConfig::new("go"));
    ctx_check.check_only = true;

    let check_res = surface.format(&ctx_check);
    assert!(matches!(
      check_res.status,
      SurfaceStatus::ViolationsFound { .. }
    ));

    let ctx_fix = test_ctx(temp.path(), ResolvedLangConfig::new("go"));

    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, SurfaceStatus::Passed));

    let formatted = std::fs::read_to_string(&file).unwrap();
    assert!(formatted.contains("\tfmt.Println(\"hi\")"));

    let check_clean = surface.format(&ctx_check);
    assert!(matches!(check_clean.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_build_golangci_lint_inline_args_shape() {
    let linters = vec!["errcheck".to_string(), "govet".to_string()];
    let args = build_golangci_lint_inline_args(&linters);
    assert_eq!(
      args,
      vec!["--enable-only".to_string(), "errcheck,govet".to_string()]
    );
  }

  #[test]
  fn test_golangci_lint_supports_enable_only_matches_installed_binary() {
    // Not a hardcoded assertion either way — this just confirms the probe
    // doesn't panic and its result is consistent with a direct check
    // against the same binary this test environment actually has (when
    // golangci-lint is on PATH).
    if !check_binary_exists("golangci-lint") {
      return;
    }
    let supports = golangci_lint_supports_enable_only();
    let help_output = std::process::Command::new("golangci-lint")
      .arg("run")
      .arg("--help")
      .output()
      .expect("golangci-lint run --help should succeed if the binary exists");
    let expected = String::from_utf8_lossy(&help_output.stdout)
      .contains("--enable-only")
      || String::from_utf8_lossy(&help_output.stderr).contains("--enable-only");
    assert_eq!(supports, expected);
  }

  #[test]
  fn test_go_lint_does_not_write_golangci_yml() {
    // Fixes #157: `fml lint` must not write `.golangci.yml` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !check_binary_exists("golangci-lint") {
      return;
    }
    let _guard = golangci_lint_lock();
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("go.mod"),
      "module example.com/testproj\n\ngo 1.21\n",
    )
    .unwrap();
    std::fs::write(
      temp.path().join("main.go"),
      "package main\n\nfunc main() {}\n",
    )
    .unwrap();

    let surface = GoSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("go"));

    let _ = surface.lint(&ctx, false);

    assert!(
      !temp.path().join(".golangci.yml").exists(),
      "fml lint must not write .golangci.yml"
    );
  }

  #[test]
  fn test_go_lint_respects_configured_linter_set() {
    // Verifies `--enable-only` actually drives which linters run, matching
    // the resolved `[lang.go] linters` set, without any `.golangci.yml` on
    // disk (Fixes #157).
    if !check_binary_exists("golangci-lint") {
      return;
    }
    let _guard = golangci_lint_lock();
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("go.mod"),
      "module example.com/testproj\n\ngo 1.21\n",
    )
    .unwrap();
    // Unchecked os.Open return: flagged by errcheck, ignored by govet.
    std::fs::write(
      temp.path().join("main.go"),
      "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n\nfunc main() {\n\tos.Open(\"nope.txt\")\n\tfmt.Println(\"hi\")\n}\n",
    )
    .unwrap();

    let surface = GoSurface;

    let mut errcheck_cfg = ResolvedLangConfig::new("go");
    errcheck_cfg.go = Some(GoOptions {
      local_prefixes: None,
      linters: Some(vec!["errcheck".to_string()]),
    });
    let ctx_errcheck = test_ctx(temp.path(), errcheck_cfg);
    let res_errcheck = surface.lint(&ctx_errcheck, false);
    assert!(matches!(
      res_errcheck.status,
      SurfaceStatus::ViolationsFound { .. }
    ));

    let mut govet_cfg = ResolvedLangConfig::new("go");
    govet_cfg.go = Some(GoOptions {
      local_prefixes: None,
      linters: Some(vec!["govet".to_string()]),
    });
    let ctx_govet = test_ctx(temp.path(), govet_cfg);
    let res_govet = surface.lint(&ctx_govet, false);
    assert!(matches!(res_govet.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_go_lint_without_go_mod_is_execution_error_not_violation() {
    // Headline repro for #107: golangci-lint in a directory with no `go.mod`
    // exits 7 with a typecheck error on stderr and "0 issues." on stdout.
    // Before the fix this rendered as `[FAIL] Violations found` showing only
    // the contradictory "0 issues." line; it must now be an `ExecutionError`
    // carrying the real cause.
    if !check_binary_exists("golangci-lint") {
      return;
    }
    let _guard = golangci_lint_lock();
    let temp = TempDir::new().unwrap();
    // Deliberately no go.mod.
    std::fs::write(
      temp.path().join("main.go"),
      "package main\n\nfunc main() {}\n",
    )
    .unwrap();

    let surface = GoSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("go"));
    let res = surface.lint(&ctx, false);

    match res.status {
      SurfaceStatus::ExecutionError { message } => {
        let lower = message.to_lowercase();
        assert!(
          lower.contains("module")
            || lower.contains("go.mod")
            || lower.contains("typechecking")
            // Defensive: if some other suite invokes golangci-lint
            // concurrently despite the guard, its global-lock abort is
            // still a non-zero exit proving the point (not `ViolationsFound`).
            || lower.contains("parallel golangci-lint is running"),
          "message should carry golangci-lint's real cause, got: {message}"
        );
      }
      other => panic!(
        "no-go.mod golangci-lint failure must be ExecutionError, got {other:?}"
      ),
    }
  }
}
