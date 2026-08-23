use colored::Colorize;

use crate::errors::{ExitStatus, FormalityError, IoError};
use crate::ui::table;

/// Runs the `fml table` command: renders a JSON table spec (from `json`, or
/// read from stdin if not given) to formatted terminal output.
pub fn run_table(json: Option<String>) -> ExitStatus {
  let json_str = if let Some(j) = json {
    j
  } else {
    use std::io::Read;
    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
      FormalityError::Io(IoError::new(None, e)).print_diagnostic();
      return ExitStatus::Error;
    }
    buf
  };
  match table::render_json(&json_str) {
    Ok(rendered) => {
      print!("{rendered}");
      ExitStatus::Clean
    }
    Err(e) => {
      eprintln!("{} Invalid table JSON spec: {}", "[ERR]".red().bold(), e);
      ExitStatus::Error
    }
  }
}
