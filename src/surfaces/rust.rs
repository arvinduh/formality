//! Rust language surface: formats via `rustfmt`/`cargo fmt` and lints via
//! `cargo clippy`, syncing the managed `.rustfmt.toml` from `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, check_binary_exists,
  create_tool_command, find_files_with_ext, find_manifest_upwards,
  render_native_config, run_tool_command, sync_native_config,
  tool_missing_guard, tool_missing_result,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Native `.rustfmt.toml` configuration representation for Rust formatting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustfmtConfig {
  /// Indentation spaces count per level.
  pub tab_spaces: usize,
  /// Maximum line width before wrapping.
  pub max_width: usize,
  /// Line end newline style (`"Unix"`, `"Windows"`, or `"Auto"`).
  pub newline_style: String,
  /// Small heuristics formatting setting.
  pub use_small_heuristics: String,
  /// Target Rust edition.
  pub edition: String,
  /// Whether to reorder import statements.
  pub reorder_imports: bool,
}

impl NativeConfig for RustfmtConfig {
  const FILE_NAME: &'static str = ".rustfmt.toml";

  fn from_context(ctx: &ExecutionContext) -> Self {
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

    Self {
      tab_spaces: ctx.lang_config.indent_size,
      max_width: ctx.lang_config.line_length,
      newline_style: newline_style.to_string(),
      use_small_heuristics: "Default".to_string(),
      edition: edition.to_string(),
      reorder_imports: true,
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Rust language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct RustSurface;

impl DeclaresFacets for RustSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs => FacetSupport::Fixed("spaces"),
      Facet::IndentWidth
      | Facet::LineLength
      | Facet::ImportSort
      | Facet::Edition => FacetSupport::Configurable,
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ProseWrap
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

/// Builds argument vector for cargo clippy invocation.
#[must_use]
pub fn build_clippy_args(fix: bool, extra_args: &[String]) -> Vec<String> {
  let mut args = vec!["clippy".to_string()];
  if fix {
    args.push("--fix".to_string());
    args.push("--allow-no-vcs".to_string());
    args.push("--allow-dirty".to_string());
    args.push("--allow-staged".to_string());
  }
  args.push("--all-targets".to_string());
  args.push("--".to_string());
  args.push("-D".to_string());
  args.push("warnings".to_string());
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds argument vector for a machine-readable `cargo clippy` invocation,
/// used by the LSP server (`fml lsp`, Fixes #159) to translate individual
/// violations into per-file `Diagnostic`s instead of one generic warning.
/// Mirrors [`build_clippy_args`] but requests `--message-format=json` output
/// instead of `--fix`, since autofixing and machine parsing are mutually
/// exclusive uses of the same invocation.
#[must_use]
pub fn build_clippy_json_args(extra_args: &[String]) -> Vec<String> {
  let mut args = vec![
    "clippy".to_string(),
    "--message-format=json".to_string(),
    "--all-targets".to_string(),
    "--".to_string(),
    "-D".to_string(),
    "warnings".to_string(),
  ];
  args.extend(extra_args.iter().cloned());
  args
}

/// Renders a [`RustfmtConfig`] as the inline `key1=val1,key2=val2` string
/// accepted by rustfmt's/`cargo fmt`'s `--config` flag, so `fml fmt`/`fml
/// lint` can apply the resolved formality.toml settings without writing
/// `.rustfmt.toml` to disk. Only `fml sync` writes that file now (see
/// [`RustSurface::sync_config`]).
#[must_use]
pub fn build_rustfmt_inline_config(cfg: &RustfmtConfig) -> String {
  format!(
    "max_width={},tab_spaces={},newline_style={},use_small_heuristics={},reorder_imports={}",
    cfg.max_width,
    cfg.tab_spaces,
    cfg.newline_style,
    cfg.use_small_heuristics,
    cfg.reorder_imports,
  )
}

pub(crate) fn build_rustfmt_fallback_cmd(
  edition: &str,
  inline_config: &str,
  check_only: bool,
  files: &[PathBuf],
) -> Command {
  let mut c = create_tool_command("rustfmt");
  c.arg("--edition").arg(edition);
  c.arg("--config").arg(inline_config);
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

  fn file_extensions(&self) -> &[&'static str] {
    &["rs"]
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
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

  // Dispatches rustfmt formatting with Cargo.toml discovery, check vs write modes, and error parsing.
  #[allow(clippy::too_many_lines)]
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if !check_binary_exists("cargo") && !check_binary_exists("rustfmt") {
      return tool_missing_result(
        self.name(),
        start,
        "cargo / rustfmt",
        "Run: rustup component add rustfmt",
      );
    }

    let files = ctx.matched_files(&["rs"]);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
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
      ctx
        .lang_config
        .rust
        .as_ref()
        .and_then(|r| r.edition.as_deref())
        .unwrap_or("2021")
    };

    // Inline `--config key=val,...` instead of writing `.rustfmt.toml` to
    // disk — see `build_rustfmt_inline_config` (Fixes #151). `fml sync`
    // remains the only path that materializes the file, for users who want
    // it on disk (e.g. for editor integrations that don't go through `fml`).
    let inline_config =
      build_rustfmt_inline_config(&RustfmtConfig::from_context(ctx));

    let mut cmd =
      if check_binary_exists("cargo") && ctx.root.join("Cargo.toml").exists() {
        let mut c = create_tool_command("cargo");
        c.arg("fmt");
        if ctx.check_only {
          c.arg("--")
            .arg("--check")
            .arg("--config")
            .arg(&inline_config);
        } else {
          c.arg("--").arg("--config").arg(&inline_config);
        }
        if !ctx.paths.is_empty()
          || !ctx.lang_config.files.is_empty()
          || !ctx.lang_config.exclude.is_empty()
        {
          for f in &files {
            c.arg(f);
          }
        }
        c
      } else {
        build_rustfmt_fallback_cmd(
          edition,
          &inline_config,
          ctx.check_only,
          &files,
        )
      };

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "cargo",
      start,
      Some("Install Rust via https://rustup.rs"),
    ) {
      return res;
    }

    // clippy requires a Cargo manifest to build against; without one, cargo
    // fails immediately with "could not find `Cargo.toml`" which is an
    // environment/setup problem, not a lint violation in the code. Surface
    // it as an actionable execution error instead of `Violations found`.
    // `cargo` itself resolves the manifest by walking upward from the
    // working directory (`ctx.root`) through every ancestor, so this guard
    // must do the same via `find_manifest_upwards` (Fixes #185) — checking
    // only `ctx.root` produced false errors for any subdirectory of a real
    // crate, despite the message below already claiming to check parents.
    if !find_manifest_upwards(&ctx.root, "Cargo.toml") {
      return SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ExecutionError {
          message: format!(
            "No Cargo.toml found in {} (or any parent directory). `cargo \
             clippy` needs a Cargo manifest to lint against — run `cargo \
             init` here, or point --root at the crate/workspace root.",
            ctx.root.display()
          ),
        },
        duration: start.elapsed(),
      };
    }

    let mut cmd = create_tool_command("cargo");
    cmd.args(build_clippy_args(fix, &ctx.lang_config.extra_args));
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #151): they
  // pass the resolved config to rustfmt inline via `--config` (see
  // `build_rustfmt_inline_config`, used in `format()` above). This method is
  // now reached only by `fml sync`, for users who explicitly want
  // `.rustfmt.toml` materialized on disk (e.g. for editor/rust-analyzer
  // integration outside of `fml`).
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<RustfmtConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::ResolvedLangConfig;
  use crate::surfaces::test_ctx;
  use tempfile::TempDir;

  #[test]
  fn test_lint_without_cargo_toml_is_execution_error_not_violation() {
    let temp = TempDir::new().unwrap();
    // No Cargo.toml written — mirrors a bare `.rs` file with no crate manifest.
    let surface = RustSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));

    let res = surface.lint(&ctx, false);
    match res.status {
      SurfaceStatus::ExecutionError { message } => {
        assert!(message.contains("Cargo.toml"));
      }
      other => {
        panic!("expected ExecutionError for missing Cargo.toml, got {other:?}")
      }
    }
  }

  #[test]
  fn test_lint_cargo_toml_in_ancestor_directory_is_not_guarded() {
    // A subdirectory of a real crate (Cargo.toml lives above `ctx.root`,
    // mirroring a nested workspace-member layout, e.g. `src/deep`) must not
    // trip the preflight guard (Fixes #185) — `cargo clippy` itself
    // resolves a manifest by walking upward from the working directory
    // exactly the same way, so the guard must mirror that instead of
    // checking only `ctx.root`.
    if !check_binary_exists("cargo") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("Cargo.toml"),
      "[package]\nname = \"testcrate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let nested = temp.path().join("src").join("deep");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(temp.path().join("src").join("lib.rs"), "pub mod deep;\n")
      .unwrap();
    std::fs::write(nested.join("mod.rs"), "pub fn f() {}\n").unwrap();

    let surface = RustSurface;
    let ctx = test_ctx(&nested, ResolvedLangConfig::new("rust"));
    let res = surface.lint(&ctx, false);

    if let SurfaceStatus::ExecutionError { message } = &res.status {
      assert!(
        !message.contains("No Cargo.toml found"),
        "Cargo.toml in an ancestor directory must not trip the \
         missing-manifest guard, got: {message}"
      );
    }
  }

  #[test]
  fn test_lint_directory_named_cargo_toml_is_not_treated_as_manifest() {
    // `.is_file()`, not `.exists()` (Fixes #185): a directory that happens
    // to be named `Cargo.toml` must not be mistaken for the manifest.
    let temp = TempDir::new().unwrap();
    std::fs::create_dir(temp.path().join("Cargo.toml")).unwrap();

    let surface = RustSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));
    let res = surface.lint(&ctx, false);

    match res.status {
      SurfaceStatus::ExecutionError { message } => {
        assert!(message.contains("No Cargo.toml found"));
      }
      other => panic!(
        "a directory named Cargo.toml must not be treated as a manifest, \
         got {other:?}"
      ),
    }
  }

  #[test]
  fn test_build_clippy_args_with_and_without_fix() {
    let no_fix = build_clippy_args(false, &[]);
    assert_eq!(
      no_fix,
      vec![
        "clippy".to_string(),
        "--all-targets".to_string(),
        "--".to_string(),
        "-D".to_string(),
        "warnings".to_string(),
      ]
    );

    let extra = vec!["--verbose".to_string()];
    let with_fix = build_clippy_args(true, &extra);
    assert_eq!(
      with_fix,
      vec![
        "clippy".to_string(),
        "--fix".to_string(),
        "--allow-no-vcs".to_string(),
        "--allow-dirty".to_string(),
        "--allow-staged".to_string(),
        "--all-targets".to_string(),
        "--".to_string(),
        "-D".to_string(),
        "warnings".to_string(),
        "--verbose".to_string(),
      ]
    );
  }

  #[test]
  fn test_sync_config_generates_edition_2024() {
    let temp = TempDir::new().unwrap();
    let surface = RustSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));

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
    assert!(content.contains("reorder_imports = true"));

    // Check mode should pass when file is up-to-date
    let mut check_ctx = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));
    check_ctx.check_only = true;
    let check_res = surface.sync_config(&check_ctx, true);
    assert!(matches!(check_res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_rustfmt_config_typed_serialization() {
    let cfg = RustfmtConfig {
      tab_spaces: 4,
      max_width: 100,
      newline_style: "Windows".to_string(),
      use_small_heuristics: "Default".to_string(),
      edition: "2021".to_string(),
      reorder_imports: true,
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("tab_spaces = 4"));
    assert!(rendered.contains("max_width = 100"));
    assert!(rendered.contains("newline_style = \"Windows\""));
    assert!(rendered.contains("edition = \"2021\""));
    assert!(rendered.contains("reorder_imports = true"));
  }

  #[test]
  fn test_rustfmt_fallback_command_args() {
    let files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];

    // check_only = false
    let cmd = build_rustfmt_fallback_cmd("2024", "max_width=80", false, &files);
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
      args
        .get(edition_idx.unwrap() + 1)
        .map(std::string::String::as_str),
      Some("2024"),
      "edition value must be 2024"
    );
    let config_idx = args.iter().position(|a| a == "--config");
    assert!(
      config_idx.is_some(),
      "--config flag must be passed to rustfmt"
    );
    assert_eq!(
      args
        .get(config_idx.unwrap() + 1)
        .map(std::string::String::as_str),
      Some("max_width=80")
    );
    assert!(!args.contains(&"--check".to_string()));
    assert!(
      args.contains(&"src/main.rs".to_string())
        || args.contains(&"src\\main.rs".to_string())
    );

    // check_only = true
    let cmd_check =
      build_rustfmt_fallback_cmd("2021", "max_width=80", true, &files);
    let check_args: Vec<String> = cmd_check
      .get_args()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();

    let check_edition_idx = check_args.iter().position(|a| a == "--edition");
    assert!(check_edition_idx.is_some());
    assert_eq!(
      check_args
        .get(check_edition_idx.unwrap() + 1)
        .map(std::string::String::as_str),
      Some("2021")
    );
    assert!(check_args.contains(&"--check".to_string()));
  }

  #[test]
  fn test_build_rustfmt_inline_config_shape() {
    let cfg = RustfmtConfig {
      tab_spaces: 2,
      max_width: 80,
      newline_style: "Unix".to_string(),
      use_small_heuristics: "Default".to_string(),
      edition: "2024".to_string(),
      reorder_imports: true,
    };
    let inline = build_rustfmt_inline_config(&cfg);
    assert_eq!(
      inline,
      "max_width=80,tab_spaces=2,newline_style=Unix,use_small_heuristics=Default,reorder_imports=true"
    );
  }

  #[test]
  fn test_rust_format_does_not_write_rustfmt_toml() {
    // Fixes #151: `fml fmt` must not write `.rustfmt.toml` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !check_binary_exists("rustfmt") && !check_binary_exists("cargo") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main(){let x=1;}\n").unwrap();

    let surface = RustSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));
    let _ = surface.format(&ctx);

    assert!(
      !temp.path().join(".rustfmt.toml").exists(),
      "fml fmt must not write .rustfmt.toml"
    );
  }
  #[test]
  fn test_rust_fallback_edition_without_cargo_toml() {
    let temp = TempDir::new().unwrap();
    // Without Cargo.toml and without explicit edition in config -> defaults to 2021
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));
    let edition = ctx
      .lang_config
      .rust
      .as_ref()
      .and_then(|r| r.edition.as_deref())
      .unwrap_or("2021");
    assert_eq!(edition, "2021");

    // With explicit edition in config -> resolves to configured edition
    let mut ctx_configured =
      test_ctx(temp.path(), ResolvedLangConfig::new("rust"));
    ctx_configured.lang_config.rust = Some(crate::config::RustOptions {
      edition: Some("2018".to_string()),
    });
    let edition_configured = ctx_configured
      .lang_config
      .rust
      .as_ref()
      .and_then(|r| r.edition.as_deref())
      .unwrap_or("2021");
    assert_eq!(edition_configured, "2018");
  }
  #[test]
  fn test_rust_format_reorders_imports() {
    if !check_binary_exists("rustfmt") && !check_binary_exists("cargo") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let file = src.join("main.rs");
    let unformatted = "use std::time::Instant;\nuse std::collections::HashMap;\nuse std::path::Path;\n\nfn main() { let _ = (HashMap::<u32, u32>::new(), Path::new(\"/\"), Instant::now()); }\n";
    std::fs::write(&file, unformatted).unwrap();

    let surface = RustSurface;
    let ctx_fix = test_ctx(temp.path(), ResolvedLangConfig::new("rust"));
    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, SurfaceStatus::Passed));

    let formatted = std::fs::read_to_string(&file).unwrap();
    let hashmap_idx = formatted.find("use std::collections::HashMap;").unwrap();
    let path_idx = formatted.find("use std::path::Path;").unwrap();
    let instant_idx = formatted.find("use std::time::Instant;").unwrap();
    assert!(hashmap_idx < path_idx);
    assert!(path_idx < instant_idx);
  }
}
