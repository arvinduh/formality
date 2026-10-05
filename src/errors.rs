//! Crate-wide error type hierarchy and exit-status/diagnostic rendering.
//!
//! Owns `FormalityError` and `ExitStatus` for unified error reporting and
//! process exit codes. Subsystem-specific error types are owned by their respective
//! modules (such as `crate::config::ConfigError`).

use std::fmt;
use std::io;
use std::path;

use colored::Colorize;
use thiserror;

use crate::config;
use crate::surfaces;

/// Standard exit statuses for CLI invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum ExitStatus {
  /// Clean exit with no violations or errors (exit code 0).
  Clean = 0,
  /// Execution completed but rule violations or config drift were found (exit code 1).
  Violations = 1,
  /// An operational failure, invalid configuration, or internal error occurred (exit code 2).
  Error = 2,
}

impl ExitStatus {
  /// Returns the raw integer exit code.
  #[must_use]
  pub const fn code(self) -> i32 {
    self as i32
  }

  /// Returns `true` if the status is [`ExitStatus::Clean`].
  #[must_use]
  pub const fn is_clean(self) -> bool {
    matches!(self, Self::Clean)
  }
}

impl From<ExitStatus> for i32 {
  fn from(status: ExitStatus) -> Self {
    status.code()
  }
}

impl TryFrom<i32> for ExitStatus {
  type Error = FormalityError;

  fn try_from(code: i32) -> Result<Self, FormalityError> {
    match code {
      0 => Ok(Self::Clean),
      1 => Ok(Self::Violations),
      2 => Ok(Self::Error),
      _ => Err(FormalityError::InvalidCli(format!(
        "Invalid exit status code: {code}"
      ))),
    }
  }
}

impl PartialEq<i32> for ExitStatus {
  fn eq(&self, other: &i32) -> bool {
    self.code() == *other
  }
}

impl PartialEq<ExitStatus> for i32 {
  fn eq(&self, other: &ExitStatus) -> bool {
    *self == other.code()
  }
}

/// Errors occurring during Git repository operations or path resolution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
  /// Both `--staged` and `--changed` flags were specified simultaneously.
  #[error(
    "--staged and --changed are mutually exclusive. Use one or the other."
  )]
  MutuallyExclusiveFlags,
  /// Execution of the `git` binary failed.
  #[error("Failed to execute git: {0}")]
  ExecutionFailed(String),
  /// Git command returned a non-zero status.
  #[error("Git command failed: {0}")]
  CommandFailed(String),
}

/// Standard IO error wrapper with optional path context.
#[derive(Debug, thiserror::Error)]
pub struct IoError {
  /// File or directory path associated with the IO operation, if known.
  pub path: Option<path::PathBuf>,
  /// Underlying standard IO error.
  #[source]
  pub source: io::Error,
}

impl IoError {
  /// Constructs a new [`IoError`] with optional path context.
  #[must_use]
  pub fn new(path: Option<path::PathBuf>, source: io::Error) -> Self {
    Self { path, source }
  }
}

impl fmt::Display for IoError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(ref p) = self.path {
      write!(f, "IO error at {}: {}", p.display(), self.source)
    } else {
      write!(f, "IO error: {}", self.source)
    }
  }
}

/// Structured crate-wide error type for formality.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// Configuration parsing, loading, or validation errors.
  #[error(transparent)]
  Config(#[from] config::Error),
  /// Git repository or path resolution errors.
  #[error(transparent)]
  Git(#[from] GitError),
  /// Language surface resolution or serialization errors.
  #[error(transparent)]
  Surface(#[from] surfaces::Error),
  /// Standard file system or stream IO errors.
  #[error(transparent)]
  Io(#[from] IoError),
  /// Command-line argument parsing or usage errors.
  #[error("{0}")]
  InvalidCli(String),
}

/// Backward-compatible alias for [`Error`].
pub type FormalityError = Error;

impl Error {
  /// Renders standardized red bold diagnostic string for stdout/stderr.
  #[must_use]
  pub fn render_diagnostic(&self) -> String {
    format!("{} {self}", "[ERR]".red().bold())
  }

  /// Prints the diagnostic to standard error.
  pub fn print_diagnostic(&self) {
    eprintln!("{}", self.render_diagnostic());
  }
}

impl From<io::Error> for Error {
  fn from(err: io::Error) -> Self {
    Error::Io(IoError::new(None, err))
  }
}

impl From<Error> for ExitStatus {
  fn from(_: Error) -> Self {
    ExitStatus::Error
  }
}

impl From<&Error> for ExitStatus {
  fn from(_: &Error) -> Self {
    ExitStatus::Error
  }
}

/// Convenience result alias for formality operations returning [`FormalityError`].
pub type Result<T, E = FormalityError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
  use std::error;

  use super::*;

  #[test]
  fn test_exit_status_conversions() {
    assert_eq!(ExitStatus::Clean.code(), 0);
    assert_eq!(ExitStatus::Violations.code(), 1);
    assert_eq!(ExitStatus::Error.code(), 2);

    assert_eq!(i32::from(ExitStatus::Clean), 0);
    assert_eq!(i32::from(ExitStatus::Violations), 1);
    assert_eq!(i32::from(ExitStatus::Error), 2);

    assert_eq!(ExitStatus::try_from(0).unwrap(), ExitStatus::Clean);
    assert_eq!(ExitStatus::try_from(1).unwrap(), ExitStatus::Violations);
    assert_eq!(ExitStatus::try_from(2).unwrap(), ExitStatus::Error);
    assert!(ExitStatus::try_from(99).is_err());

    assert!(ExitStatus::Clean.is_clean());

    assert_eq!(ExitStatus::Clean, 0);
    assert_eq!(0, ExitStatus::Clean);
    assert_eq!(ExitStatus::Violations, 1);
    assert_eq!(ExitStatus::Error, 2);
  }

  #[test]
  fn test_error_formatting_and_diagnostics() {
    let git_err = FormalityError::Git(GitError::MutuallyExclusiveFlags);
    assert!(git_err.to_string().contains("--staged and --changed"));
    assert!(git_err.render_diagnostic().contains("[ERR]"));
    assert_eq!(ExitStatus::from(&git_err), ExitStatus::Error);

    let surface_err =
      FormalityError::Surface(surfaces::Error::UnknownSurface("foo".into()));
    assert!(
      surface_err
        .to_string()
        .contains("Unknown language surface: 'foo'")
    );

    let cli_err = FormalityError::InvalidCli("bad flag".into());
    assert_eq!(cli_err.to_string(), "bad flag");
  }

  fn assert_error<T: error::Error>() {}

  #[test]
  fn test_all_inner_error_enums_implement_std_error() {
    assert_error::<FormalityError>();
    assert_error::<config::Error>();
    assert_error::<GitError>();
    assert_error::<surfaces::Error>();
    assert_error::<IoError>();
  }
}
