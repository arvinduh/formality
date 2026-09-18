//! `fml fmt` command: formats, or with `--check` only reports, the resolved
//! target surfaces via [`Runner`].

use std::path::{Path, PathBuf};

use crate::commands::dispatch_plan;
use crate::config::FormalityConfig;
use crate::engine::Plan;
use crate::errors::ExitStatus;

/// Runs the `fml fmt` command: the `[Format]` plan, writing by default and
/// reporting only under `check`. Provisioning missing tools is `fml doctor
/// --install`'s job, not this command's (v0.3.0, #282).
#[allow(clippy::too_many_arguments)]
pub fn run_fmt(
  root: &Path,
  config: &FormalityConfig,
  check: bool,
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
    &Plan::fmt(check, allow_missing),
  )
}
