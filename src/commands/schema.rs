//! `fml schema` command: generates and writes/prints the JSON Schema for
//! `formality.toml`.
//!
//! A supported, user-facing command: it is how anyone working offline, or
//! vendoring the schema into their own repo, gets the same artifact the
//! `#:schema` URL serves, and it is what the release pipeline runs to
//! generate the published schema asset.

use colored::Colorize;
use std::path;

use crate::config;
use crate::errors;

/// Runs the `fml schema` command: generates the JSON Schema for
/// `formality.toml` and either writes it to `output` or prints it to stdout.
#[must_use]
pub fn run_schema(output: Option<path::PathBuf>) -> errors::ExitStatus {
  let schema_json = config::schema::generate_schema();
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
