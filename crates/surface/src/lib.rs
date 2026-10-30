//! The languages fml formats and lints, one module each.
//!
//! Owns what each language means: which extensions it claims, which tools it
//! runs with which arguments, and how it reads their output. It does not pick
//! files or run languages in parallel; the engine does, and hands each
//! language its batch of files.

#![deny(clippy::print_stdout, clippy::print_stderr)]

mod cpp;
mod javascript;
mod json;
mod markdown;
mod prettier;
mod python;
mod rust;
mod shell;
mod taplo;
#[cfg(test)]
mod testing;
mod yaml;

use std::collections;
use std::ffi;
use std::fs;
use std::io;
use std::path;

use thiserror;

use config;
use tool;

/// Every language fml knows, in the order it reports them.
pub const SURFACES: &[&dyn Surface] = &[
  &cpp::Cpp,
  &javascript::JavaScript,
  &json::Json,
  &markdown::Markdown,
  &python::Python,
  &rust::Rust,
  &shell::Shell,
  &taplo::Toml,
  &yaml::Yaml,
];

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
  /// fml could not write a config it hands a tool.
  #[error("could not write a tool config: {0}")]
  Io(#[from] io::Error),
  /// fml could not read or write a file it formats itself.
  #[error("could not access {}: {source}", path.display())]
  File {
    /// The file.
    path: path::PathBuf,
    /// The operating system's reason.
    source: io::Error,
  },
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

impl Context<'_> {
  /// The formatting settings fml passes `language`'s tools: the workspace's
  /// `formality.toml` when it has one, its `[language]` table over
  /// `[global]`, else nothing when the tool has its own config (`native`),
  /// else fml's defaults.
  ///
  /// `None` means pass no settings, so the tool reads its own config.
  ///
  /// # Errors
  ///
  /// Returns an error when `[global]` or `[language]` has an unknown key or
  /// a wrong value.
  pub fn settings(
    &self,
    language: &str,
    native: bool,
  ) -> Result<Option<config::Global>, config::Error> {
    if self.project {
      return self.config.sections(&["global", language]).map(Some);
    }
    Ok((!native).then(config::Global::default))
  }
}

/// Reads the `path:line[:column]: message` lines most linters print,
/// resolving relative paths against `cwd` and skipping lines of any other
/// shape. A line with no column is placed at column 1.
fn problems(output: &str, cwd: &path::Path) -> Vec<Problem> {
  output
    .lines()
    .filter_map(|line| problem(line, cwd))
    .collect()
}

/// Reads one `path:line[:column]: message` line. The numbers are found by
/// position, so a Windows drive letter's colon stays part of the path.
fn problem(line: &str, cwd: &path::Path) -> Option<Problem> {
  line.match_indices(':').find_map(|(at, _)| {
    let (row, rest) = number(&line[at + 1..])?;
    let (column, rest) =
      rest.strip_prefix(':').and_then(number).unwrap_or((1, rest));
    Some(Problem {
      path: cwd.join(&line[..at]),
      line: row,
      column,
      message: rest.trim_start_matches(':').trim().to_owned(),
    })
  })
}

/// Splits the leading decimal number off `text`, or `None` when `text` does
/// not start with a digit.
fn number(text: &str) -> Option<(u32, &str)> {
  let end = text
    .find(|c: char| !c.is_ascii_digit())
    .unwrap_or(text.len());
  Some((text[..end].parse().ok()?, &text[end..]))
}

/// The `files` a tool reported, in their order. Paths are compared in
/// canonical form, so a tool that prints one another way, such as with
/// Windows' `\\?\` prefix or a long name for a short one, still names its
/// input file.
fn changed(
  files: &[path::PathBuf],
  root: &path::Path,
  reported: impl IntoIterator<Item = path::PathBuf>,
) -> Vec<path::PathBuf> {
  let reported: collections::HashSet<path::PathBuf> = reported
    .into_iter()
    .map(|path| canonical(&root.join(path)))
    .collect();
  files
    .iter()
    .filter(|file| reported.contains(&canonical(file)))
    .cloned()
    .collect()
}

/// `problems` with each path that names one of `files` replaced by that
/// file, compared like [`changed`] does, so output shows paths as fml was
/// given them.
fn locate(mut problems: Vec<Problem>, files: &[path::PathBuf]) -> Vec<Problem> {
  let inputs: collections::HashMap<path::PathBuf, &path::PathBuf> =
    files.iter().map(|file| (canonical(file), file)).collect();
  for problem in &mut problems {
    if let Some(file) = inputs.get(&canonical(&problem.path)) {
      problem.path.clone_from(file);
    }
  }
  problems
}

/// `path` with links and short names resolved, or as is when it cannot be.
fn canonical(path: &path::Path) -> path::PathBuf {
  fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Runs `tool` with `args`, then `files`, in `cwd`, and returns its output
/// when its exit code is one of `ok`.
///
/// Files under `cwd` are passed relative to it, so a tool's own ignore
/// patterns, which are relative to its config, match them.
///
/// # Errors
///
/// Returns an error when the tool is missing or exits with another code.
fn run(
  tool: &tool::Tool,
  args: &[String],
  files: &[path::PathBuf],
  cwd: &path::Path,
  ok: &[i32],
) -> Result<tool::Output, Error> {
  let args: Vec<&ffi::OsStr> = args
    .iter()
    .map(ffi::OsStr::new)
    .chain(files.iter().map(|file| relative(file, cwd).as_os_str()))
    .collect();
  let output = tool.run(&args, cwd)?;
  if output.code.is_some_and(|code| ok.contains(&code)) {
    return Ok(output);
  }
  Err(Error::Failed {
    binary: tool.binary,
    code: output.code,
    stderr: output.stderr,
  })
}

/// `file` relative to `cwd` when it is under it, unless that would read as
/// a flag.
fn relative<'a>(file: &'a path::Path, cwd: &path::Path) -> &'a path::Path {
  file
    .strip_prefix(cwd)
    .ok()
    .filter(|rest| !rest.as_os_str().to_string_lossy().starts_with('-'))
    .unwrap_or(file)
}
