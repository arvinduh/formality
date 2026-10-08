//! Installing the latest release over the running `fml` binary.
//!
//! Owns `fml update`'s library side: checking that the running binary can be
//! replaced where it is, and driving `axoupdater`, which runs the release's
//! own cargo-dist installer with its install directory forced to the
//! binary's directory. The installer downloads, verifies and installs; the
//! background release check lives in the parent `update` module, and the CLI
//! decides what the user sees.

use std::env;
use std::io;
use std::path;

use axoupdater;
use semver;
use tempfile;
use thiserror;
use tokio;

use crate::engine::update;

/// The cargo-dist app name: it names the binary and the installer assets.
const APP: &str = "fml";

/// A GitHub token sent with release requests, lifting the anonymous API rate
/// limit. The release installers read the same variable.
const TOKEN_VAR: &str = "FML_GITHUB_TOKEN";

/// Test-only stand-in for this binary's own version, so a test can run
/// `fml update` as if an older release were installed. Only debug builds
/// read it; see [`current_version`].
const CURRENT_VERSION_VAR: &str = "FML_TEST_CURRENT_VERSION";

/// Why `fml update` stopped.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// The installer writes `fml`, so a binary under another name would gain
  /// a sibling instead of being replaced.
  #[error(
    "{} is not named {APP}{}, so the installer would write a new {APP}{} \
     beside it instead of replacing it; rename it, or reinstall",
    path.display(),
    env::consts::EXE_SUFFIX,
    env::consts::EXE_SUFFIX
  )]
  NotNamedFml {
    /// The running binary.
    path: path::PathBuf,
  },
  /// The binary's directory cannot be written, so it cannot be replaced.
  #[error(
    "cannot write to {}: {source}; rerun with permission to write there",
    dir.display()
  )]
  Unwritable {
    /// The directory holding the running binary.
    dir: path::PathBuf,
    /// Why creating a file there failed.
    source: io::Error,
  },
  /// `axoupdater` ran no installer. With the version already checked, that
  /// means its own check found the running binary outside `dir`.
  #[error(
    "the installer did not run, so nothing was replaced: axoupdater found \
     the running binary outside {}",
    dir.display()
  )]
  Skipped {
    /// The directory the installer was pointed at.
    dir: path::PathBuf,
  },
  /// The installer ran and failed. It printed straight to the terminal,
  /// so `axoupdater` captured none of its output.
  #[error(
    "the installer failed ({}) with no captured output; anything it \
     printed is above",
    status.map_or_else(
      || "killed by a signal".to_string(),
      |code| format!("exit status {code}")
    )
  )]
  InstallerFailed {
    /// The installer's exit code; `None` when a signal ended it.
    status: Option<i32>,
  },
  /// The version to update from is not `SemVer`.
  #[error("invalid current version: {0}")]
  Version(#[from] semver::Error),
  /// Resolving the release or running its installer failed.
  #[error(transparent)]
  Update(#[from] Box<axoupdater::AxoupdateError>),
  /// Locating the running binary or starting the runtime failed.
  #[error(transparent)]
  Io(#[from] io::Error),
}

/// Returns the version `fml update` treats as installed. Release builds
/// (every published binary) compile the test override out.
#[must_use]
pub fn current_version() -> String {
  let forced = if cfg!(debug_assertions) {
    env::var(CURRENT_VERSION_VAR).ok()
  } else {
    None
  };
  forced.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// Returns the running binary's path, with symlinks resolved so its
/// directory is the one that holds the file. On Windows `current_exe` already
/// names the file, and canonicalizing would turn it into a `\\?\` path.
///
/// # Errors
///
/// When the running binary's path cannot be determined.
pub fn running_exe() -> io::Result<path::PathBuf> {
  let exe = env::current_exe()?;
  if cfg!(windows) {
    Ok(exe)
  } else {
    exe.canonicalize()
  }
}

/// Checks that the installer can replace `exe` where it is: it must carry
/// the name the installer writes, and its directory must be writable.
///
/// # Errors
///
/// [`Error::NotNamedFml`] for a renamed binary; [`Error::Unwritable`],
/// naming the directory, when a file cannot be created there.
pub fn check_replaceable(exe: &path::Path) -> Result<(), Error> {
  let name = format!("{APP}{}", env::consts::EXE_SUFFIX);
  if exe.file_name() != Some(name.as_ref()) {
    return Err(Error::NotNamedFml {
      path: exe.to_path_buf(),
    });
  }
  let dir = install_dir(exe);
  tempfile::tempfile_in(dir).map_err(|source| Error::Unwritable {
    dir: dir.to_path_buf(),
    source,
  })?;
  Ok(())
}

/// The directory the installer is told to install into: `exe`'s own.
fn install_dir(exe: &path::Path) -> &path::Path {
  exe.parent().unwrap_or_else(|| path::Path::new("."))
}

/// An update of one binary from this project's GitHub releases.
///
/// No install receipt is needed: the release source, current version and
/// install directory are all set explicitly, so a binary installed by any
/// method updates in place.
pub struct Updater {
  /// The configured `axoupdater` session.
  session: axoupdater::AxoUpdater,
  /// Drives `axoupdater`'s async API to completion on this thread.
  runtime: tokio::runtime::Runtime,
  /// The version being updated from.
  current: String,
  /// The directory the installer installs into.
  dir: path::PathBuf,
}

impl Updater {
  /// Prepares to update `exe`, treating `current` as the installed version.
  ///
  /// # Errors
  ///
  /// [`Error::Version`] when `current` is not `SemVer`, and [`Error::Io`]
  /// when `exe`'s directory is not UTF-8 or the runtime cannot start.
  pub fn new(exe: &path::Path, current: &str) -> Result<Self, Error> {
    let dir = install_dir(exe).to_str().ok_or_else(|| {
      io::Error::new(io::ErrorKind::InvalidData, "install path is not UTF-8")
    })?;
    // Parsed before the session exists: building it builds an HTTP client,
    // which panics on a system with no readable CA roots.
    let version = semver::Version::parse(current)?;
    let mut session = axoupdater::AxoUpdater::new_for(APP);
    if let Ok(token) = env::var(TOKEN_VAR) {
      session.set_github_token(&token);
    }
    session
      .set_release_source(axoupdater::ReleaseSource {
        release_type: axoupdater::ReleaseSourceType::GitHub,
        owner: "arvinduh".to_string(),
        name: "formality".to_string(),
        app_name: APP.to_string(),
      })
      .set_install_dir(dir)
      .set_current_version(version)
      .map_err(Box::new)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()?;
    Ok(Self {
      session,
      runtime,
      current: current.to_string(),
      dir: dir.into(),
    })
  }

  /// Returns the latest release's version when it is newer than the current
  /// one.
  ///
  /// # Errors
  ///
  /// [`Error::Update`] when the latest release cannot be resolved.
  pub fn newer_version(&mut self) -> Result<Option<String>, Error> {
    let latest = self
      .runtime
      .block_on(self.session.query_new_version())
      .map_err(Box::new)?
      .map(ToString::to_string);
    Ok(latest.filter(|latest| update::is_newer_version(latest, &self.current)))
  }

  /// Runs the latest release's installer into the binary's directory.
  ///
  /// # Side Effects
  ///
  /// The installer downloads the release archive, verifies its checksum and
  /// writes the new binary over the running one, printing its progress to
  /// this process's stdout and stderr. On Windows `axoupdater` first renames
  /// the running `.exe` aside and restores it if the installer fails.
  ///
  /// # Errors
  ///
  /// [`Error::Update`] when the installer cannot be fetched or fails,
  /// [`Error::InstallerFailed`] when it fails without captured output, and
  /// [`Error::Skipped`] when `axoupdater` declines to run it.
  pub fn install(&mut self) -> Result<(), Error> {
    match self.runtime.block_on(self.session.run()).map_err(
      |err| match err {
        axoupdater::AxoupdateError::InstallFailed {
          status,
          stdout: None,
          stderr: None,
        } => Error::InstallerFailed { status },
        err => Error::Update(Box::new(err)),
      },
    )? {
      Some(_) => Ok(()),
      None => Err(Error::Skipped {
        dir: self.dir.clone(),
      }),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn check_replaceable_refuses_a_renamed_binary() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("fml-dev");
    let err = check_replaceable(&exe).unwrap_err();
    assert!(matches!(&err, Error::NotNamedFml { path } if *path == exe));
    assert!(err.to_string().contains("fml-dev"), "{err}");
  }

  #[test]
  fn check_replaceable_accepts_fml_in_a_writable_directory() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join(format!("fml{}", env::consts::EXE_SUFFIX));
    check_replaceable(&exe).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
  }

  #[cfg(unix)]
  #[test]
  fn check_replaceable_names_a_directory_it_cannot_write() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let mode = |bits| std::fs::Permissions::from_mode(bits);
    std::fs::set_permissions(dir.path(), mode(0o555)).unwrap();
    let checked = check_replaceable(&dir.path().join("fml"));
    // Root writes through any mode bits, so the check cannot fire there.
    let writable = std::fs::File::create(dir.path().join("probe")).is_ok();
    std::fs::set_permissions(dir.path(), mode(0o755)).unwrap();
    if writable {
      return;
    }
    let err = checked.unwrap_err();
    assert!(
      matches!(&err, Error::Unwritable { dir: named, .. } if named == dir.path()),
      "{err}"
    );
    assert!(err.to_string().contains(&dir.path().display().to_string()));
  }

  #[test]
  fn installer_failure_names_its_exit_status() {
    let err = Error::InstallerFailed { status: Some(3) };
    assert!(err.to_string().contains("exit status 3"), "{err}");
  }

  #[test]
  fn updater_rejects_a_current_version_that_is_not_semver() {
    let exe = path::Path::new("fml");
    assert!(matches!(
      Updater::new(exe, "not-a-version"),
      Err(Error::Version(_))
    ));
  }
}
