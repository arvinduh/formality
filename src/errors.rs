pub use crate::config::ConfigError;
use colored::Colorize;
use std::fmt;
use std::path::PathBuf;

/// Standard exit statuses for CLI invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum ExitStatus {
  /// Clean exit with no violations or errors (exit code 0).
  Clean = 0,
  /// Execution completed but rule violations or config drift were found (exit code 1).
  Violations = 1,
  /// An operational failure, invalid configuration, missing tool, or internal error occurred (exit code 2).
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

  /// Returns `true` if the status is [`ExitStatus::Violations`].
  #[must_use]
  pub const fn is_violations(self) -> bool {
    matches!(self, Self::Violations)
  }

  /// Returns `true` if the status is [`ExitStatus::Error`].
  #[must_use]
  pub const fn is_error(self) -> bool {
    matches!(self, Self::Error)
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
  /// Both `--staged` and `--changed` flags were specified simultaneously.
  MutuallyExclusiveFlags,
  /// Execution of the `git` binary failed.
  ExecutionFailed(String),
  /// Git command returned a non-zero status.
  CommandFailed(String),
  /// Generic git error message.
  Other(String),
}

impl fmt::Display for GitError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      GitError::MutuallyExclusiveFlags => write!(
        f,
        "--staged and --changed are mutually exclusive. Use one or the other."
      ),
      GitError::ExecutionFailed(msg) => {
        write!(f, "Failed to execute git: {msg}")
      }
      GitError::CommandFailed(msg) => write!(f, "Git command failed: {msg}"),
      GitError::Other(msg) => write!(f, "{msg}"),
    }
  }
}

impl std::error::Error for GitError {}

/// Error indicating a required binary tool for a surface is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolMissingError {
  /// Name of the missing executable binary.
  pub binary: String,
  /// Name of the associated language surface.
  pub surface: String,
  /// Optional installation hint/instruction for installing the missing binary.
  pub install_hint: Option<String>,
}

impl ToolMissingError {
  /// Constructs a new [`ToolMissingError`].
  #[must_use]
  pub fn new(
    binary: impl Into<String>,
    surface: impl Into<String>,
    install_hint: Option<impl Into<String>>,
  ) -> Self {
    Self {
      binary: binary.into(),
      surface: surface.into(),
      install_hint: install_hint.map(Into::into),
    }
  }
}

impl fmt::Display for ToolMissingError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "Missing tool binary '{}' for surface '{}'",
      self.binary, self.surface
    )?;
    if let Some(ref hint) = self.install_hint {
      write!(f, " (install hint: {hint})")?;
    }
    Ok(())
  }
}

impl std::error::Error for ToolMissingError {}

/// Errors related to language surfaces or native configuration rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
  /// Surface requested by name was not recognized in the registry.
  UnknownSurface(String),
  /// Serialization of native surface configuration failed.
  SerializationFailed {
    /// Surface name.
    surface: String,
    /// Detailed failure message.
    message: String,
  },
  /// Execution of tool within surface failed.
  ExecutionFailed {
    /// Surface name.
    surface: String,
    /// Detailed failure message.
    message: String,
  },
  /// Generic surface error message.
  Other {
    /// Surface name.
    surface: String,
    /// Detailed failure message.
    message: String,
  },
}

impl fmt::Display for SurfaceError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      SurfaceError::UnknownSurface(name) => write!(
        f,
        "Unknown language surface: '{name}'. Run 'fml list-surfaces' to see supported languages."
      ),
      SurfaceError::SerializationFailed { surface, message } => {
        write!(f, "Failed to serialize {surface} config: {message}")
      }
      SurfaceError::ExecutionFailed { surface, message } => {
        write!(f, "Execution error for {surface}: {message}")
      }
      SurfaceError::Other { surface, message } => {
        write!(f, "Surface error ({surface}): {message}")
      }
    }
  }
}

impl std::error::Error for SurfaceError {}

/// Standard IO error wrapper with optional path context.
#[derive(Debug)]
pub struct IoError {
  /// File or directory path associated with the IO operation, if known.
  pub path: Option<PathBuf>,
  /// Underlying standard IO error.
  pub source: std::io::Error,
}

impl IoError {
  /// Constructs a new [`IoError`] with optional path context.
  #[must_use]
  pub fn new(path: Option<PathBuf>, source: std::io::Error) -> Self {
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

impl std::error::Error for IoError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    Some(&self.source)
  }
}

/// Structured crate-wide error type for formality.
#[derive(Debug)]
pub enum FormalityError {
  /// Configuration parsing, loading, or validation errors.
  Config(ConfigError),
  /// Git repository or path resolution errors.
  Git(GitError),
  /// Missing binary toolchain errors.
  ToolMissing(ToolMissingError),
  /// Language surface resolution or serialization errors.
  Surface(SurfaceError),
  /// Standard file system or stream IO errors.
  Io(IoError),
  /// Command-line argument parsing or usage errors.
  InvalidCli(String),
}

impl FormalityError {
  /// Map error to corresponding exit status.
  #[must_use]
  pub fn exit_status(&self) -> ExitStatus {
    ExitStatus::Error
  }

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

impl fmt::Display for FormalityError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      FormalityError::Config(e) => write!(f, "{e}"),
      FormalityError::Git(e) => write!(f, "{e}"),
      FormalityError::ToolMissing(e) => write!(f, "{e}"),
      FormalityError::Surface(e) => write!(f, "{e}"),
      FormalityError::Io(e) => write!(f, "{e}"),
      FormalityError::InvalidCli(msg) => write!(f, "{msg}"),
    }
  }
}

impl std::error::Error for FormalityError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      FormalityError::Config(e) => Some(e),
      FormalityError::Git(e) => Some(e),
      FormalityError::ToolMissing(e) => Some(e),
      FormalityError::Surface(e) => Some(e),
      FormalityError::Io(e) => Some(e),
      FormalityError::InvalidCli(_) => None,
    }
  }
}

impl From<ConfigError> for FormalityError {
  fn from(err: ConfigError) -> Self {
    FormalityError::Config(err)
  }
}

impl From<GitError> for FormalityError {
  fn from(err: GitError) -> Self {
    FormalityError::Git(err)
  }
}

impl From<ToolMissingError> for FormalityError {
  fn from(err: ToolMissingError) -> Self {
    FormalityError::ToolMissing(err)
  }
}

impl From<SurfaceError> for FormalityError {
  fn from(err: SurfaceError) -> Self {
    FormalityError::Surface(err)
  }
}

impl From<IoError> for FormalityError {
  fn from(err: IoError) -> Self {
    FormalityError::Io(err)
  }
}

impl From<std::io::Error> for FormalityError {
  fn from(err: std::io::Error) -> Self {
    FormalityError::Io(IoError::new(None, err))
  }
}

impl From<FormalityError> for ExitStatus {
  fn from(_: FormalityError) -> Self {
    ExitStatus::Error
  }
}

impl From<&FormalityError> for ExitStatus {
  fn from(_: &FormalityError) -> Self {
    ExitStatus::Error
  }
}

/// Convenience result alias for formality operations returning [`FormalityError`].
pub type Result<T, E = FormalityError> = std::result::Result<T, E>;

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
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
    assert!(ExitStatus::Violations.is_violations());
    assert!(ExitStatus::Error.is_error());

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
    assert_eq!(git_err.exit_status(), ExitStatus::Error);
    assert_eq!(ExitStatus::from(&git_err), ExitStatus::Error);

    let tool_err = FormalityError::ToolMissing(ToolMissingError::new(
      "ruff",
      "python",
      Some("pip install ruff"),
    ));
    assert!(tool_err.to_string().contains("Missing tool binary 'ruff'"));
    assert!(tool_err.to_string().contains("pip install ruff"));

    let surface_err =
      FormalityError::Surface(SurfaceError::UnknownSurface("foo".into()));
    assert!(
      surface_err
        .to_string()
        .contains("Unknown language surface: 'foo'")
    );

    let cli_err = FormalityError::InvalidCli("bad flag".into());
    assert_eq!(cli_err.to_string(), "bad flag");
  }
}
