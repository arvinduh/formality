use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
  name = "fml",
  author,
  version,
  about = "One CLI to format, lint, and sync configs across all languages",
  long_about = "formality (fml) orchestrates the best-in-class formatters and linters across Rust, Python, C/C++, Markdown, YAML, JSON, TOML, and Typst using a single canonical config."
)]
pub struct Cli {
  /// Custom path to formality config (formality.toml / .formality.toml)
  #[arg(short = 'c', long, global = true, value_name = "FILE")]
  pub config: Option<PathBuf>,

  /// Target workspace root (defaults to current working directory)
  #[arg(short = 'w', long, global = true, value_name = "DIR")]
  pub root: Option<PathBuf>,

  #[command(subcommand)]
  pub command: Commands,
}

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
    /// Automatically apply available lint fixes
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

  /// Sync native tool configs (.rustfmt.toml, ruff.toml, .clang-format, etc.) from canonical globals
  #[command(name = "sync", alias = "sync-config")]
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
  #[command(name = "list")]
  List,

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
}
