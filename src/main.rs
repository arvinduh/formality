//! The `fml` binary entry point and Process Host per rust-guide §3H.
//!
//! Parses CLI arguments with [`fml::cli::Cli::parse_checked`], delegates
//! execution to [`fml::cli::run`], and maps [`fml::errors::ExitStatus`] to
//! the process exit code.

use std::process;

use fml::cli;
use fml::errors;

fn main() -> process::ExitCode {
  let args = cli::Cli::parse_checked();
  match cli::run(args) {
    errors::ExitStatus::Clean => process::ExitCode::SUCCESS,
    errors::ExitStatus::Violations => process::ExitCode::from(1),
    errors::ExitStatus::Error => process::ExitCode::from(2),
  }
}
