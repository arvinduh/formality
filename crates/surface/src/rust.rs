//! Rust, formatted by rustfmt and linted by clippy.
//!
//! Owns both tools' arguments and output. Clippy checks a whole Cargo
//! workspace at once, so its findings are filtered to the files asked about.

use std::collections;
use std::ffi;
use std::fs;
use std::path;

use config;
use serde_json;
use toml;
use tool;

/// The rustfmt binary.
const RUSTFMT: tool::Tool = tool::Tool {
  binary: "rustfmt",
  install: "rustup component add rustfmt",
};

/// The cargo binary, which runs clippy.
const CARGO: tool::Tool = tool::Tool {
  binary: "cargo",
  install: "see https://rustup.rs",
};

/// The edition rustfmt parses with when no `Cargo.toml` names one.
const EDITION: &str = "2024";

/// The rust-guide's lint set, as clippy flags, for a crate whose manifest
/// has no `[lints]` table. The group comes first so the lints after it set
/// their own level, and every lint warns so one run reports all of them.
const LINTS: &[&str] = &[
  "-Wclippy::pedantic",
  "-Wmissing_docs",
  "-Wunsafe_op_in_unsafe_fn",
  "-Wunused_qualifications",
  "-Wclippy::wildcard_imports",
  "-Aclippy::single_component_path_imports",
  "-Wclippy::dbg_macro",
  "-Wclippy::todo",
  "-Wclippy::undocumented_unsafe_blocks",
  "-Wclippy::missing_docs_in_private_items",
  "-Wclippy::unwrap_used",
  "-Wclippy::missing_assert_message",
  "-Wclippy::exit",
  "-Wclippy::allow_attributes",
  "-Wclippy::allow_attributes_without_reason",
  "-Wclippy::redundant_clone",
];

/// Rust: `.rs` files.
pub struct Rust;

impl crate::Surface for Rust {
  fn name(&self) -> &'static str {
    "rust"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["rs"]
  }

  fn format(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<crate::Report, crate::Error> {
    let mut args = vec!["--edition".to_owned(), edition(ctx.root)];
    if let Some(settings) = ctx.settings(self.name(), native(ctx))? {
      args.extend(["--config".to_owned(), settings_arg(&settings)]);
    }
    let mut check_args = args.clone();
    check_args.extend(["--check".to_owned(), "--files-with-diff".to_owned()]);
    let output = rustfmt(ctx, &check_args, files)?;
    let reported = output.lines().filter(|line| !line.is_empty());
    let changed = crate::changed(files, ctx.root, reported.map(Into::into));
    if !check && !changed.is_empty() {
      rustfmt(ctx, &args, &changed)?;
    }
    Ok(crate::Report {
      changed,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    fix: bool,
  ) -> Result<crate::Report, crate::Error> {
    let Some(manifest) = tool::nearest_config(ctx.root, &["Cargo.toml"]) else {
      return Ok(crate::Report::default());
    };
    let dir = manifest.parent().unwrap_or(ctx.root);
    let mut args = vec![
      "clippy",
      "--quiet",
      "--all-targets",
      "--message-format=json",
    ];
    if fix {
      args.extend(["--fix", "--allow-dirty", "--allow-staged"]);
    }
    if !has_lints(&manifest) {
      args.push("--");
      args.extend(LINTS);
    }
    let args: Vec<&ffi::OsStr> =
      args.into_iter().map(ffi::OsStr::new).collect();
    let output = CARGO.run(&args, dir)?;
    let problems = diagnostics(&output.stdout, dir);
    let problems: Vec<crate::Problem> = crate::locate(problems, files)
      .into_iter()
      .filter(|problem| files.contains(&problem.path))
      .collect();
    if output.code != Some(0) && problems.is_empty() {
      return Err(crate::Error::Failed {
        binary: CARGO.binary,
        code: output.code,
        stderr: output.stderr,
      });
    }
    Ok(crate::Report {
      changed: Vec::new(),
      problems,
    })
  }
}

/// Reads the lints and errors in cargo's JSON messages, resolving paths
/// against `cwd`. A site that several targets report, such as a library
/// built once plain and once for its tests, is kept once.
fn diagnostics(output: &str, cwd: &path::Path) -> Vec<crate::Problem> {
  let mut seen = collections::HashSet::new();
  output
    .lines()
    .filter_map(|line| diagnostic(line, cwd))
    .filter(|problem| {
      let key = (problem.path.clone(), problem.line, problem.column);
      seen.insert((key, problem.rule.clone()))
    })
    .collect()
}

/// Reads one cargo JSON message, or `None` when it is not a diagnostic with
/// a place, such as a build-finished line or a count of warnings. Some
/// compile errors have no code, so the rule is optional.
fn diagnostic(line: &str, cwd: &path::Path) -> Option<crate::Problem> {
  let message = line.parse::<serde_json::Value>().ok()?;
  if message["reason"] != "compiler-message" {
    return None;
  }
  let diagnostic = &message["message"];
  let rule = diagnostic["code"]["code"].as_str();
  let spans = diagnostic["spans"].as_array()?;
  let span = spans
    .iter()
    .find(|span| span["is_primary"] == true)
    .or_else(|| spans.first())?;
  let number = |key: &str| u32::try_from(span[key].as_u64()?).ok();
  let level = diagnostic["level"].as_str()?;
  let text = diagnostic["message"].as_str()?;
  let message = match rule {
    Some(rule) => format!("{level}: {text} [{rule}]"),
    None => format!("{level}: {text}"),
  };
  Some(crate::Problem {
    path: cwd.join(span["file_name"].as_str()?),
    line: number("line_start")?,
    column: number("column_start")?,
    message,
    rule: rule.map(str::to_owned),
  })
}

/// Whether rustfmt will find a config of its own: a `rustfmt.toml` or
/// `.rustfmt.toml` at or above the root, or a user-level
/// `rustfmt/rustfmt.toml`.
fn native(ctx: &crate::Context) -> bool {
  let names = ["rustfmt.toml", ".rustfmt.toml"];
  tool::nearest_config(ctx.root, &names).is_some()
    || ctx
      .user_config
      .is_some_and(|dir| dir.join("rustfmt").join("rustfmt.toml").is_file())
}

/// The rustfmt `--config` value that carries `settings`.
fn settings_arg(settings: &config::Global) -> String {
  let newline = match settings.end_of_line {
    config::EndOfLine::Lf => "Unix",
    config::EndOfLine::Crlf => "Windows",
  };
  format!(
    "tab_spaces={},max_width={},newline_style={newline}",
    settings.indent_size, settings.line_length
  )
}

/// The edition of the nearest `Cargo.toml` at or above `root`, from its
/// `[package]` or `[workspace.package]` table, so a lone file is not parsed
/// as edition 2015.
fn edition(root: &path::Path) -> String {
  let manifest = tool::nearest_config(root, &["Cargo.toml"])
    .and_then(|path| fs::read_to_string(path).ok())
    .and_then(|text| text.parse::<toml::Table>().ok());
  let edition = manifest.as_ref().and_then(|manifest| {
    let package = manifest.get("package");
    let workspace = manifest.get("workspace").and_then(|w| w.get("package"));
    [package, workspace]
      .into_iter()
      .flatten()
      .find_map(|table| table.get("edition")?.as_str())
  });
  edition.unwrap_or(EDITION).to_owned()
}

/// Whether the manifest at `path` has a `[lints]` or `[workspace.lints]`
/// table, which clippy reads on its own. One that cannot be read counts as
/// having one, so fml adds nothing to a manifest cargo will reject anyway.
fn has_lints(path: &path::Path) -> bool {
  let Some(manifest) = fs::read_to_string(path)
    .ok()
    .and_then(|text| text.parse::<toml::Table>().ok())
  else {
    return true;
  };
  manifest.contains_key("lints")
    || manifest
      .get("workspace")
      .is_some_and(|workspace| workspace.get("lints").is_some())
}

/// Runs rustfmt with `args` over `files` in the root and returns its stdout.
///
/// Exit code 1 under `--check` means some file would change.
fn rustfmt(
  ctx: &crate::Context,
  args: &[String],
  files: &[path::PathBuf],
) -> Result<String, crate::Error> {
  Ok(crate::run(&RUSTFMT, args, files, ctx.root, &[0, 1])?.stdout)
}

#[cfg(test)]
mod tests {
  use tempfile;

  use super::*;
  use crate::Surface;

  /// Writes `files` (relative path and text) into a new temp dir.
  fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
      let path = dir.path().join(name);
      fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
      fs::write(path, text).expect("write");
    }
    dir
  }

  /// Runs `pass` against a context rooted at `root` with no fml config.
  fn with_context<T>(
    root: &path::Path,
    pass: impl Fn(&crate::Context) -> T,
  ) -> T {
    let config = config::Config::new(Vec::new()).expect("config");
    pass(&crate::Context {
      root,
      config: &config,
      project: false,
      user_config: None,
    })
  }

  #[test]
  fn format_uses_two_space_indent_by_default() {
    let dir = workspace(&[("a.rs", "fn main() {\nlet x = 1;\n}\n")]);
    let files = [dir.path().join("a.rs")];
    let check = with_context(dir.path(), |ctx| Rust.format(ctx, &files, true))
      .expect("check");
    assert_eq!(check.changed, files);

    with_context(dir.path(), |ctx| Rust.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "fn main() {\n  let x = 1;\n}\n");
  }

  #[test]
  fn format_leaves_settings_to_a_rustfmt_toml() {
    let source = "fn main() {\n    let x = 1;\n}\n";
    let dir = workspace(&[("a.rs", source), ("rustfmt.toml", "")]);
    let files = [dir.path().join("a.rs")];
    let check = with_context(dir.path(), |ctx| Rust.format(ctx, &files, true))
      .expect("check");
    assert!(check.changed.is_empty(), "rustfmt's own 4-space default");
  }

  #[test]
  fn edition_comes_from_the_workspace_package() {
    let manifest = "[workspace.package]\nedition = \"2021\"\n";
    let dir = workspace(&[("Cargo.toml", manifest)]);
    assert_eq!(edition(dir.path()), "2021");
  }

  #[test]
  fn lint_uses_the_guide_lints_only_without_a_lints_table() {
    let manifest = "[package]\nname = \"t\"\nedition = \"2024\"\n";
    let dir = workspace(&[
      ("Cargo.toml", manifest),
      (
        "src/main.rs",
        "//! A.\n\nfn main() {\n  f();\n}\n\nfn f() {}\n",
      ),
    ]);
    let files = [dir.path().join("src/main.rs")];
    let report = with_context(dir.path(), |ctx| Rust.lint(ctx, &files, false))
      .expect("lint");
    let rules: Vec<_> = report.problems.iter().map(|p| &p.rule).collect();
    let want = Some("clippy::missing_docs_in_private_items".to_owned());
    assert_eq!(rules, [&want], "f has no doc comment");
  }

  #[test]
  fn lint_reports_clippy_findings_in_the_given_files() {
    let manifest =
      "[package]\nname = \"t\"\nedition = \"2024\"\n[lints.rust]\n";
    let dir = workspace(&[
      ("Cargo.toml", manifest),
      ("src/main.rs", "fn main() {\n  let x = 1;\n}\n"),
    ]);
    let files = [dir.path().join("src/main.rs")];
    let report = with_context(dir.path(), |ctx| Rust.lint(ctx, &files, false))
      .expect("lint");
    assert_eq!(report.problems.len(), 1, "the unused variable");
    assert_eq!(report.problems[0].line, 2);
    assert_eq!(report.problems[0].rule.as_deref(), Some("unused_variables"));
  }
}
