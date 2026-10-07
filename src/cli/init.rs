//! `fml init`: writes a starter config for the detected surfaces.

use std::fs;

use clap;

use fml::config;
use fml::engine::runner;
use fml::surfaces::registry;

use crate::cli;
use crate::cli::ui;

/// Arguments for `fml init`.
#[derive(clap::Args, Debug)]
pub struct Args {
  /// Overwrite an existing config
  #[arg(short = 'f', long)]
  force: bool,

  /// Write .formality.toml instead of formality.toml
  #[arg(long)]
  hidden: bool,
}

impl Args {
  /// Writes the config, refusing to replace an existing one without
  /// `--force`.
  pub fn run(self, ctx: &cli::Context) -> runner::ExitStatus {
    let name = if self.hidden {
      ".formality.toml"
    } else {
      config::DEFAULT_CONFIG_FILE_NAME
    };
    let target = ctx.root.join(name);

    if let Some(existing) = config::resolve::find_project_config(&ctx.root) {
      if !self.force {
        ui::error(&format!(
          "{} already exists; use --force to overwrite",
          existing.display()
        ));
        return runner::ExitStatus::Violations;
      }
      if existing != target {
        ui::warn(&format!(
          "{} takes precedence over {name}, which will be ignored until it \
           is removed",
          existing.display()
        ));
      }
    }

    let detected = registry::detect_surfaces_smart(&ctx.root, &ctx.config);
    let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();
    let template = config::FormalityConfig::generate_init_template(&names);
    match fs::write(&target, template) {
      Ok(()) => {
        ui::ok(&format!(
          "wrote {} with {} surface(s)",
          target.display(),
          names.len()
        ));
        runner::ExitStatus::Clean
      }
      Err(err) => {
        ui::error(&format!("cannot write {}: {err}", target.display()));
        runner::ExitStatus::Error
      }
    }
  }
}
