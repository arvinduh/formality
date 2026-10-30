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
    args.extend(settings(ctx.settings(native)?.as_ref()));
    let mut check_args = args.clone();
    check_args.push("--check".to_owned());
    let output = crate::run(&TAPLO, &check_args, files, ctx.root, &[0, 1])?;
    let changed = unformatted(&output.stderr);
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
      Some(path::PathBuf::from(path.strip_suffix('"')?))
    })
    .collect()
}
