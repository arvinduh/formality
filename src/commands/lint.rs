//! `fml lint` command: lints the resolved target surfaces via [`Runner`].
//!
//! `lint` never writes; applying fixes is [`super::fix`]'s job.

use std::path;

use crate::commands;
use crate::config;
use crate::engine;
use crate::errors;

/// Runs the `fml lint` command: the `[Lint]` plan, always report-only.
/// Provisioning missing tools is `fml doctor --install`'s job, not this
/// command's (v0.3.0, #282).
#[must_use]
pub fn run_lint(
  root: &path::Path,
  config: &config::FormalityConfig,
  staged: bool,
  changed: bool,
  lang: &[String],
  paths: Vec<path::PathBuf>,
  allow_missing: bool,
) -> errors::ExitStatus {
  commands::dispatch_plan(
    root,
    config,
    staged,
    changed,
    lang,
    paths,
    &engine::Plan::lint(allow_missing),
  )
}
