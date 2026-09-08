//! `clap`-derived CLI argument definitions ([`Cli`], [`Commands`]) — the
//! single source of truth for every `fml` subcommand's flags, parsed once in
//! [`crate::run`] and dispatched from [`crate::run_command_inner`].

use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

/// Top-level command-line arguments parser for formality.
#[derive(Parser, Debug)]
#[command(
  name = "formality",
  bin_name = "fml",
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
  /// Format source files. Writes changes; --check reports without writing
  Fmt {
    /// Report what would be reformatted, without writing
    #[arg(long)]
    check: bool,

    /// Only act on files staged for git commit
    #[arg(short = 's', long)]
    staged: bool,

    /// Only act on modified uncommitted files in git
    #[arg(long)]
    changed: bool,

    /// Filter by specific language surface (e.g. rust, python, markdown)
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,

    /// Auto-install any missing tool dependencies first
    #[arg(short = 'i', long)]
    install: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
  },

  /// Lint source files. Never writes -- use `fml fix` to apply fixes
  Lint {
    /// Deprecated: use `fml fix`. Kept working for one minor release.
    ///
    /// Hidden from `--help` deliberately: it is on its way out, so help
    /// advertises only the spelling we want adopted. It still parses, and
    /// dispatches to the `fix` plan (lint fixes *and* format) after
    /// printing the shared deprecation notice.
    #[arg(long, hide = true)]
    fix: bool,

    /// Rejected, not a no-op: `fml lint` never writes, so a mode flag on it
    /// would be meaningless clutter. Declared only so the error names the
    /// real reason instead of clap's misleading "to pass '--check' as a
    /// value, use '-- --check'" tip; validated in [`Cli::parse_checked`].
    #[arg(long, hide = true)]
    check: bool,

    /// Only act on files staged for git commit
    #[arg(short = 's', long)]
    staged: bool,

    /// Only act on modified uncommitted files in git
    #[arg(long)]
    changed: bool,

    /// Filter by specific language surface (e.g. rust, python, markdown)
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,

    /// Auto-install any missing tool dependencies first
    #[arg(short = 'i', long)]
    install: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
  },

  /// Apply lint fixes, then reformat. Writes changes; --check reports without writing
  Fix {
    /// Report whether `fml fix` would change anything, without writing
    #[arg(long)]
    check: bool,

    /// Only act on files staged for git commit
    #[arg(short = 's', long)]
    staged: bool,

    /// Only act on modified uncommitted files in git
    #[arg(long)]
    changed: bool,

    /// Filter by specific language surface (e.g. rust, python, markdown)
    #[arg(short = 'l', long = "lang", value_name = "LANG")]
    lang: Vec<String>,

    /// Auto-install any missing tool dependencies first
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

impl Cli {
  /// Parses `std::env::args()` and rejects flag combinations clap's derive
  /// cannot express, exiting with clap's own error rendering.
  ///
  /// Used by [`crate::run`] in place of a bare [`Parser::parse`].
  #[must_use]
  pub fn parse_checked() -> Self {
    let cli = Self::parse();
    if let Err(e) = cli.validate() {
      e.exit();
    }
    cli
  }

  /// Validates flag combinations that are parseable but meaningless.
  ///
  /// Currently one rule: `fml lint --check`. `--check` selects the
  /// report-only mode, and `fml lint` is *always* report-only, so the flag
  /// is clutter rather than a no-op and is rejected outright. It is
  /// declared as a hidden arg purely so this can explain why; left
  /// undeclared, clap answers with "unexpected argument '--check' found"
  /// and a "to pass '--check' as a value, use '-- --check'" tip that points
  /// the user somewhere actively wrong.
  ///
  /// # Errors
  ///
  /// Returns a [`clap::Error`] describing the rejected combination.
  ///
  /// # Panics
  ///
  /// Panics if the `lint` subcommand is missing from [`Commands`] — it is
  /// declared directly above, so this is a "the enum was edited without
  /// updating this" assertion, not a runtime condition.
  pub fn validate(&self) -> Result<(), clap::Error> {
    if let Commands::Lint { check: true, .. } = &self.command {
      let mut cmd = Self::command();
      cmd.build();
      let lint = cmd
        .find_subcommand_mut("lint")
        .expect("`lint` subcommand is declared above");
      return Err(lint.error(
        clap::error::ErrorKind::ArgumentConflict,
        concat!(
          "`fml lint` never writes, so `--check` has no meaning.\n\n",
          "  tip: `fml lint` is already report-only. For a read-only run ",
          "of the fix pipeline, use `fml fix --check`.",
        ),
      ));
    }
    Ok(())
  }
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
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use clap::CommandFactory;

  #[test]
  fn test_list_surfaces_subcommand_and_alias() {
    let cli = Cli::try_parse_from(["fml", "list-surfaces"]).unwrap();
    assert!(matches!(cli.command, Commands::ListSurfaces));

    let cli_alias = Cli::try_parse_from(["fml", "surfaces"]).unwrap();
    assert!(matches!(cli_alias.command, Commands::ListSurfaces));
  }

  #[test]
  fn test_lint_check_is_rejected_with_a_tailored_error() {
    // `--check` parses (it is declared hidden) so that `validate` can
    // explain *why* it is refused. Clap's own "unexpected argument" answer
    // suggests `-- --check`, which would silently pass `--check` through as
    // a path argument.
    let cli = Cli::try_parse_from(["fml", "lint", "--check"])
      .expect("--check must parse so validate can reject it by name");
    let err = cli
      .validate()
      .expect_err("`fml lint --check` must be an error");
    let rendered = err.to_string();
    assert!(
      rendered.contains("never writes"),
      "error should say why lint has no mode flag, got:
{rendered}"
    );
    assert!(
      rendered.contains("fml fix --check"),
      "error should name the spelling that does what the user wanted, got:
{rendered}"
    );
    assert!(
      !rendered.contains("-- --check"),
      "error must not reproduce clap's misleading passthrough tip, got:
{rendered}"
    );
  }

  #[test]
  fn test_lint_without_check_validates() {
    let cli = Cli::try_parse_from(["fml", "lint"]).unwrap();
    assert!(cli.validate().is_ok());
  }

  #[test]
  fn test_fix_accepts_check() {
    let cli = Cli::try_parse_from(["fml", "fix", "--check"]).unwrap();
    assert!(matches!(cli.command, Commands::Fix { check: true, .. }));
    assert!(cli.validate().is_ok());
  }

  #[test]
  fn test_deprecated_lint_fix_still_parses_but_is_hidden_from_help() {
    let cli = Cli::try_parse_from(["fml", "lint", "--fix"]).unwrap();
    assert!(matches!(cli.command, Commands::Lint { fix: true, .. }));
    assert!(cli.validate().is_ok());

    let mut cmd = Cli::command();
    cmd.build();
    let help = cmd
      .find_subcommand_mut("lint")
      .expect("lint subcommand")
      .render_help()
      .to_string();
    assert!(
      !help.contains("--fix"),
      "a deprecated spelling should not be advertised in --help, got:
{help}"
    );
    assert!(
      !help.contains("--check"),
      "`fml lint` has no mode flag to advertise, got:
{help}"
    );
  }

  #[test]
  fn test_mode_flag_help_is_consistent_across_the_three_commands() {
    // The `--check` help text is reviewed as a set (#118): every command
    // that has it describes it as *reporting*, and the shared selection
    // flags read identically everywhere.
    let mut cmd = Cli::command();
    cmd.build();
    for (name, expected_check) in [
      ("fmt", "Report what would be reformatted, without writing"),
      (
        "fix",
        "Report whether `fml fix` would change anything, without writing",
      ),
    ] {
      let help = cmd
        .find_subcommand_mut(name)
        .expect("subcommand")
        .render_help()
        .to_string();
      assert!(
        help.contains(expected_check),
        "`fml {name} --help` should describe --check as reporting, got:
{help}"
      );
      assert!(
        help.contains("Only act on files staged for git commit"),
        "`fml {name} --help` should use the shared --staged wording, got:
{help}"
      );
      assert!(
        help.contains("Auto-install any missing tool dependencies first"),
        "`fml {name} --help` should use the shared --install wording, got:
{help}"
      );
    }

    let lint_help = cmd
      .find_subcommand_mut("lint")
      .expect("lint subcommand")
      .render_help()
      .to_string();
    assert!(
      lint_help.contains("Only act on files staged for git commit"),
      "`fml lint --help` should use the shared --staged wording, got:
{lint_help}"
    );
  }

  #[test]
  fn test_product_name_is_formality() {
    let cmd = Cli::command();
    assert_eq!(
      cmd.get_name(),
      "formality",
      "clap command name should report the product name"
    );
  }

  #[test]
  fn test_bin_name_is_fml() {
    let cmd = Cli::command();
    assert_eq!(
      cmd.get_bin_name(),
      Some("fml"),
      "bin_name should stay as the executable / invocation name"
    );
  }

  #[test]
  fn test_version_output_reports_product_name() {
    let expected = format!("formality {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(Cli::command().render_version(), expected);
  }

  #[test]
  fn test_help_usage_line_uses_executable_name() {
    let help = Cli::command().render_help().to_string();
    assert!(
      help.contains("Usage: fml [OPTIONS] <COMMAND>"),
      "help usage line should invoke the executable name `fml`, got:\n{help}"
    );
  }
}
