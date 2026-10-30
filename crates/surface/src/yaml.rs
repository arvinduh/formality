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
      changed: crate::prettier::format(ctx, files, check)?,
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
      // yamllint's default, without the `---` rule prettier does not enforce,
      // and wrapping where the formatter wraps.
      let width = ctx.settings(false)?.map_or(80, |s| s.line_length);
      args.push(format!(
        "--config-data={{extends: default, rules: {{document-start: \
         disable, line-length: {{max: {width}}}}}}}"
      ));
    }
    let output = crate::run(&YAMLLINT, &args, files, ctx.root, &[0, 1])?;
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output.stdout, ctx.root),
    })
  }
}
