//! `fml schema`: prints or writes the `formality.toml` JSON Schema.

use std::fs;
use std::path;

use clap;

use fml::config;
use fml::engine::runner;

use crate::cli::ui;

/// Arguments for `fml schema`.
#[derive(clap::Args, Debug)]
pub struct Args {
  /// Write the schema to this file instead of stdout
  #[arg(short = 'o', long, value_name = "FILE")]
  output: Option<path::PathBuf>,
}

impl Args {
  /// Prints the schema, or writes it to `--output`.
  pub fn run(self) -> runner::ExitStatus {
    let schema = config::schema::generate_schema();
    let Some(target) = self.output else {
      println!("{schema}");
      return runner::ExitStatus::Clean;
    };
    let written = target
      .parent()
      .map_or(Ok(()), fs::create_dir_all)
      .and_then(|()| fs::write(&target, &schema));
    match written {
      Ok(()) => {
        ui::ok(&format!("wrote {}", target.display()));
        runner::ExitStatus::Clean
      }
      Err(err) => {
        ui::error(&format!("cannot write {}: {err}", target.display()));
        runner::ExitStatus::Error
      }
    }
  }
}
