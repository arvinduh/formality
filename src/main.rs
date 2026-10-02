//! `fml` binary entry point — thin wrapper delegating to [`fml::run`], which
//! owns argument parsing and command dispatch.
//!
//! This file is the process host: it alone turns the library's
//! [`fml::errors::ExitStatus`] into the process exit code, by returning it from
//! `main` so destructors run on every exit path.

use std::process;

fn main() -> process::ExitCode {
  match fml::run() {
    fml::errors::ExitStatus::Clean => process::ExitCode::SUCCESS,
    fml::errors::ExitStatus::Violations => process::ExitCode::from(1),
    fml::errors::ExitStatus::Error => process::ExitCode::from(2),
  }
}
