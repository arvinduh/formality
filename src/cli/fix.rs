//! CLI argument definitions and adapter for `fml fix`.
//!
//! Owns argument definitions for autofix execution and delegates to
//! [`crate::engine::fix`].

use std::path;

use clap;

use crate::config;
use crate::engine::fix;
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
  fix::run(
    root,
    config,
    args.check,
    args.staged,
    args.changed,
    &args.lang,
    args.paths,
    args.allow_missing,
  )
}
