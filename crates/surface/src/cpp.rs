//! C and C++, formatted by clang-format and linted by clang-tidy.

use std::path;

use config;
use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;

/// The clang-format binary.
const CLANG_FORMAT: tool::Tool = tool::Tool {
  binary: "clang-format",
  install: "install LLVM (apt install clang-format, brew install llvm)",
};

/// The clang-tidy binary.
const CLANG_TIDY: tool::Tool = tool::Tool {
  binary: "clang-tidy",
  install: "install LLVM (apt install clang-tidy, brew install llvm)",
};

/// The checks fml runs when clang-tidy has no `.clang-tidy` of its own.
const CHECKS: &str = "-*,bugprone-*,google-*,modernize-*,performance-*,\
  readability-*,-modernize-use-trailing-return-type,\
  -readability-identifier-length,-readability-magic-numbers";

/// C and C++: sources and headers.
pub struct Cpp;

impl Surface for Cpp {
  fn name(&self) -> &'static str {
    "cpp"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    let names = [".clang-format", "_clang-format"];
    let native = tool::nearest_config(ctx.root, &names).is_some();
    let style = ctx
      .settings(native)?
      .map_or("file".to_owned(), |s| style(&s));
    let mut args = vec![format!("--style={style}")];
    let mut check_args = args.clone();
    check_args.extend(["--dry-run".to_owned(), "-Werror".to_owned()]);
    let output =
      crate::run(&CLANG_FORMAT, &check_args, files, ctx.root, &[0, 1])?;
    let mut changed: Vec<path::PathBuf> =
      crate::problems(&output.stderr, ctx.root)
        .into_iter()
        .map(|problem| problem.path)
        .collect();
    changed.dedup();
    if !check && !changed.is_empty() {
      args.push("-i".to_owned());
      crate::run(&CLANG_FORMAT, &args, &changed, ctx.root, &[0])?;
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
    let mut args = vec!["--quiet".to_owned()];
    if tool::nearest_config(ctx.root, &[".clang-tidy"]).is_none() {
      args.push(format!("--checks={CHECKS}"));
    }
    if fix {
      args.push("--fix".to_owned());
    }
    let output = crate::run(&CLANG_TIDY, &args, files, ctx.root, &[0, 1])?;
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output.stdout, ctx.root),
    })
  }
}

/// The clang-format `--style` value that carries `settings`, on Google's
/// style.
fn style(settings: &config::Global) -> String {
  let ending = match settings.end_of_line {
    config::EndOfLine::Lf => "LF",
    config::EndOfLine::Crlf => "CRLF",
  };
  format!(
    "{{BasedOnStyle: Google, IndentWidth: {}, ColumnLimit: {}, LineEnding: \
     {ending}}}",
    settings.indent_size, settings.line_length
  )
}
