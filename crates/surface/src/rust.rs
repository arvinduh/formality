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
    if let Some(settings) = ctx.settings(native(ctx))? {
      args.extend(["--config".to_owned(), settings_arg(&settings)]);
    }
    let mut check_args = args.clone();
    check_args.extend(["--check".to_owned(), "--files-with-diff".to_owned()]);
    let output = rustfmt(ctx, &check_args, files)?;
    let changed: Vec<path::PathBuf> = output
      .lines()
      .filter(|line| !line.is_empty())
      .map(|line| ctx.root.join(line))
      .collect();
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
    let problems: Vec<crate::Problem> = crate::problems(&output.stderr, dir)
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
  let args: Vec<&ffi::OsStr> = args
    .iter()
    .map(ffi::OsStr::new)
    .chain(files.iter().map(|file| file.as_os_str()))
    .collect();
  let output = RUSTFMT.run(&args, ctx.root)?;
  match output.code {
    Some(0 | 1) => Ok(output.stdout),
    code => Err(Error::Failed {
      binary: RUSTFMT.binary,
      code,
      stderr: output.stderr,
    }),
  }
}
