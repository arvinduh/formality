//! Rust language surface: formats via `rustfmt` and lints via Clippy.
//!
//! Implements `super::LanguageSurface` for Rust, syncing `.rustfmt.toml`.
//! Fleet registration is owned by `super::registry`.

use std::collections;
use std::ffi;
use std::fs;
use std::path;
use std::process;
use std::time;

use crate::config;
use crate::config::facets;
use crate::surfaces;
use crate::surfaces::glob;
use crate::surfaces::sync::native;
use crate::surfaces::tooling;

/// The `.rustfmt.toml` settings `ctx` resolves to.
fn rustfmt_config(ctx: &surfaces::ExecutionContext) -> native::ToolConfig {
  let eol = &ctx.global_config.end_of_line;
  let newline_style = if eol.eq_ignore_ascii_case("crlf") {
    "Windows"
  } else if eol.eq_ignore_ascii_case("cr") {
    "Auto"
  } else {
    "Unix"
  };
  let edition = ctx
    .lang_config
    .rust
    .as_ref()
    .and_then(|r| r.edition.as_deref())
    .unwrap_or("2024");
  native::ToolConfig::new(".rustfmt.toml")
    .set("tab_spaces", native::int(ctx.lang_config.indent_size))
    .set("max_width", native::int(ctx.lang_config.line_length))
    .set("newline_style", newline_style)
    .set("use_small_heuristics", "Default")
    .set("edition", edition)
    .set("reorder_imports", true)
}

/// The settings rustfmt takes inline through `--config k=v,...`; the edition
/// goes through its own `--edition` flag.
const RUSTFMT_INLINE_KEYS: &[&str] = &[
  "max_width",
  "tab_spaces",
  "newline_style",
  "use_small_heuristics",
  "reorder_imports",
];

/// Rust language surface implementation.
#[derive(Debug, Default)]
pub struct RustSurface;

impl facets::DeclaresFacets for RustSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs => facets::FacetSupport::Fixed("spaces"),
      facets::Facet::IndentWidth
      | facets::Facet::LineLength
      | facets::Facet::ImportSort
      | facets::Facet::Edition => facets::FacetSupport::Configurable,
      facets::Facet::QuoteStyle
      | facets::Facet::TrailingComma
      | facets::Facet::ProseWrap
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

/// Builds argument vector for cargo clippy invocation.
#[must_use]
fn build_clippy_args(fix: bool, extra_args: &[String]) -> Vec<String> {
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
/// used by the LSP server (`fml lsp`, Fixes #159 [pre-recreation]) to translate individual
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

/// Drops the files rustfmt already reaches through another listed file.
///
/// rustfmt formats each argument and then every out-of-line `mod name;`
/// beneath it, so handing it a whole tree formats a file once per listed
/// ancestor. A file is dropped only when its conventional parent (`mod.rs`,
/// `lib.rs`, `main.rs` or `<dir>.rs`) is listed and declares it at top level
/// without a `#[path]`; anything unusual stays listed, which costs only a
/// repeat.
fn module_roots(files: &[path::PathBuf]) -> Vec<&path::PathBuf> {
  let listed: collections::HashSet<&path::Path> =
    files.iter().map(path::PathBuf::as_path).collect();
  let mut sources = collections::HashMap::new();
  files
    .iter()
    .filter(|file| !declared_by_listed_parent(file, &listed, &mut sources))
    .collect()
}

/// Returns whether a listed parent module of `file` declares it; `sources`
/// caches each parent's contents across calls.
fn declared_by_listed_parent(
  file: &path::Path,
  listed: &collections::HashSet<&path::Path>,
  sources: &mut collections::HashMap<path::PathBuf, Option<String>>,
) -> bool {
  let Some((name, dir)) = module_name_and_dir(file) else {
    return false;
  };
  let mut parents: Vec<path::PathBuf> = ["mod.rs", "lib.rs", "main.rs"]
    .iter()
    .map(|parent| dir.join(parent))
    .collect();
  if let (Some(up), Some(dir_name)) = (dir.parent(), dir.file_name()) {
    let mut sibling = dir_name.to_os_string();
    sibling.push(".rs");
    parents.push(up.join(sibling));
  }
  parents.into_iter().any(|parent| {
    listed.contains(parent.as_path())
      && sources
        .entry(parent)
        .or_insert_with_key(|parent| fs::read_to_string(parent).ok())
        .as_deref()
        .is_some_and(|source| declares_module(source, name))
  })
}

/// The module name `file` holds and the directory whose parent module would
/// declare it; `None` for crate roots (`lib.rs`, `main.rs`) and odd names.
fn module_name_and_dir(file: &path::Path) -> Option<(&str, &path::Path)> {
  let dir = file.parent()?;
  match file.file_stem().and_then(ffi::OsStr::to_str)? {
    "lib" | "main" => None,
    "mod" => Some((dir.file_name()?.to_str()?, dir.parent()?)),
    stem => Some((stem, dir)),
  }
}

/// Returns whether `source` declares `mod name;` at top level, unindented
/// and not redirected by a `#[path]` attribute just above it.
fn declares_module(source: &str, name: &str) -> bool {
  let mut redirected = false;
  for line in source.lines().map(str::trim_end) {
    if line.starts_with("#[") {
      redirected |= line.contains("path");
      continue;
    }
    let decl = line
      .strip_prefix("pub ")
      .or_else(|| {
        line
          .strip_prefix("pub(")
          .and_then(|rest| rest.split_once(") ").map(|(_, decl)| decl))
      })
      .unwrap_or(line);
    let declared = decl
      .strip_prefix("mod ")
      .and_then(|rest| rest.strip_suffix(';'))
      .is_some_and(|declared| declared.trim() == name);
    if declared && !redirected {
      return true;
    }
    redirected = false;
  }
  false
}

fn build_rustfmt_fallback_cmd(
  edition: &str,
  inline_config: &str,
  check_only: bool,
  files: &[&path::PathBuf],
) -> process::Command {
  let mut c = tooling::create_tool_command("rustfmt");
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

/// Source file extensions owned by the Rust surface.
const RUST_EXTENSIONS: &[&str] = &["rs"];

impl surfaces::LanguageSurface for RustSurface {
  fn name(&self) -> &'static str {
    "rust"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["rustfmt", "clippy-driver"]
  }

  fn aliases(&self) -> &[&'static str] {
    &["rs"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    RUST_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(Self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn marker_files(&self) -> &[&'static str] {
    &["Cargo.toml"]
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![
      surfaces::ToolInfo {
        binary: "cargo",
        description: "Rust package manager & build tool",
        is_required_for_fmt: true,
        is_required_for_lint: true,
      },
      surfaces::ToolInfo {
        binary: "rustfmt",
        description: "Rust code formatter",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      surfaces::ToolInfo {
        binary: "clippy-driver",
        description: "Rust linter (cargo clippy)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  // Dispatches rustfmt formatting with Cargo.toml discovery, check vs write modes, and error parsing.
  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if !tooling::check_binary_exists("cargo")
      && !tooling::check_binary_exists("rustfmt")
    {
      return tooling::tool_missing_result(
        self.name(),
        start,
        "cargo / rustfmt",
        &tooling::install_hint_for("rustfmt"),
      );
    }

    let files = ctx.matched_files(RUST_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    let edition =
      if let Ok(manifest) = fs::read_to_string(ctx.root.join("Cargo.toml")) {
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
    // disk — see `RUSTFMT_INLINE_KEYS` (Fixes #151 [pre-recreation]). `fml sync`
    // remains the only path that materializes the file, for users who want
    // it on disk (e.g. for editor integrations that don't go through `fml`).
    let inline_config = rustfmt_config(ctx)
      .pairs(RUSTFMT_INLINE_KEYS, native::Quote::None)
      .join(",");

    let mut cmd = if tooling::check_binary_exists("cargo")
      && glob::find_manifest_upwards(&ctx.root, "Cargo.toml")
    {
      let mut c = tooling::create_tool_command("cargo");
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
        for f in module_roots(&files) {
          c.arg(f);
        }
      }
      c
    } else {
      build_rustfmt_fallback_cmd(
        edition,
        &inline_config,
        ctx.check_only,
        &module_roots(&files),
      )
    };

    cmd.args(ctx.lang_config.tool_args("rustfmt"));
    cmd.current_dir(ctx.root.as_path());

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) = tooling::tool_missing_guard(self.name(), "cargo", start)
    {
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
    if !glob::find_manifest_upwards(&ctx.root, "Cargo.toml") {
      return surfaces::SurfaceResult::error(
        self.name(),
        start,
        format!(
          "No Cargo.toml found in {} (or any parent directory). `cargo \
             clippy` needs a Cargo manifest to lint against — run `cargo \
             init` here, or point --root at the crate/workspace root.",
          ctx.root.display()
        ),
      );
    }

    let mut cmd = ctx.command("cargo");
    cmd.args(build_clippy_args(
      fix,
      ctx.lang_config.tool_args("clippy-driver"),
    ));

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #151 [pre-recreation]): they
  // pass the resolved config to rustfmt inline via `--config` (see
  // `RUSTFMT_INLINE_KEYS`, used in `format()` above). This method is
  // now reached only by `fml sync`, for users who explicitly want
  // `.rustfmt.toml` materialized on disk (e.g. for editor/rust-analyzer
  // integration outside of `fml`).
  fn sync_config(
    &self,
    ctx: &surfaces::ExecutionContext,
    check: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();
    rustfmt_config(ctx).sync(ctx, check, start, self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::surfaces;
  use crate::surfaces::LanguageSurface;

  #[test]
  fn lint_without_cargo_toml_is_execution_error_not_violation() {
    let temp = tempfile::TempDir::new().unwrap();
    // No Cargo.toml written — mirrors a bare `.rs` file with no crate manifest.
    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));

    let res = surface.lint(&ctx, false);
    match res.status {
      surfaces::SurfaceStatus::ExecutionError { message } => {
        assert!(message.contains("Cargo.toml"));
      }
      other => {
        panic!("expected ExecutionError for missing Cargo.toml, got {other:?}")
      }
    }
  }

  #[test]
  fn lint_cargo_toml_in_ancestor_directory_is_not_guarded() {
    // A subdirectory of a real crate (Cargo.toml lives above `ctx.root`,
    // mirroring a nested workspace-member layout, e.g. `src/deep`) must not
    // trip the preflight guard (Fixes #185) — `cargo clippy` itself
    // resolves a manifest by walking upward from the working directory
    // exactly the same way, so the guard must mirror that instead of
    // checking only `ctx.root`.
    if !tooling::check_binary_exists("cargo") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    fs::write(
      temp.path().join("Cargo.toml"),
      "[package]\nname = \"testcrate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let nested = temp.path().join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();
    fs::write(temp.path().join("src").join("lib.rs"), "pub mod deep;\n")
      .unwrap();
    fs::write(nested.join("mod.rs"), "pub fn f() {}\n").unwrap();

    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(&nested, config::ResolvedLangConfig::new("rust"));
    let res = surface.lint(&ctx, false);

    if let surfaces::SurfaceStatus::ExecutionError { message } = &res.status {
      assert!(
        !message.contains("No Cargo.toml found"),
        "Cargo.toml in an ancestor directory must not trip the \
         missing-manifest guard, got: {message}"
      );
    }
  }

  #[test]
  fn lint_directory_named_cargo_toml_is_not_treated_as_manifest() {
    // `.is_file()`, not `.exists()` (Fixes #185): a directory that happens
    // to be named `Cargo.toml` must not be mistaken for the manifest.
    let temp = tempfile::TempDir::new().unwrap();
    fs::create_dir(temp.path().join("Cargo.toml")).unwrap();

    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    let res = surface.lint(&ctx, false);

    match res.status {
      surfaces::SurfaceStatus::ExecutionError { message } => {
        assert!(message.contains("No Cargo.toml found"));
      }
      other => panic!(
        "a directory named Cargo.toml must not be treated as a manifest, \
         got {other:?}"
      ),
    }
  }

  #[test]
  fn format_directory_named_cargo_toml_is_not_treated_as_manifest() {
    // `.is_file()`, not `.exists()` (Fixes #204): a directory that happens
    // to be named `Cargo.toml` must not be mistaken for the manifest and
    // cause `cargo fmt` to be chosen over bare `rustfmt`.
    if !tooling::check_binary_exists("rustfmt")
      && !tooling::check_binary_exists("cargo")
    {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    fs::create_dir(temp.path().join("Cargo.toml")).unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("main.rs");
    fs::write(&file, "fn main() {}\n").unwrap();

    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    let res = surface.format(&ctx);

    assert!(
      !matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "directory named Cargo.toml must not trigger cargo fmt failure, got: {:?}",
      res.status
    );
  }

  #[test]
  fn format_cargo_toml_in_ancestor_directory_finds_manifest() {
    // A subdirectory of a real crate (Cargo.toml in ancestor) must find
    // the manifest via `find_manifest_upwards` rather than taking the
    // bare-rustfmt fallback (Fixes #204).
    if !tooling::check_binary_exists("cargo") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    fs::write(
      temp.path().join("Cargo.toml"),
      "[package]\nname = \"test_format_ancestor_crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let nested = temp.path().join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("lib.rs");
    fs::write(&file, "pub fn foo() {}\n").unwrap();

    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(&nested, config::ResolvedLangConfig::new("rust"));
    let res = surface.format(&ctx);

    assert!(
      !matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "ancestor Cargo.toml must be detected for cargo fmt, got: {:?}",
      res.status
    );
  }

  #[test]
  fn build_clippy_args_with_and_without_fix() {
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
  fn sync_config_generates_edition_2024() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));

    let res = surface.sync_config(&ctx, false);
    assert_eq!(res.status.created_file_names(), [".rustfmt.toml"]);

    let config_path = temp.path().join(".rustfmt.toml");
    assert!(config_path.is_file());

    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("edition = \"2024\""));
    assert!(content.contains("tab_spaces = 2"));
    assert!(content.contains("max_width = 80"));
    assert!(content.contains("newline_style = \"Unix\""));
    assert!(content.contains("reorder_imports = true"));

    // Check mode should pass when file is up-to-date
    let mut check_ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    check_ctx.check_only = true;
    let check_res = surface.sync_config(&check_ctx, true);
    assert!(matches!(check_res.status, surfaces::SurfaceStatus::Passed));
  }

  /// `fml sync`'s file and `fml fmt`'s inline `--config` come from the same
  /// settings, so they agree on every value, including the CRLF mapping.
  #[test]
  fn rustfmt_config_file_and_inline_agree() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut lang = config::ResolvedLangConfig::new("rust");
    lang.indent_size = 2;
    lang.line_length = 80;
    let mut ctx = surfaces::test_ctx(temp.path(), lang);
    ctx.global_config = std::sync::Arc::new(config::ResolvedGlobalConfig {
      end_of_line: "crlf".to_string(),
      ..Default::default()
    });

    let cfg = rustfmt_config(&ctx);
    let file = cfg.render();
    for line in [
      "tab_spaces = 2",
      "max_width = 80",
      "newline_style = \"Windows\"",
      "edition = \"2024\"",
    ] {
      assert!(file.contains(line), "{line} missing from:\n{file}");
    }
    assert_eq!(
      cfg
        .pairs(RUSTFMT_INLINE_KEYS, native::Quote::None)
        .join(","),
      "max_width=80,tab_spaces=2,newline_style=Windows,\
       use_small_heuristics=Default,reorder_imports=true"
    );
  }

  #[test]
  fn rustfmt_fallback_command_args() {
    let main = path::PathBuf::from("src/main.rs");
    let lib = path::PathBuf::from("src/lib.rs");
    let files = [&main, &lib];

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
      args.get(edition_idx.unwrap() + 1).map(String::as_str),
      Some("2024"),
      "edition value must be 2024"
    );
    let config_idx = args.iter().position(|a| a == "--config");
    assert!(
      config_idx.is_some(),
      "--config flag must be passed to rustfmt"
    );
    assert_eq!(
      args.get(config_idx.unwrap() + 1).map(String::as_str),
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
        .map(String::as_str),
      Some("2021")
    );
    assert!(check_args.contains(&"--check".to_string()));
  }

  #[test]
  fn rust_format_does_not_write_rustfmt_toml() {
    // Fixes #151 [pre-recreation]: `fml fmt` must not write `.rustfmt.toml` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("rustfmt")
      && !tooling::check_binary_exists("cargo")
    {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main(){let x=1;}\n").unwrap();

    let surface = RustSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    let _ = surface.format(&ctx);

    assert!(
      !temp.path().join(".rustfmt.toml").exists(),
      "fml fmt must not write .rustfmt.toml"
    );
  }

  #[test]
  fn rust_fallback_edition_without_cargo_toml() {
    let temp = tempfile::TempDir::new().unwrap();
    // Without Cargo.toml and without explicit edition in config -> defaults to 2021
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    let edition = ctx
      .lang_config
      .rust
      .as_ref()
      .and_then(|r| r.edition.as_deref())
      .unwrap_or("2021");
    assert_eq!(edition, "2021");

    // With explicit edition in config -> resolves to configured edition
    let mut ctx_configured =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    ctx_configured.lang_config.rust = Some(config::options::RustOptions {
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
  fn rust_format_reorders_imports() {
    if !tooling::check_binary_exists("rustfmt")
      && !tooling::check_binary_exists("cargo")
    {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("main.rs");
    let unformatted = "use std::time::Instant;\nuse std::collections::HashMap;\nuse std::path::Path;\n\nfn main() { let _ = (HashMap::<u32, u32>::new(), Path::new(\"/\"), Instant::now()); }\n";
    fs::write(&file, unformatted).unwrap();

    let surface = RustSurface;
    let ctx_fix =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("rust"));
    let fix_res = surface.format(&ctx_fix);
    assert!(matches!(fix_res.status, surfaces::SurfaceStatus::Passed));

    let formatted = fs::read_to_string(&file).unwrap();
    let hashmap_idx = formatted.find("use std::collections::HashMap;").unwrap();
    let path_idx = formatted.find("use std::path::Path;").unwrap();
    let instant_idx = formatted.find("use std::time::Instant;").unwrap();
    assert!(hashmap_idx < path_idx);
    assert!(path_idx < instant_idx);
  }

  #[test]
  fn declares_module_matches_top_level_declarations_only() {
    let source = "mod a;\npub mod b;\npub(crate) mod c;\n\
      #[path = \"x.rs\"]\nmod d;\n#[cfg(test)]\nmod e;\n\
      mod f {\n  mod g;\n}\n// mod h;\nmod i; // note\n";
    for name in ["a", "b", "c", "e"] {
      assert!(declares_module(source, name), "{name} is declared");
    }
    for name in ["d", "g", "h", "i", "z"] {
      assert!(!declares_module(source, name), "{name} is not reached");
    }
  }

  #[test]
  fn module_roots_drops_files_a_listed_parent_declares() {
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(src.join("engine/doctor")).unwrap();
    let write = |rel: &str, body: &str| {
      let file = src.join(rel);
      fs::write(&file, body).unwrap();
      file
    };
    let lib = write("lib.rs", "pub mod engine;\npub mod util;\n");
    let engine = write("engine.rs", "pub mod doctor;\n");
    let doctor = write("engine/doctor/mod.rs", "mod venv;\n");
    let venv = write("engine/doctor/venv.rs", "");
    let util = write("util.rs", "");
    let orphan = write("orphan.rs", "");
    let unlisted_parent = write("engine/other.rs", "");

    let files = vec![
      lib.clone(),
      engine,
      doctor,
      venv,
      util,
      orphan.clone(),
      unlisted_parent.clone(),
    ];
    assert_eq!(module_roots(&files), [&lib, &orphan, &unlisted_parent]);

    let partial = vec![files[2].clone(), files[3].clone()];
    assert_eq!(module_roots(&partial), [&partial[0]]);
  }
}
