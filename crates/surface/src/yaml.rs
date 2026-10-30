//! YAML, formatted by prettier and linted by yamllint.

use std::path;

use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// The yamllint binary.
const YAMLLINT: tool::Tool = tool::Tool {
  binary: "yamllint",
  install: "uv tool install yamllint",
};

/// The config files yamllint reads on its own.
const CONFIGS: &[&str] = &[".yamllint", ".yamllint.yaml", ".yamllint.yml"];

/// YAML: `.yaml` and `.yml` files.
pub struct Yaml;

impl Surface for Yaml {
  fn name(&self) -> &'static str {
    "yaml"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["yaml", "yml"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    Ok(Report {
      changed: crate::prettier::format(self.name(), ctx, files, check)?,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    _fix: bool,
  ) -> Result<Report, Error> {
    let mut args = vec!["--format=parsable".to_owned()];
    let native = tool::nearest_config(ctx.root, CONFIGS).is_some();
    if !native {
      // yamllint's default, agreeing with prettier: one space before a
      // comment, no `---`, and wrapping where it wraps. Keys such as a
      // workflow's `on:` are not booleans.
      let width = ctx
        .settings(self.name(), false)?
        .map_or(80, |s| s.line_length);
      args.push(format!(
        "--config-data={{extends: default, rules: {{comments: \
         {{min-spaces-from-content: 1}}, document-start: disable, \
         line-length: {{max: {width}}}, truthy: {{check-keys: false}}}}}}"
      ));
    }
    let output = crate::run(&YAMLLINT, &args, files, ctx.root, &[0, 1])?;
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output.stdout, ctx.root),
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
    let dir = testing::workspace(&[("a.yaml", "a:   [1,2]\n")]);
    let files = [dir.path().join("a.yaml")];
    let report = testing::with_context(dir.path(), |ctx| {
      Yaml.format(ctx, &files, false).expect("format")
    });
    assert_eq!(report.changed, files);
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "a: [1, 2]\n");
  }

  #[test]
  fn lint_uses_fml_rules_only_without_a_yamllint_config() {
    let source = ("a.yaml", "on: 1 # c\na: 1 \n");
    let dir = testing::workspace(&[source]);
    let files = [dir.path().join("a.yaml")];
    let report = testing::with_context(dir.path(), |ctx| {
      Yaml.lint(ctx, &files, false).expect("lint")
    });
    assert_eq!(report.problems.len(), 1, "trailing spaces, no `---` rule");
    assert_eq!((report.problems[0].line, report.problems[0].column), (2, 5));
    assert_eq!(report.problems[0].path, files[0]);

    let dir = testing::workspace(&[source, (".yamllint", "rules: {}\n")]);
    let files = [dir.path().join("a.yaml")];
    let report = testing::with_context(dir.path(), |ctx| {
      Yaml.lint(ctx, &files, false).expect("lint")
    });
    assert!(report.problems.is_empty(), "the .yamllint enables no rules");
  }
}
