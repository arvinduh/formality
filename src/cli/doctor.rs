//! CLI argument definitions and adapter for `fml doctor`.
//!
//! Owns argument definitions for doctor diagnostics and delegates execution
//! to [`crate::engine::doctor`].

use std::path;

use clap;

use crate::config;
use crate::engine::doctor;
use crate::errors;

/// Arguments for `fml doctor`.
#[derive(clap::Args, Clone, Copy, Debug)]
pub struct Args {
  /// Inspect all supported surfaces regardless of project detection
  #[arg(short = 'a', long)]
  pub all: bool,

  /// Automatically install missing toolchains using available package managers
  #[arg(short = 'i', long)]
  pub install: bool,
}

/// Executes the `doctor` command.
#[must_use]
pub fn run(
  args: Args,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  doctor::run(root, args.all, args.install, config)
}
