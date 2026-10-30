//! Prettier, the formatter Markdown, YAML and JSON share.
//!
//! Owns prettier's arguments and its list of changed files. The languages that
//! use it own their linters.

use std::path;

use config;
use tool;

use crate::Context;
use crate::Error;

/// The prettier binary.
pub const PRETTIER: tool::Tool = tool::Tool {
  binary: "prettier",
  install: "npm install -g prettier",
};

/// The config files prettier reads on its own.
const CONFIGS: &[&str] = &[
  ".prettierrc",
  ".prettierrc.json",
  ".prettierrc.yaml",
  ".prettierrc.yml",
  ".prettierrc.toml",
  ".prettierrc.js",
  "prettier.config.js",
];

/// Formats `language`'s `files` with prettier, or with `check` only lists
/// the files that would change, and returns those files.
///
/// # Errors
///
/// Returns an error when prettier is missing or fails, or the config is wrong.
pub fn format(
  language: &str,
  ctx: &Context,
  files: &[path::PathBuf],
  check: bool,
) -> Result<Vec<path::PathBuf>, Error> {
  let mut args = args(language, ctx)?;
  args.push("--list-different".to_owned());
  let output = crate::run(&PRETTIER, &args, files, ctx.root, &[0, 1])?;
  let reported = output.stdout.lines().filter(|line| !line.is_empty());
  let changed = crate::changed(files, ctx.root, reported.map(Into::into));
  if !check && !changed.is_empty() {
    args.pop();
    args.push("--write".to_owned());
    crate::run(&PRETTIER, &args, &changed, ctx.root, &[0])?;
  }
  Ok(changed)
}

/// The prettier arguments that carry `language`'s settings, or none when
/// prettier has a config of its own and no `formality.toml` overrides it.
///
/// # Errors
///
/// Returns an error when the config is wrong.
pub fn args(language: &str, ctx: &Context) -> Result<Vec<String>, Error> {
  Ok(settings(ctx.settings(language, native(ctx))?.as_ref()))
}

/// Whether prettier finds a config of its own above the root.
pub fn native(ctx: &Context) -> bool {
  tool::nearest_config(ctx.root, CONFIGS).is_some()
}

/// The prettier arguments that carry `settings`, or none to let prettier read
/// its own config.
fn settings(settings: Option<&config::Global>) -> Vec<String> {
  let Some(settings) = settings else {
    return Vec::new();
  };
  let ending = match settings.end_of_line {
    config::EndOfLine::Lf => "lf",
    config::EndOfLine::Crlf => "crlf",
  };
  let mut args = vec![
    format!("--tab-width={}", settings.indent_size),
    format!("--print-width={}", settings.line_length),
    format!("--end-of-line={ending}"),
  ];
  if matches!(settings.quote_style, config::Quote::Single) {
    args.push("--single-quote".to_owned());
  }
  args
}
