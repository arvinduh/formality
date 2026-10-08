//! `fml doctor`: checks the toolchain and optionally installs what is missing.

use clap;

use fml::engine::doctor;
use fml::engine::runner;
use fml::surfaces;
use fml::surfaces::registry;

use crate::cli;
use crate::cli::ui;

/// Arguments for `fml doctor`.
#[derive(clap::Args, Debug)]
pub struct Args {
  /// Check every surface, not only the ones detected in the workspace
  #[arg(short = 'a', long)]
  all: bool,

  /// Install missing tools, and stale ones whose installer can pin them
  #[arg(short = 'i', long)]
  install: bool,
}

impl Args {
  /// Checks the tools, then the Python virtualenv and `.gitignore` hygiene,
  /// then installs if asked.
  pub fn run(self, ctx: &cli::Context) -> runner::ExitStatus {
    let mut surfaces = registry::detect_surfaces_smart(&ctx.root, &ctx.config);
    if self.all || surfaces.is_empty() {
      surfaces = registry::all_surfaces();
    }

    let checks = doctor::check(&surfaces, &ctx.config, |_| true);
    for check in &checks {
      ui::check(check);
    }
    hygiene(ctx, &surfaces);

    let pending: Vec<&doctor::Check> =
      checks.iter().filter(|c| c.needs_install()).collect();
    if pending.is_empty() {
      return runner::ExitStatus::Clean;
    }
    if !self.install {
      println!(
        "\n{} tool(s) to install; run 'fml doctor --install'",
        pending.len()
      );
      let missing = checks.iter().any(|c| {
        matches!(c.status, fml::engine::version::ToolStatus::NotFound)
      });
      return if missing {
        runner::ExitStatus::Error
      } else {
        runner::ExitStatus::Clean
      };
    }

    let mut failed = false;
    for check in pending {
      let outcome = doctor::install(&check.tool);
      failed |= !matches!(outcome, doctor::Install::Installed);
      ui::install(check.tool.binary, &outcome);
    }
    if failed {
      runner::ExitStatus::Error
    } else {
      runner::ExitStatus::Clean
    }
  }
}

/// Warns about an inactive Python virtualenv and `.gitignore` gaps.
fn hygiene(
  ctx: &cli::Context,
  surfaces: &[Box<dyn surfaces::LanguageSurface>],
) {
  if surfaces.iter().any(|s| s.name() == "python") {
    let venv = doctor::venv::detect_virtualenv(&ctx.root);
    match venv.venv_path {
      Some(path) if venv.is_active => {
        println!("\nvirtualenv: {}", path.display());
      }
      _ => ui::warn("no active Python virtualenv"),
    }
  }
  let report = doctor::gitignore::check_gitignore_hygiene(&ctx.root, surfaces);
  if !report.gitignore_exists {
    ui::warn("no .gitignore in the workspace root");
  }
  for issue in report.issues {
    ui::warn(&format!(
      ".gitignore is missing {} entries: {}",
      issue.category,
      issue.missing_patterns.join(", ")
    ));
  }
}
