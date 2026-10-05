//! `clap`-derived CLI argument definitions (`Cli`, `Commands`) — the single source of truth for every `fml` subcommand's flags.
//!
//! Owns argument schema definitions, flag parsing, and validation. Subcommand
//! handlers live in `fml::commands`, and top-level dispatch lives in
//! `crate` (the `fml` binary Process Host).

use std::path;

use clap;

/// Top-level command-line arguments parser for formality.
#[derive(clap::Parser, Debug)]
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
  pub config: Option<path::PathBuf>,

  /// Target workspace root (defaults to current working directory)
  #[arg(short = 'w', long, global = true, value_name = "DIR")]
  pub root: Option<path::PathBuf>,

  /// The subcommand to execute.
  #[command(subcommand)]
  pub command: Commands,
}

/// Available subcommands for formality CLI.
#[derive(clap::Subcommand, Debug)]
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

    /// A missing required tool alone does not fail the run (still reported
    /// in the table and the summary's "(allowed)" marker); a real violation
    /// or execution error still exits non-zero
    #[arg(long)]
    allow_missing: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<path::PathBuf>,
  },

  /// Lint source files. Never writes -- use `fml fix` to apply fixes
  Lint {
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

    /// A missing required tool alone does not fail the run (still reported
    /// in the table and the summary's "(allowed)" marker); a real violation
    /// or execution error still exits non-zero
    #[arg(long)]
    allow_missing: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<path::PathBuf>,
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

    /// A missing required tool alone does not fail the run (still reported
    /// in the table and the summary's "(allowed)" marker); a real violation
    /// or execution error still exits non-zero
    #[arg(long)]
    allow_missing: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<path::PathBuf>,
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

  /// Scaffold a new formality.toml
  Init {
    /// Overwrite existing configuration file if it already exists
    #[arg(short = 'f', long)]
    force: bool,

    /// Create hidden config file (.formality.toml) instead of formality.toml
    #[arg(long)]
    hidden: bool,
  },

  /// Write the JSON Schema for formality.toml to stdout or a file
  ///
  /// Briefly deprecated in favour of `UPDATE_SCHEMA=1 cargo test --test
  /// schema_drift`, un-deprecated in v0.3.0 (#255): that replacement needs
  /// a Rust toolchain *and* a checkout of this repository, so anyone who
  /// installed `fml` as a released binary could not run it. It also
  /// generates the published schema asset in the release pipeline, which a
  /// test cannot do.
  Schema {
    /// Optional file path to write the JSON schema to (defaults to stdout)
    #[arg(short = 'o', long, value_name = "FILE")]
    output: Option<path::PathBuf>,
  },

  /// Start formality as an LSP server (stdio transport)
  ///
  /// A document formatter and diagnostics publisher: `textDocument/formatting`
  /// runs `fml fmt` on the requested file; `did_save` / `did_open` run
  /// `fml lint` (or a structured per-surface parser) to publish diagnostics;
  /// `did_change_watched_files` reloads `formality.toml`, keeping the previous
  /// config when the new one is invalid.
  ///
  /// This is not a replacement for your language server — it does not spawn,
  /// proxy, or route requests to rust-analyzer, pyright, clangd, or any other
  /// LSP. Run it alongside your existing language server, with formality
  /// owning formatting and lint diagnostics and the other server owning
  /// everything else (hover, completion, go-to-definition, …). See
  /// README.md's "Editor setup" section for per-editor wiring.
  ///
  /// Editors connect via stdio (the default transport for most editors).
  Lsp,
}

impl Cli {
  /// Parses `std::env::args()` and rejects flag combinations clap's derive
  /// cannot express, exiting with clap's own error rendering.
  ///
  /// Used by the binary entry point in place of a bare [`clap::Parser::parse`].
  #[must_use]
  pub fn parse_checked() -> Self {
    let cli = <Self as clap::Parser>::parse();
    if let Err(e) = cli.validate() {
      e.exit();
    }
    cli
  }

  /// Rejects `fml lint --check`, a flag combination that parses but means
  /// nothing.
  ///
  /// `--check` selects the report-only mode, and `fml lint` is *always*
  /// report-only, so the flag is clutter rather than a no-op and is
  /// rejected outright. It is declared as a hidden arg purely so this can
  /// explain why; left undeclared, clap answers with "unexpected argument
  /// '--check' found" and a "to pass '--check' as a value, use '-- --check'"
  /// tip that points the user somewhere actively wrong.
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
      let mut cmd = <Self as clap::CommandFactory>::command();
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

#[cfg(test)]
mod tests {
  use super::*;
  use clap::CommandFactory;
  use clap::Parser;

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
  fn test_schema_parses_and_is_advertised_in_help() {
    // Un-deprecated in v0.3.0 (#255): it is a supported command, so it
    // must be visible in `--help` like any other.
    let cli = Cli::try_parse_from(["fml", "schema"]).unwrap();
    assert!(matches!(cli.command, Commands::Schema { output: None }));
    assert!(cli.validate().is_ok());

    let cli =
      Cli::try_parse_from(["fml", "schema", "-o", "schema.json"]).unwrap();
    assert!(matches!(
      cli.command,
      Commands::Schema {
        output: Some(ref p)
      } if p == path::Path::new("schema.json")
    ));

    let mut cmd = Cli::command();
    cmd.build();
    let visible_subcommands: Vec<&str> = cmd
      .get_subcommands()
      .filter(|c| !c.is_hide_set())
      .map(clap::Command::get_name)
      .collect();
    assert!(
      visible_subcommands.contains(&"schema"),
      "`schema` is supported and should be visible, got: {visible_subcommands:?}"
    );
    let help = cmd.render_help().to_string();
    assert!(
      help.lines().any(|l| l.trim_start().starts_with("schema ")),
      "supported `schema` subcommand should be advertised in --help, got:\n{help}"
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
  fn test_fmt_lint_fix_validate() {
    for argv in [["fml", "fmt"], ["fml", "lint"], ["fml", "fix"]] {
      let cli = Cli::try_parse_from(argv).unwrap();
      assert!(cli.validate().is_ok());
    }
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
