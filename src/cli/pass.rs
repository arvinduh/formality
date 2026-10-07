//! `fml fmt`, `fml lint` and `fml fix`: one pipeline, three plans.
//!
//! Owns the file-selection flags the three commands share and the pipeline
//! that resolves targets, warns about stale tools, runs the plan, and prints
//! the results.

use std::path;
use std::time;

use clap;

use fml::engine::doctor;
use fml::engine::runner;
use fml::engine::target;
use fml::engine::version;

use crate::cli;
use crate::cli::ui;

/// Which files a pass acts on.
#[derive(clap::Args, Debug)]
pub struct Selection {
  /// Only files staged for commit
  #[arg(short = 's', long, conflicts_with = "changed")]
  staged: bool,

  /// Only modified, uncommitted files
  #[arg(long)]
  changed: bool,

  /// Only these language surfaces (e.g. rust, python, markdown)
  #[arg(short = 'l', long = "lang", value_name = "LANG")]
  lang: Vec<String>,

  /// Don't fail the run when a required tool is missing (violations and
  /// tool errors still fail it)
  #[arg(long)]
  allow_missing: bool,

  /// Files or directories to act on (defaults to the whole workspace)
  #[arg(value_name = "PATH")]
  paths: Vec<path::PathBuf>,
}

/// Arguments for `fml fmt`.
#[derive(clap::Args, Debug)]
pub struct Fmt {
  /// Report what would be reformatted, without writing
  #[arg(long)]
  check: bool,

  #[command(flatten)]
  selection: Selection,
}

/// Arguments for `fml fix`.
#[derive(clap::Args, Debug)]
pub struct Fix {
  /// Report whether fixes would change anything, without writing
  #[arg(long)]
  check: bool,

  #[command(flatten)]
  selection: Selection,
}

impl Fmt {
  /// Formats the selected files.
  pub fn run(self, ctx: &cli::Context) -> runner::ExitStatus {
    let plan = runner::Plan::fmt(self.check, self.selection.allow_missing);
    self.selection.run(ctx, &plan, (true, false))
  }
}

impl Fix {
  /// Applies lint fixes, then formats, over the selected files.
  pub fn run(self, ctx: &cli::Context) -> runner::ExitStatus {
    let plan = runner::Plan::fix(self.check, self.selection.allow_missing);
    self.selection.run(ctx, &plan, (true, true))
  }
}

impl Selection {
  /// Lints the selected files.
  pub fn lint(self, ctx: &cli::Context) -> runner::ExitStatus {
    let plan = runner::Plan::lint(self.allow_missing);
    self.run(ctx, &plan, (false, true))
  }

  /// Runs `plan` over the selection. `needs` says whether the plan formats
  /// and lints, so only the tools it uses are checked for staleness.
  fn run(
    self,
    ctx: &cli::Context,
    plan: &runner::Plan,
    (formats, lints): (bool, bool),
  ) -> runner::ExitStatus {
    let scoped = !self.paths.is_empty();
    let changes = match (self.staged, self.changed) {
      (true, _) => Some(target::Changes::Staged),
      (_, true) => Some(target::Changes::Changed),
      _ => None,
    };
    let resolved = target::resolve_targets(
      &ctx.root,
      changes,
      self.paths,
      &self.lang,
      &ctx.config,
    );
    let target = match resolved {
      Ok(Some(target)) => target,
      Ok(None) => {
        let under = if scoped { " under the given paths" } else { "" };
        let which = changes.map(|c| c.to_string()).unwrap_or_default();
        println!("No {which} files{under}.");
        return runner::ExitStatus::Clean;
      }
      Err(err) => {
        ui::error(&err);
        return runner::ExitStatus::Error;
      }
    };

    let needed = |tool: &fml::surfaces::ToolInfo| {
      (formats && tool.is_required_for_fmt)
        || (lints && tool.is_required_for_lint)
    };
    for check in doctor::check(&target.surfaces, &ctx.config, needed) {
      if let version::ToolStatus::Stale { current, pinned } = check.status {
        ui::warn(&format!(
          "{} is v{current}, pinned v{pinned}; run 'fml doctor --install'",
          check.tool.binary
        ));
      }
    }

    let start = time::Instant::now();
    let results = runner::Runner::run(
      &target.surfaces,
      &ctx.root,
      &target.scope,
      plan,
      &ctx.config,
    );
    ui::results(&results, start.elapsed());
    runner::compute_exit_status(&results, plan.allow_missing)
  }
}
