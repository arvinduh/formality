//! `fml fmt` command: formats, or with `--check` only reports, the resolved
//! target surfaces via [`Runner`].

use std::path;

use crate::commands;
use crate::config;
use crate::engine;
use crate::errors;

/// Runs the `fml fmt` command: the `[Format]` plan, writing by default and
/// reporting only under `check`. Provisioning missing tools is `fml doctor
/// --install`'s job, not this command's (v0.3.0, #282).
#[expect(
  clippy::too_many_arguments,
  reason = "CLI command entry point passes clap flag arguments directly"
)]
#[expect(
  clippy::fn_params_excessive_bools,
  reason = "CLI command entry point passes clap flag arguments directly"
)]
#[must_use]
pub fn run_fmt(
  root: &path::Path,
  config: &config::FormalityConfig,
  check: bool,
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
    &engine::Plan::fmt(check, allow_missing),
  )
}
