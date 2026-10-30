//! Runs fml's languages over a set of files, in parallel.
//!
//! Owns which language gets which files and what each command asks of a
//! language, such as a fix running lint autofixes before formatting. The
//! caller picks the files and prints the outcomes; this crate does neither.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::collections;
use std::hash;
use std::path;

use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use surface;

/// What fml was asked to do.
#[derive(Clone, Copy)]
pub enum Command {
  /// Format the files.
  Format,
  /// Report lint problems.
  Lint,
  /// Apply lint autofixes, then format, then report what is left.
  Fix,
}

/// What one language did with its files.
pub struct Outcome {
  /// The language's name.
  pub language: &'static str,
  /// Its report, or why it could not run.
  pub result: Result<surface::Report, surface::Error>,
}

/// Runs `command` over `files`, grouped by lowercase extension as
/// `discover` returns them, each language on its own thread. With `check`,
/// nothing is written and the outcomes say what would change.
///
/// Languages with no files are left out, so an empty result means there was
/// nothing to do.
#[must_use]
pub fn run<S: hash::BuildHasher>(
  ctx: &surface::Context,
  files: &collections::HashMap<String, Vec<path::PathBuf>, S>,
  command: Command,
  check: bool,
) -> Vec<Outcome> {
  let batches: Vec<(&dyn surface::Surface, Vec<path::PathBuf>)> =
    surface::SURFACES
      .iter()
      .filter_map(|&language| {
        let mut batch: Vec<path::PathBuf> = language
          .extensions()
          .iter()
          .filter_map(|extension| files.get(*extension))
          .flatten()
          .cloned()
          .collect();
        batch.sort();
        (!batch.is_empty()).then_some((language, batch))
      })
      .collect();
  batches
    .into_par_iter()
    .map(|(language, batch)| Outcome {
      language: language.name(),
      result: apply(language, ctx, &batch, command, check),
    })
    .collect()
}

/// Runs `command` for one `language` over its `files`.
fn apply(
  language: &dyn surface::Surface,
  ctx: &surface::Context,
  files: &[path::PathBuf],
  command: Command,
  check: bool,
) -> Result<surface::Report, surface::Error> {
  match command {
    Command::Format => language.format(ctx, files, check),
    Command::Lint => language.lint(ctx, files, false),
    Command::Fix => {
      let lint = language.lint(ctx, files, !check)?;
      let format = language.format(ctx, files, check)?;
      Ok(surface::Report {
        changed: format.changed,
        problems: lint.problems,
      })
    }
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use config;
  use tempfile;

  use super::*;

  /// Writes `files` into a temp dir and runs `command` over them there.
  fn run_in(
    files: &[(&str, &str)],
    command: Command,
    check: bool,
  ) -> (tempfile::TempDir, Vec<Outcome>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut map = collections::HashMap::<String, Vec<path::PathBuf>>::new();
    for (name, text) in files {
      let file = dir.path().join(name);
      fs::write(&file, text).expect("write");
      let extension = name.rsplit('.').next().expect("extension");
      map.entry(extension.to_owned()).or_default().push(file);
    }
    let config = config::Config::new(Vec::new()).expect("config");
    let ctx = surface::Context {
      root: dir.path(),
      config: &config,
      project: false,
      user_config: None,
    };
    let outcomes = run(&ctx, &map, command, check);
    (dir, outcomes)
  }

  #[test]
  fn each_language_with_files_reports_once() {
    let files = [("a.toml", "a=1\n"), ("b.json", "{\"b\":1}\n")];
    let (dir, outcomes) = run_in(&files, Command::Format, true);
    let languages: Vec<&str> = outcomes.iter().map(|o| o.language).collect();
    assert_eq!(languages, ["json", "toml"]);
    for outcome in &outcomes {
      let report = outcome.result.as_ref().expect("report");
      assert_eq!(report.changed.len(), 1, "{}", outcome.language);
    }
    let text = fs::read_to_string(dir.path().join("a.toml")).expect("read");
    assert_eq!(text, "a=1\n", "check writes nothing");
  }

  #[test]
  fn fix_applies_autofixes_before_formatting() {
    let files = [("a.py", "import os\nx=1\n")];
    let (dir, outcomes) = run_in(&files, Command::Fix, false);
    let report = outcomes[0].result.as_ref().expect("report");
    assert!(report.problems.is_empty(), "the unused import is fixed");
    let text = fs::read_to_string(dir.path().join("a.py")).expect("read");
    assert_eq!(text, "x = 1\n");
  }
}
