//! In-place autofix pipeline: runs a lint-fix pass followed by a format pass.
//!
//! Dispatches the `[Lint, Format]` plan through `crate::engine::runner::Runner`.
//! Read-only linting is handled by `super::lint`, formatting alone is handled
//! by `super::fmt`, and tool installation belongs to `super::doctor`.

use std::path;

use crate::config;
use crate::engine::plan;
use crate::engine::runner;
use crate::errors;

/// Runs the autofix pipeline: the `[Lint, Format]` plan, writing by default
/// and reporting only under `check`.
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
    &runner::Plan::fix(check, allow_missing),
  )
}
