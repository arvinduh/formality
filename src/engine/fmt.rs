//! Formatting pipeline and execution.
//!
//! Dispatches the `[Format]` pass through `crate::engine::runner::Runner`.
//! Linting is handled by `super::lint`, and autofixing is handled by
//! `super::fix`.

use std::path;

use crate::config;
use crate::engine::plan;
use crate::engine::runner;
use crate::errors;

/// Runs the formatting pipeline: the `[Format]` plan, writing by default and
/// reporting only under `check`.
#[expect(
  clippy::too_many_arguments,
  reason = "pipeline entry point takes execution configuration options"
)]
#[expect(
  clippy::fn_params_excessive_bools,
  reason = "pipeline entry point takes execution configuration options"
)]
#[must_use]
pub fn run(
  root: &path::Path,
  config: &config::FormalityConfig,
  check: bool,
  staged: bool,
  changed: bool,
  lang: &[String],
  paths: Vec<path::PathBuf>,
  allow_missing: bool,
) -> errors::ExitStatus {
  plan::dispatch_plan(
    root,
    config,
    staged,
    changed,
    lang,
    paths,
    &runner::Plan::fmt(check, allow_missing),
  )
}
