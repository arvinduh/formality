//! CLI argument definitions and schema output for `fml schema`.
//!
//! Owns argument definitions for schema output and delegates generation directly
//! to [`crate::config::schema`].

use std::path;

use clap;
use colored::Colorize;

use crate::config;
use crate::errors;

/// Arguments for `fml schema`.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
  /// Optional file path to write the JSON schema to (defaults to stdout)
  #[arg(short = 'o', long, value_name = "FILE")]
  pub output: Option<path::PathBuf>,
}

/// Executes the `schema` command.
#[must_use]
pub fn run(args: Args) -> errors::ExitStatus {
  let schema_json = config::schema::generate_schema();
  if let Some(target_file) = args.output {
    if let Some(parent) = target_file.parent() {
      let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&target_file, &schema_json) {
      Ok(()) => {
        println!(
          "{} Wrote JSON Schema to {}",
          "[OK]".green().bold(),
          target_file.display().to_string().cyan()
        );
        errors::ExitStatus::Clean
      }
      Err(e) => {
        errors::FormalityError::Io(errors::IoError::new(Some(target_file), e))
          .print_diagnostic();
        errors::ExitStatus::Error
      }
    }
  } else {
    println!("{schema_json}");
    errors::ExitStatus::Clean
  }
}
