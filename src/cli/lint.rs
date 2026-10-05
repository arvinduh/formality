//! CLI argument definitions and adapter for `fml lint`.
//!
//! Owns argument definitions for linting and delegates execution to
//! [`crate::engine::lint`].

use std::path;

use clap;

use crate::config;
use crate::engine::lint;
use crate::errors;

/// Arguments for `fml lint`.
#[expect(
  clippy::struct_excessive_bools,
  reason = "CLI argument struct mirrors clap flag declarations"
)]
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
  /// Rejected, not a no-op: `fml lint` never writes, so a mode flag on it
  /// would be meaningless clutter. Declared only so the error names the
  /// real reason instead of clap's misleading "to pass '--check' as a
  /// value, use '-- --check'" tip; validated in [`super::Cli::validate`].
  #[arg(long, hide = true)]
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

/// Executes the `lint` command.
#[must_use]
pub fn run(
  args: Args,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  lint::run(
    root,
    config,
    args.staged,
    args.changed,
    &args.lang,
    args.paths,
    args.allow_missing,
  )
}
