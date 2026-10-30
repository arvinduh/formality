//! Printing what a run found, and the exit status it adds up to.
//!
//! Owns fml's output: one line per changed file or problem, then a summary,
//! or with `quiet` only what failed. Paths print relative to the current
//! directory.

use std::iter;
use std::path;

use engine;

/// How a run ended, short of an error that stopped it.
pub enum Status {
  /// Nothing to report.
  Clean,
  /// A violation, or a file that needs formatting.
  Violations,
  /// A language could not run.
  Failed,
}

/// What the run was, for wording its output.
pub struct Run {
  /// The command.
  pub command: engine::Command,
  /// Whether it only checked.
  pub check: bool,
  /// Whether to print only what failed.
  pub quiet: bool,
}

/// Prints `outcomes` with paths relative to `cwd` and returns the status
/// they add up to.
#[expect(clippy::print_stdout, clippy::print_stderr, reason = "fml's output")]
pub fn print(
  run: &Run,
  outcomes: &[engine::Outcome],
  cwd: &path::Path,
) -> Status {
  let (mut changed, mut problems, mut failed) = (0, 0, 0);
  for outcome in outcomes {
    let report = match &outcome.result {
      Ok(report) => report,
      Err(err) => {
        eprintln!("[ERROR] {}: {err}", outcome.language);
        failed += 1;
        continue;
      }
    };
    for file in &report.changed {
      let file = relative(file, cwd);
      if run.check {
        println!("would format {file}");
      } else if !run.quiet {
        println!("formatted {file}");
      }
    }
    for problem in &report.problems {
      let file = relative(&problem.path, cwd);
      let (line, column) = (problem.line, problem.column);
      println!("{file}:{line}:{column}: {}", problem.message);
    }
    changed += report.changed.len();
    problems += report.problems.len();
  }
  let status = if failed > 0 {
    Status::Failed
  } else if problems > 0 || (run.check && changed > 0) {
    Status::Violations
  } else {
    Status::Clean
  };
  if !run.quiet {
    println!("{}", summary(run, changed, problems, failed));
  }
  status
}

/// The one-line summary, such as `2 files formatted, 1 violation`.
fn summary(
  run: &Run,
  changed: usize,
  problems: usize,
  failed: usize,
) -> String {
  let mut parts = Vec::new();
  if !matches!(run.command, engine::Command::Lint) {
    let verb = if run.check {
      "would be formatted"
    } else {
      "formatted"
    };
    parts.push(format!("{} {verb}", count(changed, "file")));
  }
  if !matches!(run.command, engine::Command::Format) {
    parts.push(count(problems, "violation"));
  }
  if failed > 0 {
    parts.push(format!("{} failed", count(failed, "language")));
  }
  parts.join(", ")
}

/// `n` and `noun`, plural unless `n` is 1.
fn count(n: usize, noun: &str) -> String {
  if n == 1 {
    format!("1 {noun}")
  } else {
    format!("{n} {noun}s")
  }
}

/// `path` relative to `cwd`, climbing out of `cwd` with `..` when needed.
fn relative(path: &path::Path, cwd: &path::Path) -> String {
  let shared = path
    .components()
    .zip(cwd.components())
    .take_while(|(a, b)| a == b)
    .count();
  if shared == 0 {
    return path.display().to_string();
  }
  let up = cwd.components().count() - shared;
  let mut out = path::PathBuf::new();
  out.extend(iter::repeat_n("..", up));
  out.extend(path.components().skip(shared));
  out.display().to_string()
}
