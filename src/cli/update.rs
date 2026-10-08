//! `fml update`: replace the running binary with the latest release.
//!
//! Sequences `engine::update::install`'s steps and prints their progress;
//! the library owns fetching, verification and the swap.

use std::env;
use std::io;
use std::io::Write;

use fml::engine::runner;
use fml::engine::update::install;

use crate::cli::ui;

/// Runs `fml update`, reporting a failure as an `[ERR]` line.
pub fn run() -> runner::ExitStatus {
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
  let Some(tag) = install::newer_release(&current)? else {
    ui::ok(&format!("fml v{current} is already the latest release."));
    return Ok(());
  };
  println!("⚡ formality v{current} → {tag}");
  let asset = install::asset_name();
  if install::runs_under_emulation() {
    println!("   {}", install::emulation_note(&asset));
  }
  // The file behind any symlink. Windows reports the file itself, and its
  // canonical form is an unreadable `\\?\` path.
  let exe = if cfg!(windows) {
    env::current_exe()?
  } else {
    env::current_exe()?.canonicalize()?
  };
  let staging = install::stage(&exe)?;
  print!("   Downloading {asset}… ");
  io::stdout().flush()?;
  let (archive, bytes) = install::download(&tag, staging.path())?;
  println!("done ({:.1} MB)", megabytes(bytes));
  let binary = install::extract(&archive)?;
  install::install(&exe, &binary, &tag)?;
  println!("   Replaced {}", exe.display());
  Ok(())
}

/// Converts a byte count to megabytes for display.
#[expect(
  clippy::cast_precision_loss,
  reason = "a release archive is far below 2^52 bytes"
)]
fn megabytes(bytes: usize) -> f64 {
  bytes as f64 / 1_000_000.0
}
