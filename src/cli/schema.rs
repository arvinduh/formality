//! CLI argument definitions and adapter for `fml schema`.
//!
//! Owns argument definitions for schema output and delegates execution to
//! [`crate::engine::schema`].

use std::path;

use clap;

use crate::engine::schema;
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
  schema::run(args.output)
}
