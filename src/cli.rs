//! Command routing for the `fml` binary.
//!
//! Owns the arguments, the error a run can stop with, and which file under
//! `cli/` runs each command. `main` owns the process: logging and turning
//! the result into an exit code.

mod fix;
mod fmt;
mod lint;
mod pass;
mod report;
mod select;
mod update;
mod workspace;

use std::io;
use std::path;

use clap;
use config;
use ignore;
use thiserror;

pub use report::Status;

/// Cargo-style colors for `--help`; clap drops them for non-TTY output and
/// under `NO_COLOR`.
const STYLES: clap::builder::Styles = clap::builder::Styles::styled()
  .header(clap::builder::styling::AnsiColor::Green.on_default().bold())
  .usage(clap::builder::styling::AnsiColor::Green.on_default().bold())
  .literal(clap::builder::styling::AnsiColor::Cyan.on_default().bold())
  .placeholder(clap::builder::styling::AnsiColor::Cyan.on_default());

/// Format and lint every file in a repo, in your style.
#[derive(clap::Parser)]
#[command(name = "fml", version, styles = STYLES)]
pub struct Args {
  /// Print nothing on success, and only the violations on failure.
  #[arg(short, long, global = true)]
  quiet: bool,
  /// Log more to stderr; `-vv` for debug. `RUST_LOG` overrides it.
  #[arg(short, action = clap::ArgAction::Count, global = true)]
  pub verbose: u8,
  /// One more config layer, merged last.
  #[arg(short, long, value_name = "FILE", global = true)]
  config: Option<path::PathBuf>,
  /// What to do.
  #[command(subcommand)]
  command: Command,
}

/// fml's commands.
#[derive(clap::Subcommand)]
enum Command {
  /// Format files.
  Fmt(fmt::Args),
  /// Report lint violations.
  Lint(lint::Args),
  /// Apply lint autofixes, then format.
  Fix(fix::Args),
  /// Replace this binary with the latest release.
  Update,
}

/// Why a run stopped before it could report.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// A config layer is invalid.
  #[error(transparent)]
  Config(#[from] config::Error),
  /// The workspace root holds both `formality.toml` and `.formality.toml`.
  #[error(
    "{} holds both formality.toml and .formality.toml; keep one",
    root.display()
  )]
  BothConfigs {
    /// The workspace root.
    root: path::PathBuf,
  },
  /// A file could not be read.
  #[error("cannot read {}: {source}", path.display())]
  Read {
    /// The file.
    path: path::PathBuf,
    /// Why.
    source: io::Error,
  },
  /// The files could not be walked, or a named path does not exist.
  #[error(transparent)]
  Discover(#[from] ignore::Error),
  /// git could not list the files.
  #[error("git failed: {message}")]
  Git {
    /// What git said.
    message: String,
  },
  /// The current directory is unavailable.
  #[error("cannot read the current directory: {0}")]
  Cwd(io::Error),
  /// `fml update` failed.
  #[error(transparent)]
  Update(#[from] update::Error),
}

impl Args {
  /// Runs the command these arguments name.
  ///
  /// # Errors
  ///
  /// Returns an error when the workspace, its config or its files cannot be
  /// read, or `fml update` fails.
  pub fn run(self) -> Result<Status, Error> {
    let global = pass::Global {
      quiet: self.quiet,
      config: self.config,
    };
    match self.command {
      Command::Fmt(args) => args.run(&global),
      Command::Lint(args) => args.run(&global),
      Command::Fix(args) => args.run(&global),
      Command::Update => update::run(global.quiet),
    }
  }
}
