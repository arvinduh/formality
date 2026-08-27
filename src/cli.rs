//! `clap`-derived CLI argument definitions ([`Cli`], [`Commands`]) — the
//! single source of truth for every `fml` subcommand's flags, parsed once in
//! [`crate::run`] and dispatched from [`crate::run_command_inner`].

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Top-level command-line arguments parser for formality.
#[derive(Parser, Debug)]
#[command(
  name = "fml",
  author,
  version,
  about = "One CLI to format, lint, and sync configs across all languages",
  long_about = "formality (fml) orchestrates the best-in-class formatters and linters across Rust, Python, C/C++, Java, Go, JavaScript/TypeScript, Kotlin, Markdown, YAML, JSON, TOML, and Typst using a single canonical config."
)]
pub struct Cli {
  /// Custom path to formality config (formality.toml / .formality.toml)
  #[arg(short = 'c', long, global = true, value_name = "FILE")]
  pub config: Option<PathBuf>,

  /// Target workspace root (defaults to current working directory)
  #[arg(short = 'w', long, global = true, value_name = "DIR")]
  pub root: Option<PathBuf>,

  /// The subcommand to execute.
  #[command(subcommand)]
  pub command: Commands,
}

/// Available subcommands for formality CLI.
#[derive(Subcommand, Debug)]
pub enum Commands {
  /// Format source files across detected or specified surfaces
  Fmt {
    /// Check formatting without writing changes to disk
    #[arg(long)]
    check: bool,

    /// Only format files staged for git commit
    #[arg(short = 's', long)]
    staged: bool,

    /// Only format modified uncommitted files in git
    #[arg(long)]
    changed: bool,

    /// Filter by specific language surface (e.g. rust, python, markdown)
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,

    /// Auto-install any missing tool dependencies before formatting
    #[arg(short = 'i', long)]
    install: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
  },

  /// Lint source files across detected or specified surfaces
  Lint {
    /// Automatically apply available lint fixes (does not reformat; see `fml fix` for lint+format together)
    #[arg(long)]
    fix: bool,

    /// Only lint files staged for git commit
    #[arg(short = 's', long)]
    staged: bool,

    /// Only lint modified uncommitted files in git
    #[arg(long)]
    changed: bool,

    /// Filter by specific language surface (e.g. rust, python, markdown)
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,

    /// Auto-install any missing tool dependencies before linting
    #[arg(short = 'i', long)]
    install: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
  },

  /// Automatically fix lint violations and reformat code (equivalent to `fml lint --fix` followed by `fml fmt`)
  Fix {
    /// Only fix files staged for git commit
    #[arg(short = 's', long)]
    staged: bool,

    /// Only fix modified uncommitted files in git
    #[arg(long)]
    changed: bool,

    /// Filter by specific language surface (e.g. rust, python, markdown)
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,

    /// Auto-install any missing tool dependencies before fixing
    #[arg(short = 'i', long)]
    install: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
  },

  /// Sync native tool configs (.rustfmt.toml, ruff.toml, .clang-format, etc.) from canonical globals
  Sync {
    /// Check whether native tool configs are in sync without writing changes
    #[arg(long)]
    check: bool,

    /// Filter by specific language surface
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,
  },

  /// Diagnose installed toolchains and binaries with installation hints
  Doctor {
    /// Inspect all supported surfaces regardless of project detection
    #[arg(short = 'a', long)]
    all: bool,

    /// Automatically install missing toolchains using available package managers
    #[arg(short = 'i', long)]
    install: bool,
  },

  /// Automatically install missing toolchains for detected surfaces
  Install {
    /// Install tools for all supported language surfaces
    #[arg(short = 'a', long)]
    all: bool,
  },

  /// Scaffold a new formality.toml configuration in the current directory
  Init {
    /// Overwrite existing configuration file if it already exists
    #[arg(short = 'f', long)]
    force: bool,

    /// Create hidden config file (.formality.toml) instead of formality.toml
    #[arg(long)]
    hidden: bool,
  },

  /// List all supported surfaces and indicate which are detected in this project
  #[command(name = "list-surfaces", alias = "surfaces")]
  ListSurfaces,

  /// Output the JSON Schema for formality.toml to stdout or file
  Schema {
    /// Optional file path to write the JSON schema to (defaults to stdout)
    #[arg(short = 'o', long, value_name = "FILE")]
    output: Option<PathBuf>,
  },

  /// Start formality as an LSP server (stdio transport)
  ///
  /// Acts as a passthrough Language Server that delegates to the underlying
  /// per-language LSP servers (rust-analyzer, pyright, clangd, …) while
  /// adding formality's own capabilities on top: unified formatting via
  /// `fml fmt`, cross-language diagnostics via `fml lint`, and config-sync
  /// notifications when formality.toml changes.
  ///
  /// Editors connect via stdio (the default transport for most editors).
  /// The server auto-discovers the active surfaces and only spawns child
  /// LSP processes for languages present in the workspace.
  Lsp,

  /// Render an opinionated semantic terminal table from JSON specification
  Table {
    /// Table specification JSON string (reads from stdin if omitted)
    #[arg(long)]
    json: Option<String>,
  },

  /// Migrate project files to match the current formality release
  Migrate {
    /// Which migration to run.
    #[command(subcommand)]
    command: MigrateCommands,
  },
}

/// Subcommands of `fml migrate`.
#[derive(Subcommand, Debug)]
pub enum MigrateCommands {
  /// Rewrite the `#:schema` directive in formality.toml / .formality.toml to
  /// point at the current release's schema URL, leaving the rest of the file
  /// untouched
  Schema,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_list_surfaces_subcommand_and_alias() {
    let cli = Cli::try_parse_from(["fml", "list-surfaces"]).unwrap();
    assert!(matches!(cli.command, Commands::ListSurfaces));

    let cli_alias = Cli::try_parse_from(["fml", "surfaces"]).unwrap();
    assert!(matches!(cli_alias.command, Commands::ListSurfaces));
  }
}
