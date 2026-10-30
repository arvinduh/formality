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

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::testing;

  #[test]
  fn format_check_reports_without_writing_and_format_writes() {
    let dir = testing::workspace(&[("a.json", "{\"a\":1}\n")]);
    let file = dir.path().join("a.json");
    let files = [file.clone()];
    let check = testing::with_context(dir.path(), |ctx| {
      Json.format(ctx, &files, true).expect("check")
    });
    assert_eq!(check.changed, files);
    assert_eq!(fs::read_to_string(&file).expect("read"), "{\"a\":1}\n");

    testing::with_context(dir.path(), |ctx| Json.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&file).expect("read");
    assert_eq!(text, "{ \"a\": 1 }\n");
  }
}
