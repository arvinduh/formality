//! The `fml` command: formats and lints every file in a repo with the best
//! tool for each language.
//!
//! Owns the arguments, reading the workspace from disk, choosing the files,
//! and turning what the languages report into output and an exit code: 0
//! clean, 1 violations, 2 an operational failure.

mod report;
mod workspace;

use std::env;
use std::io;
use std::path;
use std::process;

use clap;
use clap::Parser;
use config;
use discover;
use engine;
use env_logger;
use ignore;
use log;
use surface;
use thiserror;

/// Format and lint every file in a repo, in your style.
#[derive(clap::Parser)]
#[command(name = "fml", version)]
struct Args {
  /// Print nothing on success, and only the violations on failure.
  #[arg(short, long, global = true)]
  quiet: bool,
  /// Log more to stderr; `-vv` for debug. `RUST_LOG` overrides it.
  #[arg(short, action = clap::ArgAction::Count, global = true)]
  verbose: u8,
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
  Fmt {
    /// Only report the files that would change.
    #[arg(long)]
    check: bool,
    /// Which files.
    #[command(flatten)]
    select: Select,
  },
  /// Report lint violations.
  Lint {
    /// Which files.
    #[command(flatten)]
    select: Select,
  },
  /// Apply lint autofixes, then format.
  Fix {
    /// Only report whether `fix` would change anything.
    #[arg(long)]
    check: bool,
    /// Which files.
    #[command(flatten)]
    select: Select,
  },
}

/// The files a command acts on.
#[derive(clap::Args)]
struct Select {
  /// Files, or directories to walk; the workspace root when none.
  paths: Vec<path::PathBuf>,
}

/// Why a run stopped before it could report.
#[derive(Debug, thiserror::Error)]
enum Error {
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
  /// The current directory is unavailable.
  #[error("cannot read the current directory: {0}")]
  Cwd(io::Error),
}

fn main() -> process::ExitCode {
  let args = Args::parse();
  let level = match args.verbose {
    0 => log::LevelFilter::Warn,
    1 => log::LevelFilter::Info,
    _ => log::LevelFilter::Debug,
  };
  env_logger::Builder::new()
    .filter_level(level)
    .parse_default_env()
    .format_timestamp(None)
    .format_target(false)
    .init();
  match run(&args) {
    Ok(report::Status::Clean) => process::ExitCode::SUCCESS,
    Ok(report::Status::Violations) => process::ExitCode::from(1),
    Ok(report::Status::Failed) => process::ExitCode::from(2),
    Err(err) => {
      #[expect(clippy::print_stderr, reason = "fml's error output")]
      {
        eprintln!("[ERROR] {err}");
      }
      process::ExitCode::from(2)
    }
  }
}

/// Runs the command `args` names.
///
/// # Errors
///
/// Returns an error when the workspace, its config or its files cannot be
/// read.
fn run(args: &Args) -> Result<report::Status, Error> {
  let (command, check, select) = match &args.command {
    Command::Fmt { check, select } => (engine::Command::Format, *check, select),
    Command::Lint { select } => (engine::Command::Lint, false, select),
    Command::Fix { check, select } => (engine::Command::Fix, *check, select),
  };
  let cwd = env::current_dir().map_err(Error::Cwd)?;
  let workspace = workspace::load(&cwd, args.config.as_deref())?;
  // Checked once here, so a bad `[global]` is one error, not one per
  // language.
  let mut sections = vec!["global"];
  sections.extend(surface::SURFACES.iter().map(|language| language.name()));
  workspace.config.known(&sections)?;
  workspace.config.sections::<config::Global>(&["global"])?;
  let paths: Vec<path::PathBuf> =
    select.paths.iter().map(|path| cwd.join(path)).collect();
  let mut ignores: Vec<&str> = discover::DEFAULT_IGNORES.to_vec();
  ignores.extend(workspace.ignores.iter().map(String::as_str));
  let files = discover::discover(&workspace.root, &paths, &ignores)?;
  let ctx = surface::Context {
    root: &workspace.root,
    config: &workspace.config,
    project: workspace.project,
    user_config: workspace.user_config.as_deref(),
  };
  let outcomes = engine::run(&ctx, &files, command, check);
  let run = report::Run {
    command,
    check,
    quiet: args.quiet,
  };
  Ok(report::print(&run, &outcomes, &cwd))
}
