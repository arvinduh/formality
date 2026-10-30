//! JSON, formatted by prettier. It has no linter.

use std::path;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// JSON: `.json` and `.jsonc` files.
pub struct Json;

impl Surface for Json {
  fn name(&self) -> &'static str {
    "json"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["json", "jsonc"]
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
    _ctx: &Context,
    _files: &[path::PathBuf],
    _fix: bool,
  ) -> Result<Report, Error> {
    Ok(Report::default())
  }
}
