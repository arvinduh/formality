//! `fml fix` command: runs a lint-fix pass followed by a format pass across
//! the resolved target surfaces via [`Runner`].

use std::path::{Path, PathBuf};

use crate::commands::dispatch_surface_action;
use crate::config::FormalityConfig;
use crate::engine::RunnerAction;
use crate::errors::ExitStatus;

/// Runs the `fml fix` command: applies automatic lint fixes to the resolved
/// target surfaces, optionally installing missing tools first.
pub fn run_fix(
  root: &Path,
  config: &FormalityConfig,
  staged: bool,
  changed: bool,
  lang: Vec<String>,
  install: bool,
  paths: Vec<PathBuf>,
) -> ExitStatus {
  dispatch_surface_action(
    root,
    config,
    staged,
    changed,
    lang,
    install,
    paths,
    RunnerAction::Fix,
    "fixes",
  )
}
