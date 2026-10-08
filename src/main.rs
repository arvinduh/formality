//! The `fml` binary entry point and process host.
//!
//! Parses arguments, installs `env_logger` (`RUST_LOG` overrides `-v`), runs the command, and maps its
//! [`runner::ExitStatus`] to the process exit code. `cli` is declared here,
//! not in the library, so the library cannot depend on it.

mod cli;

use std::process;

use clap::Parser;
use env_logger;

use fml::engine::runner;

fn main() -> process::ExitCode {
  let cli = cli::Cli::parse();
  env_logger::Builder::new()
    .filter_level(cli.log_level())
    .parse_default_env()
    .init();
  match cli.run() {
    runner::ExitStatus::Clean => process::ExitCode::SUCCESS,
    runner::ExitStatus::Violations => process::ExitCode::from(1),
    runner::ExitStatus::Error => process::ExitCode::from(2),
  }
}
