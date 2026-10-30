//! Python, formatted and linted by ruff.
//!
//! Owns ruff's arguments and output. Import sorting runs as part of
//! formatting, since it is layout, not a finding.

use std::fs;
use std::path;

use config;
use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// The ruff binary, which formats and lints.
const RUFF: tool::Tool = tool::Tool {
  binary: "ruff",
  install: "uv tool install ruff",
};

/// The rules fml lints with when ruff has no config of its own.
const RULES: &str = "E,F,I,UP,B,SIM";

/// Python: `.py` and `.pyi` files.
pub struct Python;

impl Surface for Python {
  fn name(&self) -> &'static str {
    "python"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["py", "pyi"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    let settings = settings(ctx.settings(self.name(), native(ctx))?.as_ref());
    let imports = ruff(ctx, &["check", "--select", "I"], &settings, files)?;
    let layout = ruff(ctx, &["format", "--check"], &settings, files)?;
    let mut changed: Vec<path::PathBuf> = crate::problems(&imports, ctx.root)
      .into_iter()
      .chain(crate::problems(&layout, ctx.root))
      .map(|problem| problem.path)
      .collect();
    changed.sort_unstable();
    changed.dedup();
    if !check && !changed.is_empty() {
      ruff(
        ctx,
        &["check", "--select", "I", "--fix"],
        &settings,
        &changed,
      )?;
      ruff(ctx, &["format"], &settings, &changed)?;
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
    let native = native(ctx);
    let mut args = vec!["check"];
    if !native {
      args.extend(["--select", RULES]);
    }
    if fix {
      args.push("--fix");
    }
    let settings = settings(ctx.settings(self.name(), native)?.as_ref());
    let output = ruff(ctx, &args, &settings, files)?;
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output, ctx.root),
    })
  }
}

/// Whether ruff will find a config of its own: a `ruff.toml`, a
/// `.ruff.toml` or a `pyproject.toml` with a `[tool.ruff]` table at or above
/// the root, or a user-level `ruff/ruff.toml`.
fn native(ctx: &Context) -> bool {
  let project = tool::nearest_config(
    ctx.root,
    &["ruff.toml", ".ruff.toml", "pyproject.toml"],
  )
  .is_some_and(|path| {
    !path.ends_with("pyproject.toml")
      || fs::read_to_string(&path).is_ok_and(|text| text.contains("[tool.ruff"))
  });
  let user = ctx
    .user_config
    .is_some_and(|dir| dir.join("ruff").join("ruff.toml").is_file());
  project || user
}

/// The ruff arguments that carry `settings`, or none to let ruff read its own
/// config.
fn settings(settings: Option<&config::Global>) -> Vec<String> {
  let Some(settings) = settings else {
    return Vec::new();
  };
  let quotes = match settings.quote_style {
    config::Quote::Single => "single",
    config::Quote::Double => "double",
  };
  let ending = match settings.end_of_line {
    config::EndOfLine::Lf => "lf",
    config::EndOfLine::Crlf => "cr-lf",
  };
  vec![
    format!("--line-length={}", settings.line_length),
    format!("--config=indent-width={}", settings.indent_size),
    format!("--config=format.quote-style='{quotes}'"),
    format!("--config=format.line-ending='{ending}'"),
  ]
}

/// Runs ruff with `args`, `settings` and `files` in the root, in its concise
/// output format, and returns its stdout.
///
/// Exit code 1 means findings, which the caller reads from the output.
fn ruff(
  ctx: &Context,
  args: &[&str],
  settings: &[String],
  files: &[path::PathBuf],
) -> Result<String, Error> {
  let args: Vec<String> = args
    .iter()
    .copied()
    .chain(["--output-format=concise"])
    .map(str::to_owned)
    .chain(settings.iter().cloned())
    .collect();
  Ok(crate::run(&RUFF, &args, files, ctx.root, &[0, 1])?.stdout)
}

#[cfg(test)]
mod tests {
  use tempfile;

  use super::*;

  /// Writes `files` (name and text) into a new temp dir and returns it.
  fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
      fs::write(dir.path().join(name), text).expect("write");
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
  fn format_check_reports_without_writing_and_format_writes() {
    let dir = workspace(&[("a.py", "import sys\nimport os\nx=1\n")]);
    let file = dir.path().join("a.py");
    let files = [file.clone()];
    let check = with_context(dir.path(), |ctx| {
      Python.format(ctx, &files, true).expect("check")
    });
    assert_eq!(check.changed, files);
    assert!(fs::read_to_string(&file).expect("read").contains("x=1"));

    with_context(dir.path(), |ctx| Python.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&file).expect("read");
    assert_eq!(text, "import os\nimport sys\n\nx = 1\n");
  }

  #[test]
  fn lint_uses_fml_rules_only_without_a_ruff_config() {
    let source = ("a.py", "import os\n");
    let dir = workspace(&[source]);
    let files = [dir.path().join("a.py")];
    let report = with_context(dir.path(), |ctx| {
      Python.lint(ctx, &files, false).expect("lint")
    });
    assert_eq!(report.problems.len(), 1, "F401 under fml's rules");
    assert_eq!(report.problems[0].line, 1);
    assert!(report.problems[0].message.starts_with("F401"));

    let dir = workspace(&[source, ("ruff.toml", "lint.select = [\"E\"]\n")]);
    let files = [dir.path().join("a.py")];
    let report = with_context(dir.path(), |ctx| {
      Python.lint(ctx, &files, false).expect("lint")
    });
    assert!(report.problems.is_empty(), "the ruff.toml selects only E");
  }
}
