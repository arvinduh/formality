//! `fml fix`: apply lint autofixes, then format.
//!
//! Owns the command's flags; the pipeline it shares with `fmt` and `lint`
//! lives in `pass`.

use clap;
use engine;

use crate::cli;
use crate::cli::pass;

/// Apply lint autofixes, then format.
#[derive(clap::Args)]
pub struct Args {
  /// Only report whether `fix` would change anything.
  #[arg(long)]
  check: bool,
  /// Which files.
  #[command(flatten)]
  select: pass::Select,
}

impl Args {
  /// Fixes and formats the selected files, or with `--check` reports what
  /// would change.
  ///
  /// # Errors
  ///
  /// Returns an error when the workspace, its config or its files cannot be
  /// read.
  pub fn run(self, global: &pass::Global) -> Result<cli::Status, cli::Error> {
    pass::run(global, engine::Command::Fix, self.check, &self.select)
  }
}
