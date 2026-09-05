//! `fml fix` command: runs a lint-fix pass followed by a format pass across
//! the resolved target surfaces via [`Runner`], or with `--check` reports
//! what that would do without writing.

use std::path::{Path, PathBuf};

use crate::commands::dispatch_plan;
use crate::config::FormalityConfig;
use crate::engine::Plan;
use crate::errors::ExitStatus;

/// Runs the `fml fix` command: the `[Lint, Format]` plan, writing by default
/// and reporting only under `check`, optionally installing missing tools
/// first.
#[allow(clippy::too_many_arguments)]
pub fn run_fix(
  root: &Path,
  config: &FormalityConfig,
  check: bool,
  staged: bool,
  changed: bool,
  lang: Vec<String>,
  install: bool,
  paths: Vec<PathBuf>,
) -> ExitStatus {
  dispatch_plan(
    root,
    config,
    staged,
    changed,
    lang,
    install,
    paths,
    &Plan::fix(check),
    "fixes",
  )
}
