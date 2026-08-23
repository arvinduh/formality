use colored::Colorize;
use std::path::PathBuf;

use crate::config::schema::generate_schema;
use crate::errors::{ExitStatus, FormalityError, IoError};

/// Runs the `fml schema` command: generates the JSON Schema for
/// `formality.toml` and either writes it to `output` or prints it to stdout.
pub fn run_schema(output: Option<PathBuf>) -> ExitStatus {
  let schema_json = generate_schema();
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
        ExitStatus::Clean
      }
      Err(e) => {
        FormalityError::Io(IoError::new(Some(target_file), e))
          .print_diagnostic();
        ExitStatus::Error
      }
    }
  } else {
    println!("{schema_json}");
    ExitStatus::Clean
  }
}
