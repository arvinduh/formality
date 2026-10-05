//! CLI argument definitions and adapter for `fml sync`.
//!
//! Owns argument definitions for native config synchronization and delegates
//! execution to [`crate::engine::sync`].

use std::path;

use clap;

use crate::config;
use crate::engine::sync;
use crate::errors;

/// Arguments for `fml sync`.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
  /// Check whether native tool configs are in sync without writing changes
  #[arg(long)]
  pub check: bool,

  /// Filter by specific language surface
  #[arg(short = 'l', long = "lang", value_name = "LANG")]
  pub lang: Vec<String>,
}

/// Executes the `sync` command.
#[must_use]
pub fn run(
  args: &Args,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  sync::run(root, config, args.check, &args.lang)
}
