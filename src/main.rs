//! The `fml` binary: the process host.
//!
//! Owns process-level side effects: parsing arguments, installing the
//! logger, and mapping the command's outcome onto an exit code: 0 clean, 1
//! violations, 2 an operational failure. Commands live in `cli`.

mod cli;

use std::env;
use std::process;

use clap::Parser;
use env_logger;
use log;

fn main() -> process::ExitCode {
  // The installer otherwise adds its directory to PATH in shell rc files
  // when it is not on PATH; replacing a binary in place must not.
  // SAFETY: no other thread exists yet to read the environment.
  unsafe { env::set_var("FML_NO_MODIFY_PATH", "1") };
  let args = cli::Args::parse();
  let level = match args.verbose {
    0 => log::LevelFilter::Warn,
    1 => log::LevelFilter::Info,
    _ => log::LevelFilter::Debug,
  };
  env_logger::Builder::new()
    .filter_level(level)
    .parse_default_env()
    .format_timestamp(None)
    .format_target(false)
    .init();
  match args.run() {
    Ok(cli::Status::Clean) => process::ExitCode::SUCCESS,
    Ok(cli::Status::Violations) => process::ExitCode::from(1),
    Ok(cli::Status::Failed) => process::ExitCode::from(2),
    Err(err) => {
      #[expect(clippy::print_stderr, reason = "fml's error output")]
      {
        eprintln!("[ERROR] {err}");
      }
      process::ExitCode::from(2)
    }
  }
}
