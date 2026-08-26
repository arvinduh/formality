//! `fml fmt` command: formats, or with `--check` only verifies, the resolved
//! target surfaces via [`Runner`].

use std::path::{Path, PathBuf};

use crate::commands::{
  resolve_git_paths, resolve_target_surfaces, warn_tool_install_failed,
};
use crate::config::FormalityConfig;
use crate::engine::{Runner, RunnerAction};
use crate::errors::ExitStatus;

/// Runs the `fml fmt` command: formats (or, with `check`, only verifies) the
/// resolved target surfaces, optionally installing missing tools first.
#[allow(clippy::too_many_arguments)]
pub fn run_fmt(
  root: &Path,
  config: &FormalityConfig,
  check: bool,
  staged: bool,
  changed: bool,
  lang: Vec<String>,
  install: bool,
  paths: Vec<PathBuf>,
) -> ExitStatus {
  let target_paths = match resolve_git_paths(root, staged, changed, paths) {
    Ok(p) => p,
    Err(e) => {
      e.print_diagnostic();
      return ExitStatus::Error;
    }
  };

  let surfaces =
    match resolve_target_surfaces(root, &lang, &target_paths, config) {
      Ok(s) => s,
      Err(e) => {
        e.print_diagnostic();
        return ExitStatus::Error;
      }
    };

  let mut install_failed = false;
  if install
    && !crate::commands::doctor::preflight_install(&surfaces, config, true)
  {
    warn_tool_install_failed("formatting");
    install_failed = true;
  }

  let status = Runner::run(
    surfaces,
    root,
    &target_paths,
    RunnerAction::Format { check },
    config,
  );
  if install_failed && status.is_clean() {
    ExitStatus::Error
  } else {
    status
  }
}
