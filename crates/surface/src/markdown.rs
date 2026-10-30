//! Markdown, formatted by prettier and linted by markdownlint-cli2.

use std::path;

use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// The markdownlint-cli2 binary.
const MARKDOWNLINT: tool::Tool = tool::Tool {
  binary: "markdownlint-cli2",
  install: "npm install -g markdownlint-cli2",
};

/// Markdown: `.md` files.
pub struct Markdown;

impl Surface for Markdown {
  fn name(&self) -> &'static str {
    "markdown"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["md"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    Ok(Report {
      changed: crate::prettier::format(ctx, files, check)?,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    fix: bool,
  ) -> Result<Report, Error> {
    // markdownlint-cli2 finds its own `.markdownlint*` configs, so fml adds
    // no rules of its own.
    let args = if fix {
      vec!["--fix".to_owned()]
    } else {
      Vec::new()
    };
    let output = crate::run(&MARKDOWNLINT, &args, files, ctx.root, &[0, 1])?;
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output.stderr, ctx.root),
    })
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::testing;

  #[test]
  fn format_writes_with_prettier() {
    let dir = testing::workspace(&[("a.md", "# a\n\n* b\n")]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false).expect("format")
    });
    assert_eq!(report.changed, files);
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "# a\n\n- b\n");
  }

  #[test]
  fn lint_reports_markdownlint_findings() {
    let dir = testing::workspace(&[("a.md", "# a\n# b\n")]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.lint(ctx, &files, false).expect("lint")
    });
    assert!(!report.problems.is_empty());
    assert_eq!(report.problems[0].path, files[0]);
    assert!(report.problems[0].message.contains("MD"));
  }
}
