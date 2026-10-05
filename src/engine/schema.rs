//! `fml schema` command: generates and writes or prints the configuration schema.
//!
//! Dispatches schema generation to `crate::config::schema::generate_schema`.
//! Strict schema parsing and validation live in `crate::config::strict`.

use colored::Colorize;
use std::path;

use crate::config;
use crate::errors;

/// Generates the canonical JSON Schema for `formality.toml`.
#[must_use]
pub fn generate() -> String {
  config::schema::generate_schema()
}

/// Runs the schema pipeline: generates the JSON Schema for `formality.toml`
/// and either writes it to `output` or prints it to stdout.
#[must_use]
pub fn run(output: Option<path::PathBuf>) -> errors::ExitStatus {
  let schema_json = generate();
  if let Some(target_file) = output {
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
