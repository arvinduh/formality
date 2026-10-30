//! The pipeline `fmt`, `lint` and `fix` share: load the workspace, pick the
//! files, run the languages and print what they found.
//!
//! Owns the order of those steps and the `Select` flags. Each step's details
//! live in `workspace`, `select`, `report` and the libraries.

use std::collections;
use std::env;
use std::path;

use clap;
use config;
use discover;
use engine;
use surface;

use crate::cli;
use crate::cli::report;
use crate::cli::select;
use crate::cli::workspace;

/// The global flags a pass reads.
pub struct Global {
  /// Print only what failed.
  pub quiet: bool,
  /// One more config layer, merged last.
  pub config: Option<path::PathBuf>,
}

/// The files a command acts on.
#[derive(clap::Args)]
pub struct Select {
  /// Only files in the git index.
  #[arg(long, conflicts_with = "changed")]
  staged: bool,
  /// Only files that differ from `HEAD`, plus untracked files.
  #[arg(long)]
  changed: bool,
  /// Files, or directories to walk; the workspace root when none.
  paths: Vec<path::PathBuf>,
}

/// Runs `command` over the files `select` names and prints the outcome.
///
/// # Errors
///
/// Returns an error when the workspace, its config or its files cannot be
/// read.
pub fn run(
  global: &Global,
  command: engine::Command,
  check: bool,
  select: &Select,
) -> Result<cli::Status, cli::Error> {
  let cwd = env::current_dir().map_err(cli::Error::Cwd)?;
  let workspace = workspace::load(&cwd, global.config.as_deref())?;
  // Checked once here, so a bad `[global]` is one error, not one per
  // language.
  let mut sections = vec!["global"];
  sections.extend(surface::SURFACES.iter().map(|language| language.name()));
  workspace.config.known(&sections)?;
  workspace.config.sections::<config::Global>(&["global"])?;
  let paths: Vec<path::PathBuf> =
    select.paths.iter().map(|path| cwd.join(path)).collect();
  let git = match (select.staged, select.changed) {
    (true, _) => Some(select::Git::Staged),
    (_, true) => Some(select::Git::Changed),
    _ => None,
  };
  let mut ignores: Vec<&str> = discover::DEFAULT_IGNORES.to_vec();
  ignores.extend(workspace.ignores.iter().map(String::as_str));
  let files = match git {
    Some(which) => {
      let files = select::files(&workspace.root, which, &paths)?;
      if files.is_empty() {
        collections::HashMap::new()
      } else {
        discover::discover(&workspace.root, &files, &ignores)?
      }
    }
    None => discover::discover(&workspace.root, &paths, &ignores)?,
  };
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
    quiet: global.quiet,
  };
  Ok(report::print(&run, &outcomes, &cwd))
}
