//! Installing a published release over the running `fml` binary.
//!
//! Owns `fml update`'s steps: resolving the latest release and naming this
//! build's cargo-dist archive. The background release check and its cache live in the parent
//! `update` module; the CLI decides what the user sees.

use std::env;
use std::process;

use thiserror;

use crate::engine::update;

/// The target triple this binary was compiled for (set by `build.rs`).
const TARGET: &str = env!("FML_TARGET");

/// The GitHub API endpoint naming the latest release.
const LATEST_RELEASE_API: &str =
  "https://api.github.com/repos/arvinduh/formality/releases/latest";

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
