//! Rust, formatted by rustfmt and linted by clippy.
//!
//! Owns both tools' arguments and output. Clippy checks a whole Cargo
//! workspace at once, so its findings are filtered to the files asked about.

use std::ffi;
use std::fs;
use std::path;

use config;
use toml;
use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

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

/// Rust: `.rs` files.
pub struct Rust;

impl Surface for Rust {
  fn name(&self) -> &'static str {
    "rust"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["rs"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
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
    Ok(Report {
      changed,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    fix: bool,
  ) -> Result<Report, Error> {
    let Some(manifest) = tool::nearest_config(ctx.root, &["Cargo.toml"]) else {
      return Ok(Report::default());
    };
    let dir = manifest.parent().unwrap_or(ctx.root);
    let mut args = vec!["clippy", "--quiet", "--message-format=short"];
    if fix {
      args.extend(["--fix", "--allow-dirty", "--allow-staged"]);
    }
    let args: Vec<&ffi::OsStr> =
      args.into_iter().map(ffi::OsStr::new).collect();
    let output = CARGO.run(&args, dir)?;
    let problems = crate::problems(&output.stderr, dir);
    let problems: Vec<crate::Problem> = crate::locate(problems, files)
      .into_iter()
      .filter(|problem| files.contains(&problem.path))
      .collect();
    if output.code != Some(0) && problems.is_empty() {
      return Err(Error::Failed {
        binary: CARGO.binary,
        code: output.code,
        stderr: output.stderr,
      });
    }
    Ok(Report {
      changed: Vec::new(),
      problems,
    })
  }
}

/// Whether rustfmt will find a config of its own: a `rustfmt.toml` or
/// `.rustfmt.toml` at or above the root, or a user-level
/// `rustfmt/rustfmt.toml`.
fn native(ctx: &Context) -> bool {
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

/// Runs rustfmt with `args` over `files` in the root and returns its stdout.
///
/// Exit code 1 under `--check` means some file would change.
fn rustfmt(
  ctx: &Context,
  args: &[String],
  files: &[path::PathBuf],
) -> Result<String, Error> {
  Ok(crate::run(&RUSTFMT, args, files, ctx.root, &[0, 1])?.stdout)
}

#[cfg(test)]
mod tests {
  use tempfile;

  use super::*;

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
  fn with_context<T>(root: &path::Path, pass: impl Fn(&Context) -> T) -> T {
    let config = config::Config::new(Vec::new()).expect("config");
    pass(&Context {
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
  fn lint_reports_clippy_findings_in_the_given_files() {
    let manifest = "[package]\nname = \"t\"\nedition = \"2024\"\n";
    let dir = workspace(&[
      ("Cargo.toml", manifest),
      ("src/main.rs", "fn main() {\n  let x = 1;\n}\n"),
    ]);
    let files = [dir.path().join("src/main.rs")];
    let report = with_context(dir.path(), |ctx| Rust.lint(ctx, &files, false))
      .expect("lint");
    assert_eq!(report.problems.len(), 1, "the unused variable");
    assert_eq!(report.problems[0].line, 2);
  }
}
