//! `fml sync`: writes or checks native tool configs.

use std::time;

use clap;

use fml::engine::runner;
use fml::engine::target;

use crate::cli;
use crate::cli::ui;

/// Arguments for `fml sync`.
#[derive(clap::Args, Debug)]
pub struct Args {
  /// Report configs that are out of sync, without writing
  #[arg(long)]
  check: bool,

  /// Only these language surfaces
  #[arg(short = 'l', long = "lang", value_name = "LANG")]
  lang: Vec<String>,
}

impl Args {
  /// Syncs every selected surface's native config.
  pub fn run(self, ctx: &cli::Context) -> runner::ExitStatus {
    let target = match target::resolve_workspace_targets(
      &ctx.root,
      &self.lang,
      &ctx.config,
    ) {
      Ok(target) => target,
      Err(err) => {
        ui::error(&err);
        return runner::ExitStatus::Error;
      }
    };
    let plan = runner::Plan::sync(self.check);
    let start = time::Instant::now();
    let results = runner::Runner::run(
      &target.surfaces,
      &ctx.root,
      &target.scope,
      &plan,
      &ctx.config,
    );
    ui::results(&results, start.elapsed());
    runner::compute_exit_status(&results, plan.allow_missing)
  }
}
