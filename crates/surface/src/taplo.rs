//! TOML, formatted by taplo. It has no linter.

use std::ffi;
use std::fs;
use std::path;

use config;
use tool;

/// The taplo binary.
const TAPLO: tool::Tool = tool::Tool {
  binary: "taplo",
  install: "npm install -g @taplo/cli",
};

/// TOML: `.toml` files.
pub struct Toml;

impl crate::Surface for Toml {
  fn name(&self) -> &'static str {
    "toml"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["toml"]
  }

  fn format(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<crate::Report, crate::Error> {
    let native =
      tool::nearest_config(ctx.root, &[".taplo.toml", "taplo.toml"]).is_some();
    let mut args = vec!["fmt".to_owned()];
    args.extend(settings(ctx.settings(self.name(), native)?.as_ref()));
    // taplo's file globbing finds nothing on Windows, so fml feeds it each
    // file on stdin and compares.
    args.push("-".to_owned());
    let args: Vec<&ffi::OsStr> = args.iter().map(ffi::OsStr::new).collect();
    let mut changed = Vec::new();
    for file in files {
      let error = |source| crate::Error::File {
        path: file.clone(),
        source,
      };
      let text = fs::read_to_string(file).map_err(error)?;
      let output = TAPLO.pipe(&args, ctx.root, &text)?;
      if output.code != Some(0) {
        return Err(crate::Error::Failed {
          binary: TAPLO.binary,
          code: output.code,
          stderr: output.stderr,
        });
      }
      if output.stdout != text {
        if !check {
          fs::write(file, &output.stdout).map_err(error)?;
        }
        changed.push(file.clone());
      }
    }
    Ok(crate::Report {
      changed,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    _ctx: &crate::Context,
    _files: &[path::PathBuf],
    _fix: bool,
  ) -> Result<crate::Report, crate::Error> {
    Ok(crate::Report::default())
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

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::Surface;
  use crate::testing;

  #[test]
  fn format_check_reports_without_writing_and_format_writes() {
    let dir = testing::workspace(&[("a.toml", "[t]\na=1\n")]);
    let files = [dir.path().join("a.toml")];
    let check = testing::with_context(dir.path(), |ctx| {
      Toml.format(ctx, &files, true).expect("check")
    });
    assert_eq!(check.changed, files);
    assert_eq!(fs::read_to_string(&files[0]).expect("read"), "[t]\na=1\n");

    testing::with_context(dir.path(), |ctx| Toml.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "[t]\na = 1\n");
  }
}
