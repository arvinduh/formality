//! `fml update`: replace the running binary with the latest release.
//!
//! Sequences `engine::update::install`'s steps and prints their progress;
//! the library and the release's installer own fetching, verification and
//! the swap.

use std::env;
use std::error;
use std::iter;

use fml::engine::runner;
use fml::engine::update::install;

use crate::cli::ui;

/// Runs `fml update`, reporting a failure as an `[ERR]` line.
///
/// Must run before this process starts any thread: it sets an environment
/// variable for the installer.
pub fn run() -> runner::ExitStatus {
  // The installer otherwise adds its install directory to PATH in shell rc
  // files (or the Windows user PATH) when that directory is not on PATH.
  // Replacing a binary in place must not edit the user's shell setup.
  // SAFETY: `Cli::run` dispatches `fml update` before spawning the
  // update-check thread, and nothing earlier in `main` starts a thread, so
  // no other thread can be reading the environment concurrently.
  unsafe { env::set_var("FML_NO_MODIFY_PATH", "1") };
  match update() {
    Ok(()) => runner::ExitStatus::Clean,
    Err(err) => {
      ui::error(&with_causes(&err));
      runner::ExitStatus::Error
    }
  }
}

/// Installs the latest release over this binary, if it is newer.
fn update() -> Result<(), install::Error> {
  let current = install::current_version();
  let exe = install::running_exe()?;
  let mut updater = install::Updater::new(&exe, &current)?;
  let Some(latest) = updater.newer_version()? else {
    ui::ok(&format!("fml v{current} is already the latest release."));
    return Ok(());
  };
  install::check_replaceable(&exe)?;
  println!("⚡ formality v{current} → v{latest}");
  updater.install()?;
  println!("   Replaced {}", exe.display());
  Ok(())
}

/// Renders `err` followed by each cause in its source chain, so a failure
/// such as `failed to execute installer` names why. A cause whose text the
/// message already contains is not repeated.
fn with_causes(err: &dyn error::Error) -> String {
  let mut message = err.to_string();
  for cause in iter::successors(err.source(), |cause| cause.source()) {
    let cause = cause.to_string();
    if !message.contains(&cause) {
      message = format!("{message}: {cause}");
    }
  }
  message
}

#[cfg(test)]
mod tests {
  use std::fmt;
  use std::io;

  use super::*;

  #[derive(Debug)]
  struct Exec(io::Error);

  impl fmt::Display for Exec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      f.write_str("failed to execute installer")
    }
  }

  impl error::Error for Exec {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
      Some(&self.0)
    }
  }

  #[test]
  fn with_causes_appends_each_source() {
    let err = Exec(io::Error::other("connection reset"));
    assert_eq!(
      with_causes(&err),
      "failed to execute installer: connection reset"
    );
  }

  #[test]
  fn with_causes_skips_a_source_the_message_already_shows() {
    let err = install::Error::Unwritable {
      dir: "bin".into(),
      source: io::Error::other("denied"),
    };
    assert_eq!(with_causes(&err), err.to_string());
  }
}
