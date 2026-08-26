//! `fml fix` command: runs a lint-fix pass followed by a format pass across
//! the resolved target surfaces via [`Runner`].

use std::path::{Path, PathBuf};

use crate::commands::{
  resolve_git_paths, resolve_target_surfaces, warn_tool_install_failed,
};
use crate::config::FormalityConfig;
use crate::engine::{Runner, RunnerAction};
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
  if install {
    let lint_ok =
      crate::commands::doctor::preflight_install(&surfaces, config, false);
    let fmt_ok =
      crate::commands::doctor::preflight_install(&surfaces, config, true);
    if !lint_ok || !fmt_ok {
      warn_tool_install_failed("fixes");
      install_failed = true;
    }
  } else {
    crate::commands::doctor::preflight_warn_stale_tools(
      &surfaces, config, true, true,
    );
  }

  let status =
    Runner::run(surfaces, root, &target_paths, RunnerAction::Fix, config);
  if install_failed && status.is_clean() {
    ExitStatus::Error
  } else {
    status
  }
}
