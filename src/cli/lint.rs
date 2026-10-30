//! `fml lint`: report lint violations.
//!
//! Owns the command's flags; the pipeline it shares with `fmt` and `fix`
//! lives in `pass`.

use clap;
use engine;

use crate::cli;
use crate::cli::pass;

/// Report lint violations.
#[derive(clap::Args)]
pub struct Args {
  /// Which files.
  #[command(flatten)]
  select: pass::Select,
}

impl Args {
  /// Lints the selected files and reports what the linters found.
  ///
  /// # Errors
  ///
  /// Returns an error when the workspace, its config or its files cannot be
  /// read.
  pub fn run(self, global: &pass::Global) -> Result<cli::Status, cli::Error> {
    pass::run(global, engine::Command::Lint, false, &self.select)
  }
}
