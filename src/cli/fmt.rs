//! `fml fmt`: format files.
//!
//! Owns the command's flags; the pipeline it shares with `lint` and `fix`
//! lives in `pass`.

use clap;
use engine;

use crate::cli;
use crate::cli::pass;

/// Format files.
#[derive(clap::Args)]
pub struct Args {
  /// Only report the files that would change.
  #[arg(long)]
  check: bool,
  /// Which files.
  #[command(flatten)]
  select: pass::Select,
}

impl Args {
  /// Formats the selected files, or with `--check` reports which would
  /// change.
  ///
  /// # Errors
  ///
  /// Returns an error when the workspace, its config or its files cannot be
  /// read.
  pub fn run(self, global: &pass::Global) -> Result<cli::Status, cli::Error> {
    pass::run(global, engine::Command::Format, self.check, &self.select)
  }
}
