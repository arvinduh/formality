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
      .settings(self.name(), native)?
      .map_or("file".to_owned(), |s| style(&s));
    let mut args = vec![format!("--style={style}")];
    let mut check_args = args.clone();
    check_args.extend(["--dry-run".to_owned(), "-Werror".to_owned()]);
    let output =
      crate::run(&CLANG_FORMAT, &check_args, files, ctx.root, &[0, 1])?;
    let reported = crate::problems(&output.stderr, ctx.root)
      .into_iter()
      .map(|problem| problem.path);
    let changed = crate::changed(files, ctx.root, reported);
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
      problems: crate::locate(crate::problems(&output.stdout, ctx.root), files),
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

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::testing;

  #[test]
  fn format_check_reports_without_writing_and_format_writes() {
    let source = "int main(){\nreturn 0;}\n";
    let dir = testing::workspace(&[("a.cc", source)]);
    let files = [dir.path().join("a.cc")];
    let check = testing::with_context(dir.path(), |ctx| {
      Cpp.format(ctx, &files, true).expect("check")
    });
    assert_eq!(check.changed, files);
    assert_eq!(fs::read_to_string(&files[0]).expect("read"), source);

    testing::with_context(dir.path(), |ctx| Cpp.format(ctx, &files, false))
      .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(
      text, "int main() { return 0; }\n",
      "Google keeps it on a line"
    );
  }

  #[test]
  fn lint_uses_fml_checks_only_without_a_clang_tidy_config() {
    let source = (
      "a.cc",
      "int main() {\n  int* p = 0;\n  return p ? 1 : 0;\n}\n",
    );
    let dir = testing::workspace(&[source]);
    let files = [dir.path().join("a.cc")];
    let report = testing::with_context(dir.path(), |ctx| {
      Cpp.lint(ctx, &files, false).expect("lint")
    });
    let found = |report: &Report| {
      report
        .problems
        .iter()
        .any(|p| p.message.contains("modernize-use-nullptr"))
    };
    assert!(found(&report), "modernize-use-nullptr under fml's checks");
    assert_eq!(report.problems[0].path, files[0]);

    let config = (".clang-tidy", "Checks: '-*,bugprone-*'\n");
    let dir = testing::workspace(&[source, config]);
    let files = [dir.path().join("a.cc")];
    let report = testing::with_context(dir.path(), |ctx| {
      Cpp.lint(ctx, &files, false).expect("lint")
    });
    assert!(!found(&report), "the .clang-tidy turns modernize off");
  }
}
