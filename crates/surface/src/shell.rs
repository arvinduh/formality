//! Shell, formatted by shfmt and linted by shellcheck.

use std::path;

use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// The shfmt binary.
const SHFMT: tool::Tool = tool::Tool {
  binary: "shfmt",
  install: "go install mvdan.cc/sh/v3/cmd/shfmt@latest",
};

/// The shellcheck binary.
const SHELLCHECK: tool::Tool = tool::Tool {
  binary: "shellcheck",
  install: "uv tool install shellcheck-py",
};

/// Shell: `.sh` and `.bash` files.
pub struct Shell;

impl Surface for Shell {
  fn name(&self) -> &'static str {
    "shell"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["sh", "bash"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    // shfmt has no config file of its own, so fml's settings always apply:
    // switch cases indented (`-ci`), binary operators leading lines (`-bn`).
    let indent = ctx.settings(false)?.map_or(2, |s| s.indent_size);
    let mut args = vec![format!("-i={indent}"), "-ci".into(), "-bn".into()];
    args.push("-l".to_owned());
    let output = crate::run(&SHFMT, &args, files, ctx.root, &[0, 1])?;
    let changed: Vec<path::PathBuf> = output
      .stdout
      .lines()
      .filter(|line| !line.is_empty())
      .map(|line| ctx.root.join(line))
      .collect();
    if !check && !changed.is_empty() {
      args.pop();
      args.push("-w".to_owned());
      crate::run(&SHFMT, &args, &changed, ctx.root, &[0])?;
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
    _fix: bool,
  ) -> Result<Report, Error> {
    // shellcheck reads `.shellcheckrc` above the file and `~/.shellcheckrc`
    // on its own, so fml adds no checks.
    let args = ["--format=gcc".to_owned()];
    let output = crate::run(&SHELLCHECK, &args, files, ctx.root, &[0, 1])?;
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output.stdout, ctx.root),
    })
  }
}
