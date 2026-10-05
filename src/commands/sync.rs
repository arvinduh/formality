//! `fml sync` command: synchronizes native tool configs from `formality.toml`.
//!
//! Manages native config file synchronization across surfaces. Formatting and
//! linting passes are handled by [`super::fmt`] and [`super::lint`].

use std::path;

use crate::commands;
use crate::config;
use crate::engine;
use crate::errors;

/// Runs the `fml sync` command: the `[ConfigSync]` plan, writing by default
/// and reporting only under `check`, for the resolved target surfaces.
#[must_use]
pub fn run_sync(
  root: &path::Path,
  config: &config::FormalityConfig,
  check: bool,
  lang: &[String],
) -> errors::ExitStatus {
  let scope =
    engine::Scope::resolve(root, &[], &config.resolve_global().exclude);
  let surfaces =
    match commands::resolve_target_surfaces(root, lang, &scope, config) {
      Ok(s) => s,
      Err(e) => {
        e.print_diagnostic();
        return errors::ExitStatus::Error;
      }
    };
  engine::Runner::run(
    &surfaces,
    root,
    &scope,
    &engine::Plan::sync(check),
    config,
  )
}
