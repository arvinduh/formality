//! `fml sync` command: synchronizes, or with `--check` only verifies, the
//! native tool configs generated from `formality.toml` via [`Runner`].

use std::path::Path;

use crate::commands::resolve_target_surfaces;
use crate::config::FormalityConfig;
use crate::engine::{Plan, Runner, Scope};
use crate::errors::ExitStatus;

/// Runs the `fml sync` command: the `[ConfigSync]` plan, writing by default
/// and reporting only under `check`, for the resolved target surfaces.
#[must_use]
pub fn run_sync(
  root: &Path,
  config: &FormalityConfig,
  check: bool,
  lang: &[String],
) -> ExitStatus {
  let scope = Scope::resolve(root, &[], &config.resolve_global().exclude);
  let surfaces = match resolve_target_surfaces(root, lang, &scope, config) {
    Ok(s) => s,
    Err(e) => {
      e.print_diagnostic();
      return ExitStatus::Error;
    }
  };
  Runner::run(&surfaces, root, &scope, &Plan::sync(check), config)
}
