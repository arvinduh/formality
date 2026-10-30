//! The languages fml formats and lints, one module each.
//!
//! Owns what each language means: which extensions it claims, which tools it
//! runs with which arguments, and how it reads their output. It does not pick
//! files or run languages in parallel; the engine does, and hands each
//! language its batch of files.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::path;

use thiserror;

use config;
use tool;

/// One language: its files, and how to format and lint them.
pub trait Surface: Sync {
  /// The language's name, which is also its config section.
  fn name(&self) -> &'static str;

  /// The lowercase extensions, without the dot, of the files it claims.
  fn extensions(&self) -> &'static [&'static str];

  /// Formats `files`, or with `check` only reports which would change.
  ///
  /// # Errors
  ///
  /// Returns an error when a tool is missing, fails, or the config is wrong.
  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error>;

  /// Lints `files`, applying the linter's autofixes first with `fix`.
  ///
  /// # Errors
  ///
  /// Returns an error when a tool is missing, fails, or the config is wrong.
  fn lint(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    fix: bool,
  ) -> Result<Report, Error>;
}

/// What a run needs to know about the workspace it runs in.
pub struct Context<'a> {
  /// The absolute workspace root.
  pub root: &'a path::Path,
  /// The merged config layers.
  pub config: &'a config::Config,
  /// Whether the workspace has its own `formality.toml`.
  pub project: bool,
  /// The user's config directory, such as `~/.config`, where tools keep
  /// their user-level configs.
  pub user_config: Option<&'a path::Path>,
}

/// What a format or lint pass found.
#[derive(Default)]
pub struct Report {
  /// Files the pass reformatted, or with `check` would reformat.
  pub changed: Vec<path::PathBuf>,
  /// Problems the linter reported, in its order.
  pub problems: Vec<Problem>,
}

/// One problem a linter reported at a place in a file.
pub struct Problem {
  /// The file, absolute.
  pub path: path::PathBuf,
  /// The 1-based line.
  pub line: u32,
  /// The 1-based column.
  pub column: u32,
  /// The rule and message as the linter wrote them.
  pub message: String,
}

/// Why a language could not format or lint its files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// A tool is missing or could not start.
  #[error(transparent)]
  Tool(#[from] tool::Error),
  /// The config has an unknown key or a wrong value.
  #[error(transparent)]
  Config(#[from] config::Error),
  /// A tool ran but failed instead of reporting.
  #[error("{binary} failed with exit code {code:?}: {stderr}")]
  Failed {
    /// The tool that failed.
    binary: &'static str,
    /// Its exit code, `None` when a signal ended it.
    code: Option<i32>,
    /// What it wrote to stderr.
    stderr: String,
  },
}
