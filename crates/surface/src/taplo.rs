//! TOML, formatted by taplo. It has no linter.

use std::path;

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
    let output = crate::run(&TAPLO, &check_args, files, ctx.root, &[0, 1])?;
    let changed = crate::changed(files, ctx.root, unformatted(&output.stderr));
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

/// The files `taplo fmt --check` reports, from its
/// `the file is not properly formatted path="..."` log lines.
fn unformatted(log: &str) -> Vec<path::PathBuf> {
  log
    .lines()
    .filter(|line| line.contains("not properly formatted"))
    .filter_map(|line| {
      let (_, path) = line.split_once("path=\"")?;
      let path = path.strip_suffix('"')?.replace("\\\\", "\\");
      Some(path::PathBuf::from(path))
    })
    .collect()
}

#[cfg(test)]
mod tests {
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
    assert_eq!(check.changed, files);
    assert_eq!(fs::read_to_string(&files[0]).expect("read"), "[t]\na=1\n");

    testing::with_context(dir.path(), |ctx| Toml.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "[t]\na = 1\n");
  }
}
