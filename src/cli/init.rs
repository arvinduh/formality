//! CLI argument definitions and adapter for `fml init`.
//!
//! Owns argument definitions for config initialization and delegates to
//! [`crate::engine::init`].

use std::path;

use clap;

use crate::config;
use crate::engine::init;
use crate::errors;

/// Arguments for `fml init`.
#[derive(clap::Args, Clone, Copy, Debug)]
pub struct Args {
  /// Overwrite existing configuration file if it already exists
  #[arg(short = 'f', long)]
  pub force: bool,

  /// Create hidden config file (.formality.toml) instead of formality.toml
  #[arg(long)]
  pub hidden: bool,
}

/// Executes the `init` command.
#[must_use]
pub fn run(
  args: Args,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  init::run(root, config, args.force, args.hidden)
}
