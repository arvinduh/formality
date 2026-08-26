//! `fml sync` command: synchronizes, or with `--check` only verifies, the
//! native tool configs generated from `formality.toml` via [`Runner`].

use std::path::Path;

use crate::commands::resolve_target_surfaces;
use crate::config::FormalityConfig;
use crate::engine::{Runner, RunnerAction};
use crate::errors::ExitStatus;

/// Runs the `fml sync` command: synchronizes (or, with `check`, only
/// verifies) the native tool configs generated from `formality.toml` for the
/// resolved target surfaces.
pub fn run_sync(
  root: &Path,
  config: &FormalityConfig,
  check: bool,
  lang: Vec<String>,
) -> ExitStatus {
  let surfaces = match resolve_target_surfaces(root, &lang, &[], config) {
    Ok(s) => s,
    Err(e) => {
      e.print_diagnostic();
      return ExitStatus::Error;
    }
  };
  Runner::run(surfaces, root, &[], RunnerAction::Sync { check }, config)
}
