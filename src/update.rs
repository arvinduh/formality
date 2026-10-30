//! `fml update`: replace the running binary with the latest release.
//!
//! Owns checking that the binary can be replaced where it is and driving
//! `axoupdater`, which runs the release's own installer into the binary's
//! directory. The installer downloads, verifies and installs.

use std::env;
use std::ffi;
use std::io;
use std::path;

use axoupdater;
use semver;
use tempfile;
use thiserror;

/// The cargo-dist app name, which names the binary and the installers.
const APP: &str = "fml";

/// A GitHub token for release requests, lifting the anonymous rate limit.
/// The installers read the same variable.
const TOKEN: &str = "FML_GITHUB_TOKEN";

/// Why `fml update` stopped.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// The installer writes `fml`, so a binary under another name would gain
  /// a sibling instead of being replaced.
  #[error(
    "{} is not named {APP}{}, so the installer would not replace it; \
     rename it, or reinstall",
    path.display(),
    env::consts::EXE_SUFFIX
  )]
  NotNamedFml {
    /// The running binary.
    path: path::PathBuf,
  },
  /// The binary's directory cannot be written.
  #[error("cannot write to {}: {source}", dir.display())]
  Unwritable {
    /// The directory holding the binary.
    dir: path::PathBuf,
    /// Why creating a file there failed.
    source: io::Error,
  },
  /// The running binary cannot be found, or its path is not UTF-8.
  #[error("cannot locate the running binary: {0}")]
  Exe(io::Error),
  /// Checking for or installing the release failed.
  #[error(transparent)]
  Update(#[from] Box<axoupdater::AxoupdateError>),
}

/// What `fml update` did.
pub enum Outcome {
  /// The running version is the latest.
  Latest(String),
  /// The binary was replaced.
  Updated {
    /// The version that was running.
    from: String,
    /// The version installed over it.
    to: String,
  },
}

/// Installs the latest release over the running binary when it is newer.
///
/// # Errors
///
/// Returns an error when the binary cannot be replaced where it is, or the
/// release cannot be fetched or installed.
pub fn run() -> Result<Outcome, Error> {
  let current = env!("CARGO_PKG_VERSION");
  let exe = running_exe().map_err(Error::Exe)?;
  let dir = exe.parent().unwrap_or_else(|| path::Path::new("."));
  let dir_utf8 = dir.to_str().ok_or_else(|| {
    Error::Exe(io::Error::new(
      io::ErrorKind::InvalidData,
      "path is not UTF-8",
    ))
  })?;
  let version = semver::Version::parse(current)
    .expect("Cargo only builds packages with a semver version");
  let mut updater = axoupdater::AxoUpdater::new_for(APP);
  if let Ok(token) = env::var(TOKEN) {
    updater.set_github_token(&token);
  }
  updater
    .set_release_source(axoupdater::ReleaseSource {
      release_type: axoupdater::ReleaseSourceType::GitHub,
      owner: "arvinduh".to_owned(),
      name: "formality".to_owned(),
      app_name: APP.to_owned(),
    })
    .set_install_dir(dir_utf8)
    .set_current_version(version)
    .map_err(Box::new)?;
  if !updater.is_update_needed_sync().map_err(Box::new)? {
    return Ok(Outcome::Latest(current.to_owned()));
  }
  check_replaceable(&exe, dir)?;
  let result = updater.run_sync().map_err(Box::new)?;
  let latest =
    result.map_or_else(|| current.to_owned(), |r| r.new_version.to_string());
  Ok(Outcome::Updated {
    from: current.to_owned(),
    to: latest,
  })
}

/// The running binary's path, with symlinks resolved so its directory holds
/// the file. On Windows the path already names the file.
fn running_exe() -> io::Result<path::PathBuf> {
  let exe = env::current_exe()?;
  if cfg!(windows) {
    Ok(exe)
  } else {
    exe.canonicalize()
  }
}

/// Checks that the installer can replace `exe` in `dir`: it must carry the
/// name the installer writes, and `dir` must be writable.
fn check_replaceable(exe: &path::Path, dir: &path::Path) -> Result<(), Error> {
  let name = format!("{APP}{}", env::consts::EXE_SUFFIX);
  // Windows file names ignore case, so `FML.EXE` is the same file.
  let named =
    exe
      .file_name()
      .and_then(ffi::OsStr::to_str)
      .is_some_and(|actual| {
        actual == name || cfg!(windows) && actual.eq_ignore_ascii_case(&name)
      });
  if !named {
    return Err(Error::NotNamedFml {
      path: exe.to_path_buf(),
    });
  }
  tempfile::tempfile_in(dir).map_err(|source| Error::Unwritable {
    dir: dir.to_path_buf(),
    source,
  })?;
  Ok(())
}
