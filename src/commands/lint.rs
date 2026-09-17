//! `fml lint` command: lints the resolved target surfaces via [`Runner`].
//!
//! `lint` never writes. The deprecated `--fix` spelling is handled by
//! [`crate::run_command_inner`], which dispatches it to [`super::fix`]
//! rather than giving `lint` a writing form of its own.

use std::path::{Path, PathBuf};

use crate::commands::dispatch_plan;
use crate::config::FormalityConfig;
use crate::engine::Plan;
use crate::errors::ExitStatus;

/// Runs the `fml lint` command: the `[Lint]` plan, always report-only.
/// Provisioning missing tools is `fml doctor --install`'s job, not this
/// command's (v0.3.0, #282).
#[allow(clippy::too_many_arguments)]
pub fn run_lint(
  root: &Path,
  config: &FormalityConfig,
  staged: bool,
  changed: bool,
  lang: Vec<String>,
  paths: Vec<PathBuf>,
  allow_missing: bool,
) -> ExitStatus {
  dispatch_plan(
    root,
    config,
    staged,
    changed,
    lang,
    paths,
    &Plan::lint(allow_missing),
  )
}
