//! Linting pipeline and execution.
//!
//! `lint` never writes; applying fixes is `super::fix`'s job.

use std::path;

use crate::config;
use crate::engine::plan;
use crate::engine::runner;
use crate::errors;

/// Runs the linting pipeline: the `[Lint]` plan, always report-only.
#[must_use]
pub fn run(
  root: &path::Path,
  config: &config::FormalityConfig,
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
    &runner::Plan::lint(allow_missing),
  )
}
