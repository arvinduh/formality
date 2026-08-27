//! `fml lint` command: lints, optionally autofixing, the resolved target
//! surfaces via [`Runner`].

use std::path::{Path, PathBuf};

use crate::commands::dispatch_surface_action;
use crate::config::FormalityConfig;
use crate::engine::RunnerAction;
use crate::errors::ExitStatus;

/// Runs the `fml lint` command: lints (optionally autofixing with `fix`) the
/// resolved target surfaces, optionally installing missing tools first.
#[allow(clippy::too_many_arguments)]
pub fn run_lint(
  root: &Path,
  config: &FormalityConfig,
  fix: bool,
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
    RunnerAction::Lint { fix },
    "linting",
  )
}
