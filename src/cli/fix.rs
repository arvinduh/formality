//! CLI argument definitions and pipeline execution for `fml fix`.
//!
//! Owns argument definitions for autofix execution and composes the execution
//! pipeline from pure engine primitives.

use std::path;

use clap;
use colored::Colorize;

use crate::config;
use crate::engine::doctor;
use crate::engine::runner;
use crate::engine::target;
use crate::errors;

/// Arguments for `fml fix`.
#[expect(
  clippy::struct_excessive_bools,
  reason = "CLI argument struct mirrors clap flag declarations"
)]
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
  /// Report whether `fml fix` would change anything, without writing
  #[arg(long)]
  pub check: bool,

  /// Only act on files staged for git commit
  #[arg(short = 's', long)]
  pub staged: bool,

  /// Only act on modified uncommitted files in git
  #[arg(long)]
  pub changed: bool,

  /// Filter by specific language surface (e.g. rust, python, markdown)
  #[arg(short = 'l', long = "lang", value_name = "LANG")]
  pub lang: Vec<String>,

  /// A missing required tool alone does not fail the run (still reported
  /// in the table and the summary's "(allowed)" marker); a real violation
  /// or execution error still exits non-zero
  #[arg(long)]
  pub allow_missing: bool,

  /// Optional paths or files to target
  #[arg(value_name = "PATH")]
  pub paths: Vec<path::PathBuf>,
}

/// Executes the `fix` command.
#[must_use]
pub fn run(
  args: Args,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  let scoped = !args.paths.is_empty();
  let target = match target::resolve_targets(
    root,
    args.staged,
    args.changed,
    args.paths,
    &args.lang,
    config,
  ) {
    Ok(Some(t)) => t,
    Ok(None) => {
      let flag = if args.staged { "staged" } else { "changed" };
      let under = if scoped { " under the given paths" } else { "" };
      println!("{}", format!("No {flag} files{under}.").yellow());
      return errors::ExitStatus::Clean;
    }
    Err(e) => {
      e.print_diagnostic();
      return errors::ExitStatus::Error;
    }
  };

  doctor::preflight_warn_stale_tools(&target.surfaces, config, true, true);
  let plan = runner::Plan::fix(args.check, args.allow_missing);
  runner::Runner::run_into(
    &mut std::io::stdout(),
    &target.surfaces,
    root,
    &target.scope,
    &plan,
    config,
  )
}
