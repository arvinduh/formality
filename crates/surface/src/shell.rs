//! Shell, formatted by shfmt and linted by shellcheck.

use std::path;

use tool;

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

impl crate::Surface for Shell {
  fn name(&self) -> &'static str {
    "shell"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["sh", "bash"]
  }

  fn format(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<crate::Report, crate::Error> {
    // shfmt has no config file of its own, so fml's settings always apply:
    // switch cases indented (`-ci`), binary operators leading lines (`-bn`).
    let indent = ctx
      .settings(self.name(), false)?
      .map_or(2, |s| s.indent_size);
    let mut args = vec![format!("-i={indent}"), "-ci".into(), "-bn".into()];
    args.push("-l".to_owned());
    let output = crate::run(&SHFMT, &args, files, ctx.root, &[0, 1])?;
    let reported = output.stdout.lines().filter(|line| !line.is_empty());
    let changed = crate::changed(files, ctx.root, reported.map(Into::into));
    if !check && !changed.is_empty() {
      args.pop();
      args.push("-w".to_owned());
      crate::run(&SHFMT, &args, &changed, ctx.root, &[0])?;
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
    _fix: bool,
  ) -> Result<crate::Report, crate::Error> {
    // shellcheck reads `.shellcheckrc` above the file and `~/.shellcheckrc`
    // on its own, so fml adds no checks.
    let args = ["--format=gcc".to_owned()];
    let output = crate::run(&SHELLCHECK, &args, files, ctx.root, &[0, 1])?;
    Ok(crate::Report {
      changed: Vec::new(),
      problems: crate::locate(crate::problems(&output.stdout, ctx.root), files),
    })
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::Surface;
  use crate::testing;

  #[test]
  fn format_indents_two_spaces_and_cases() {
    let source = "case $1 in\na) echo a ;;\nesac\nif true; then\necho b\nfi\n";
    let dir = testing::workspace(&[("a.sh", source)]);
    let files = [dir.path().join("a.sh")];
    let report = testing::with_context(dir.path(), |ctx| {
      Shell.format(ctx, &files, false).expect("format")
    });
    assert_eq!(report.changed, files);
    let text = fs::read_to_string(&files[0]).expect("read");
    let want =
      "case $1 in\n  a) echo a ;;\nesac\nif true; then\n  echo b\nfi\n";
    assert_eq!(text, want);
  }

  #[test]
  fn lint_reports_shellcheck_findings() {
    let dir = testing::workspace(&[("a.sh", "#!/bin/sh\necho $1\n")]);
    let files = [dir.path().join("a.sh")];
    let report = testing::with_context(dir.path(), |ctx| {
      Shell.lint(ctx, &files, false).expect("lint")
    });
    assert_eq!(report.problems.len(), 1);
    assert_eq!((report.problems[0].line, report.problems[0].column), (2, 6));
    assert!(report.problems[0].message.contains("SC2086"));
  }
}
