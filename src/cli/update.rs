//! `fml update`: replace the running binary with the latest release.
//!
//! Sequences `engine::update::install`'s steps and prints their progress;
//! the library and the release's installer own fetching, verification and
//! the swap.

use std::env;

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
      ui::error(&err);
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
