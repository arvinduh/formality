//! CLI argument definitions and pipeline execution for `fml sync`.
//!
//! Owns argument definitions for native config synchronization and composes the
//! execution pipeline from pure engine primitives.

use std::path;

use clap;

use crate::config;
use crate::engine::runner;
use crate::engine::target;
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
  let target = match target::resolve_workspace_targets(root, &args.lang, config)
  {
    Ok(t) => t,
    Err(e) => {
      e.print_diagnostic();
      return errors::ExitStatus::Error;
    }
  };
  let plan = runner::Plan::sync(args.check);
  runner::Runner::run_into(
    &mut std::io::stdout(),
    &target.surfaces,
    root,
    &target.scope,
    &plan,
    config,
  )
}
