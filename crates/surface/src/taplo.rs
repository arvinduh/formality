//! TOML, formatted by taplo. It has no linter.

use std::path;
use std::slice;

use config;
use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// The taplo binary.
const TAPLO: tool::Tool = tool::Tool {
  binary: "taplo",
  install: "npm install -g @taplo/cli",
};

/// TOML: `.toml` files.
pub struct Toml;

impl Surface for Toml {
  fn name(&self) -> &'static str {
    "toml"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["toml"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    let native =
      tool::nearest_config(ctx.root, &[".taplo.toml", "taplo.toml"]).is_some();
    let mut args = vec!["fmt".to_owned()];
    args.extend(settings(ctx.settings(self.name(), native)?.as_ref()));
    let mut check_args = args.clone();
    check_args.push("--check".to_owned());
    // taplo names unformatted files only in its logs, whose shape differs
    // across platforms, so each file is checked on its own by exit code.
    let mut changed = Vec::new();
    for file in files {
      let file = slice::from_ref(file);
      let output = crate::run(&TAPLO, &check_args, file, ctx.root, &[0, 1])?;
      if output.code == Some(1) {
        changed.extend_from_slice(file);
      }
    }
    if !check && !changed.is_empty() {
      crate::run(&TAPLO, &args, &changed, ctx.root, &[0])?;
    }
    Ok(Report {
      changed,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    _ctx: &Context,
    _files: &[path::PathBuf],
    _fix: bool,
  ) -> Result<Report, Error> {
    Ok(Report::default())
  }
}

/// The taplo `--option` arguments that carry `settings`, or none to let taplo
/// read its own config.
fn settings(settings: Option<&config::Global>) -> Vec<String> {
  let Some(settings) = settings else {
    return Vec::new();
  };
  let indent = " ".repeat(usize::from(settings.indent_size));
  let crlf = matches!(settings.end_of_line, config::EndOfLine::Crlf);
  vec![
    format!("--option=indent_string={indent}"),
    format!("--option=column_width={}", settings.line_length),
    format!("--option=crlf={crlf}"),
  ]
}

#[cfg(test)]
mod tests {
  use std::ffi;
  use std::fs;

  use super::*;
  use crate::testing;

  #[test]
  fn format_check_reports_without_writing_and_format_writes() {
    let dir = testing::workspace(&[("a.toml", "[t]\na=1\n")]);
    let files = [dir.path().join("a.toml")];
    let check = testing::with_context(dir.path(), |ctx| {
      Toml.format(ctx, &files, true).expect("check")
    });
    let args = ["fmt", "--check", "a.toml"].map(ffi::OsStr::new);
    let raw = TAPLO.run(&args, dir.path()).expect("taplo runs");
    assert_eq!(check.changed, files, "taplo said: {}", raw.stderr);
    assert_eq!(fs::read_to_string(&files[0]).expect("read"), "[t]\na=1\n");

    testing::with_context(dir.path(), |ctx| Toml.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "[t]\na = 1\n");
  }
}
