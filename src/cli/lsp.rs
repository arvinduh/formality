//! CLI argument definitions and adapter for `fml lsp`.
//!
//! Owns argument definitions for the Language Server Protocol mode and
//! delegates execution to [`crate::engine::lsp`].

use std::path;

use clap;

use crate::engine::lsp;
use crate::errors;

/// Arguments for `fml lsp`.
#[derive(clap::Args, Clone, Debug)]
pub struct Args;

/// Executes the `lsp` command.
#[must_use]
pub fn run(_args: Args, root: Option<&path::Path>) -> errors::ExitStatus {
  lsp::run(root)
}
