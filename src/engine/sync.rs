//! Native configuration synchronization and drift checking.
//!
//! Manages native config file synchronization across surfaces. Formatting and
//! linting passes are handled by `super::fmt` and `super::lint`.

use std::path;

use crate::config;
use crate::engine::plan;
use crate::engine::runner;
use crate::errors;

/// Runs the native config synchronization pipeline: the `[ConfigSync]` plan,
/// writing by default and reporting only under `check`, for the resolved target surfaces.
#[must_use]
pub fn run(
  root: &path::Path,
  config: &config::FormalityConfig,
  check: bool,
  lang: &[String],
) -> errors::ExitStatus {
  let scope =
    runner::Scope::resolve(root, &[], &config.resolve_global().exclude);
  let surfaces = match plan::resolve_target_surfaces(root, lang, &scope, config)
  {
    Ok(s) => s,
    Err(e) => {
      e.print_diagnostic();
      return errors::ExitStatus::Error;
    }
  };
  runner::Runner::run(
    &surfaces,
    root,
    &scope,
    &runner::Plan::sync(check),
    config,
  )
}
