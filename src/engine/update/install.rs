//! Installing a published release over the running `fml` binary.
//!
//! Owns `fml update`'s steps: resolving the latest release, staging beside
//! the running binary, downloading this build's cargo-dist archive verified
//! by its published checksum, and unpacking it. The background release check and its cache live in the parent
//! `update` module; the CLI decides what the user sees.

use std::env;
use std::fs;
use std::io;
use std::path;
use std::process;

use sha2;
use tempfile;
use thiserror;

use crate::engine::update;

/// The target triple this binary was compiled for (set by `build.rs`).
const TARGET: &str = env!("FML_TARGET");

/// The GitHub API endpoint naming the latest release.
const LATEST_RELEASE_API: &str =
  "https://api.github.com/repos/arvinduh/formality/releases/latest";

/// Where release assets download from, as `<base>/<tag>/<asset>`.
const DOWNLOAD_BASE: &str =
  "https://github.com/arvinduh/formality/releases/download";

/// Test-only base URL standing in for the GitHub releases (`<base>/latest`
/// for the API response, `<base>/download/<tag>/<asset>` for assets). Only
/// debug builds read it; see [`test_override`].
const RELEASES_URL_VAR: &str = "FML_TEST_RELEASES_URL";

/// Test-only stand-in for this binary's own version, so a test can run
/// `fml update` as if an older release were installed.
const CURRENT_VERSION_VAR: &str = "FML_TEST_CURRENT_VERSION";

/// Why `fml update` stopped. Every variant is raised before the running
/// binary is touched.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// A subprocess could not start or exited unsuccessfully.
  #[error("`{command}` failed: {detail}")]
  Command {
    /// The command line, for the user to retry by hand.
    command: String,
    /// The process's stderr, or why it could not start.
    detail: String,
  },
  /// The latest-release response names no tag.
  #[error("the latest-release response names no tag")]
  NoTag,
  /// The downloaded archive does not match its published sha256.
  #[error("{asset} has sha256 {actual}, but {asset}.sha256 says {expected}")]
  ChecksumMismatch {
    /// The archive's file name.
    asset: String,
    /// The digest the release publishes.
    expected: String,
    /// The digest of the bytes downloaded.
    actual: String,
  },
  /// The published `.sha256` file holds no sha256 digest.
  #[error("{0}.sha256 holds no sha256 digest")]
  BadChecksumFile(String),
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
  /// The extracted archive holds no `fml` binary.
  #[error("the archive holds no {0}")]
  MissingBinary(String),
  /// Writing the staged files failed.
  #[error(transparent)]
  Io(#[from] io::Error),
}

/// Reads the test-only variable `name`. Release builds (every published
/// binary) compile the read out, so no shipped `fml` honours it.
fn test_override(name: &str) -> Option<String> {
  if cfg!(debug_assertions) {
    env::var(name).ok()
  } else {
    None
  }
}

/// Returns the version `fml update` treats as installed.
#[must_use]
pub fn current_version() -> String {
  test_override(CURRENT_VERSION_VAR)
    .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// Returns the latest release's tag when it is newer than `current`.
///
/// # Errors
///
/// [`Error::Command`] when the release API cannot be reached;
/// [`Error::NoTag`] when its response names no tag.
pub fn newer_release(current: &str) -> Result<Option<String>, Error> {
  let url = test_override(RELEASES_URL_VAR)
    .map_or_else(|| LATEST_RELEASE_API.to_string(), |base| base + "/latest");
  let body = curl(&url)?;
  let tag = update::parse_latest_tag_from_json(&String::from_utf8_lossy(&body))
    .ok_or(Error::NoTag)?;
  Ok(update::is_newer_version(&tag, current).then_some(tag))
}

/// Creates the staging directory beside `exe`, on the same filesystem, which
/// also proves the directory writable before anything is downloaded. It is
/// removed when dropped.
///
/// # Errors
///
/// [`Error::Unwritable`], naming the directory, when it cannot be written.
pub fn stage(exe: &path::Path) -> Result<tempfile::TempDir, Error> {
  let dir = exe.parent().unwrap_or_else(|| path::Path::new("."));
  tempfile::Builder::new()
    .prefix(".fml-update-")
    .tempdir_in(dir)
    .map_err(|source| Error::Unwritable {
      dir: dir.to_path_buf(),
      source,
    })
}

/// Downloads this build's archive for `tag` into `dir`, verified against the
/// release's published `.sha256` before it is written, and returns its path
/// and size in bytes.
///
/// # Errors
///
/// [`Error::Command`] when either file cannot be fetched (a missing
/// `.sha256` included), a checksum error when the archive does not match,
/// and [`Error::Io`] when it cannot be written.
pub fn download(
  tag: &str,
  dir: &path::Path,
) -> Result<(path::PathBuf, usize), Error> {
  let asset = asset_name();
  let base = test_override(RELEASES_URL_VAR)
    .map_or_else(|| DOWNLOAD_BASE.to_string(), |base| base + "/download");
  let url = format!("{base}/{tag}/{asset}");
  let sums = curl(&format!("{url}.sha256"))?;
  let bytes = curl(&url)?;
  verify_sha256(&asset, &bytes, &String::from_utf8_lossy(&sums))?;
  let archive = dir.join(&asset);
  fs::write(&archive, &bytes)?;
  Ok((archive, bytes.len()))
}

/// Unpacks `archive` with the system `tar` into a new directory beside it and
/// returns the `fml` binary inside.
///
/// # Errors
///
/// [`Error::Command`] when `tar` is missing or rejects the archive;
/// [`Error::MissingBinary`] when it holds no `fml` binary.
pub fn extract(archive: &path::Path) -> Result<path::PathBuf, Error> {
  let dir = archive.with_extension("unpacked");
  fs::create_dir(&dir)?;
  run(
    process::Command::new(tar_program())
      .arg("-xf")
      .arg(archive)
      .arg("-C")
      .arg(&dir),
  )?;
  find_binary(&dir)
}

/// The `tar` that can read this target's archive. On Windows that is the
/// bsdtar in `System32`, which reads `.zip`; a GNU tar earlier on `PATH`
/// (Git for Windows ships one) cannot.
fn tar_program() -> path::PathBuf {
  match env::var_os("SystemRoot") {
    Some(root) if cfg!(windows) => {
      path::Path::new(&root).join("System32").join("tar.exe")
    }
    _ => path::PathBuf::from("tar"),
  }
}

/// Finds the `fml` binary in `dir`: at its root (cargo-dist's `.zip`) or one
/// directory down (its `.tar.xz`).
fn find_binary(dir: &path::Path) -> Result<path::PathBuf, Error> {
  let name = format!("fml{}", env::consts::EXE_SUFFIX);
  let top = dir.join(&name);
  if top.is_file() {
    return Ok(top);
  }
  for entry in fs::read_dir(dir)? {
    let candidate = entry?.path().join(&name);
    if candidate.is_file() {
      return Ok(candidate);
    }
  }
  Err(Error::MissingBinary(name))
}

/// Lowercase hex digits, indexed by nibble.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Checks `bytes` against `sums`, the contents of the asset's published
/// `.sha256` file (`<hex digest> *<file name>`).
///
/// # Errors
///
/// [`Error::BadChecksumFile`] when `sums` does not start with a sha256 hex
/// digest; [`Error::ChecksumMismatch`] when the digest differs.
fn verify_sha256(asset: &str, bytes: &[u8], sums: &str) -> Result<(), Error> {
  let expected = sums
    .split_whitespace()
    .next()
    .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
    .ok_or_else(|| Error::BadChecksumFile(asset.to_string()))?;
  let actual = <sha2::Sha256 as sha2::Digest>::digest(bytes)
    .iter()
    .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]])
    .map(char::from)
    .collect::<String>();
  if actual.eq_ignore_ascii_case(expected) {
    Ok(())
  } else {
    Err(Error::ChecksumMismatch {
      asset: asset.to_string(),
      expected: expected.to_string(),
      actual,
    })
  }
}

/// Fetches `url` and returns the response body, failing on an HTTP error.
fn curl(url: &str) -> Result<Vec<u8>, Error> {
  run(process::Command::new("curl").args([
    "-fsSL",
    "--retry",
    "2",
    "-H",
    "User-Agent: formality-cli",
    url,
  ]))
}

/// Runs `command` to completion and returns its stdout.
fn run(command: &mut process::Command) -> Result<Vec<u8>, Error> {
  let shown = format!(
    "{} {}",
    command.get_program().to_string_lossy(),
    command
      .get_args()
      .map(|arg| arg.to_string_lossy())
      .collect::<Vec<_>>()
      .join(" ")
  );
  let output = command.output().map_err(|err| Error::Command {
    command: shown.clone(),
    detail: err.to_string(),
  })?;
  if output.status.success() {
    Ok(output.stdout)
  } else {
    Err(Error::Command {
      command: shown,
      detail: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
  }
}

/// Returns the cargo-dist archive name for this build's target, such as
/// `fml-x86_64-unknown-linux-gnu.tar.xz`. cargo-dist zips Windows builds and
/// packs every other target as `.tar.xz`.
#[must_use]
pub fn asset_name() -> String {
  let extension = if cfg!(windows) { "zip" } else { "tar.xz" };
  format!("fml-{TARGET}.{extension}")
}

#[cfg(test)]
mod tests {
  use super::*;

  /// sha256 of the three bytes `abc` (FIPS 180-2 test vector).
  const ABC: &str =
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

  #[test]
  fn verify_sha256_accepts_the_published_digest_line() {
    let sums = format!("{ABC} *fml-x.tar.xz\n");
    assert!(verify_sha256("fml-x.tar.xz", b"abc", &sums).is_ok());
  }

  #[test]
  fn verify_sha256_rejects_a_mismatch() {
    let sums = format!("{ABC} *fml-x.tar.xz\n");
    let err = verify_sha256("fml-x.tar.xz", b"abd", &sums).unwrap_err();
    assert!(
      matches!(&err, Error::ChecksumMismatch { expected, .. } if expected == ABC),
      "{err}"
    );
  }

  #[test]
  fn verify_sha256_rejects_a_file_with_no_digest() {
    for sums in ["", "Not Found", "abc *fml-x.tar.xz"] {
      assert!(matches!(
        verify_sha256("fml-x.tar.xz", b"abc", sums),
        Err(Error::BadChecksumFile(_))
      ));
    }
  }

  #[cfg(unix)]
  #[test]
  fn stage_names_a_directory_it_cannot_write() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let staged = stage(&dir.path().join("fml"));
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    // Root writes through any mode bits, so the check cannot fire there.
    let Err(err) = staged else { return };
    assert!(
      matches!(&err, Error::Unwritable { dir: named, .. } if named == dir.path()),
      "{err}"
    );
    assert!(err.to_string().contains(&dir.path().display().to_string()));
  }

  #[test]
  fn find_binary_reads_both_cargo_dist_layouts() {
    let name = format!("fml{}", env::consts::EXE_SUFFIX);
    let zip = tempfile::tempdir().unwrap();
    fs::write(zip.path().join(&name), "").unwrap();
    assert_eq!(find_binary(zip.path()).unwrap(), zip.path().join(&name));

    let tar = tempfile::tempdir().unwrap();
    let nested = tar.path().join("fml-x86_64-unknown-linux-gnu");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("README.md"), "").unwrap();
    fs::write(nested.join(&name), "").unwrap();
    assert_eq!(find_binary(tar.path()).unwrap(), nested.join(&name));

    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
      find_binary(empty.path()),
      Err(Error::MissingBinary(_))
    ));
  }

  #[test]
  fn extract_rejects_an_archive_tar_cannot_read() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join(asset_name());
    fs::write(&archive, "not an archive").unwrap();
    assert!(matches!(extract(&archive), Err(Error::Command { .. })));
  }

  #[test]
  fn run_names_the_command_that_could_not_start() {
    let err = run(&mut process::Command::new("fml-no-such-program"))
      .expect_err("a missing program cannot run");
    assert!(err.to_string().contains("fml-no-such-program"), "{err}");
  }

  #[test]
  fn asset_name_follows_cargo_dist_naming_for_this_target() {
    let name = asset_name();
    assert!(name.starts_with(&format!("fml-{TARGET}.")), "{name}");
    assert_eq!(
      name.rsplit('.').next() == Some("zip"),
      cfg!(windows),
      "{name}"
    );
  }
}
