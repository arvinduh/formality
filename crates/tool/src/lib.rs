//! Finds and runs the external formatters and linters fml wraps.
//!
//! Owns locating a tool's binary, running it over a batch of files, and
//! finding the tool's own config file. It knows no languages: each language
//! names its tools, their arguments and their config files, and decides what
//! an exit code means.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::ffi;
use std::io;
use std::path;
use std::process;

use thiserror;
use which;

/// An external program fml runs, and how to get it when it is missing.
pub struct Tool {
  /// The program name looked up on `PATH`, such as `ruff`.
  pub binary: &'static str,
  /// The command that installs it, shown when it is missing.
  pub install: &'static str,
}

/// Why a tool could not be run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// The binary is not on `PATH`.
  #[error("{binary} not found; install it with: {install}")]
  Missing {
    /// The program name that was looked up.
    binary: &'static str,
    /// The command that installs it.
    install: &'static str,
  },
  /// The binary was found but could not be started.
  #[error("could not run {binary}: {source}")]
  Spawn {
    /// The program that failed to start.
    binary: &'static str,
    /// The operating system's reason.
    source: io::Error,
  },
}

/// What a finished tool reported.
pub struct Output {
  /// The exit code, or `None` when a signal ended the tool.
  pub code: Option<i32>,
  /// Everything the tool wrote to stdout, decoded lossily as UTF-8.
  pub stdout: String,
  /// Everything the tool wrote to stderr, decoded lossily as UTF-8.
  pub stderr: String,
}

impl Tool {
  /// Runs the tool with `args` in `cwd` and waits for it to finish.
  ///
  /// # Errors
  ///
  /// Returns [`Error::Missing`] when the binary is not on `PATH`, and
  /// [`Error::Spawn`] when it is found but cannot be started.
  pub fn run(
    &self,
    args: &[&ffi::OsStr],
    cwd: &path::Path,
  ) -> Result<Output, Error> {
    let program = which::which(self.binary).map_err(|_| Error::Missing {
      binary: self.binary,
      install: self.install,
    })?;
    // fml parses what tools print, so it asks them for plain text even when
    // the caller's environment forces color.
    let output = process::Command::new(program)
      .args(args)
      .current_dir(cwd)
      .env_remove("FORCE_COLOR")
      .env_remove("CLICOLOR_FORCE")
      .env("NO_COLOR", "1")
      .env("CARGO_TERM_COLOR", "never")
      .output()
      .map_err(|source| Error::Spawn {
        binary: self.binary,
        source,
      })?;
    Ok(Output {
      code: output.status.code(),
      stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
  }
}

/// Finds the nearest of `names` in `start` or the directories above it, the
/// way a tool looks for its own config file.
///
/// Within one directory, `names` are tried in order. Returns `None` when no
/// directory up to the filesystem root holds any of them.
#[must_use]
pub fn nearest_config(
  start: &path::Path,
  names: &[&str],
) -> Option<path::PathBuf> {
  start.ancestors().find_map(|dir| {
    names
      .iter()
      .map(|name| dir.join(name))
      .find(|path| path.is_file())
  })
}

#[cfg(test)]
mod tests {
  use std::fs;

  use tempfile;

  use super::*;

  #[test]
  fn a_missing_binary_names_its_install_command() {
    let tool = Tool {
      binary: "fml-test-no-such-tool",
      install: "cargo install it",
    };
    let err = tool.run(&[], path::Path::new(".")).err().expect("missing");
    assert_eq!(
      err.to_string(),
      "fml-test-no-such-tool not found; install it with: cargo install it"
    );
  }

  #[test]
  fn run_captures_exit_code_and_stdout() {
    let tool = Tool {
      binary: "rustc",
      install: "rustup",
    };
    let args = [ffi::OsStr::new("--version")];
    let output = tool.run(&args, path::Path::new(".")).expect("rustc runs");
    assert_eq!(output.code, Some(0), "{}", output.stderr);
    assert!(output.stdout.starts_with("rustc "), "{}", output.stdout);
  }

  #[test]
  fn nearest_config_walks_up_and_tries_names_in_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let nested = dir.path().join("a/b");
    fs::create_dir_all(&nested).expect("mkdir");
    fs::write(dir.path().join("ruff.toml"), "").expect("write");
    fs::write(dir.path().join(".ruff.toml"), "").expect("write");

    let found = nearest_config(&nested, &[".ruff.toml", "ruff.toml"]);
    assert_eq!(found, Some(dir.path().join(".ruff.toml")));
    assert_eq!(nearest_config(&nested, &["biome.json"]), None);
  }
}
