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

    /// Removed in v0.3.0: `--install` lives only on `fml doctor` now.
    ///
    /// Declared hidden, not deleted outright, so [`Cli::validate`] can
    /// reject it by name with a message pointing at `fml doctor --install`
    /// instead of clap's bare "unexpected argument".
    #[arg(short = 'i', long, hide = true)]
    install: bool,

    /// A missing required tool alone does not fail the run (still reported
    /// in the table and the summary's "(allowed)" marker); a real violation
    /// or execution error still exits non-zero
    #[arg(long)]
    allow_missing: bool,

    /// Optional paths or files to target
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
  },

  /// Lint source files. Never writes -- use `fml fix` to apply fixes
  Lint {
    /// Removed in v0.3.0: `fml fix` is the only spelling now.
    ///
    /// Declared hidden, not deleted outright, so [`Cli::validate`] can
    /// reject it by name with a message pointing at `fml fix` instead of
    /// clap's bare "unexpected argument".
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

    /// Removed in v0.3.0: `--install` lives only on `fml doctor` now.
    ///
    /// Declared hidden, not deleted outright, so [`Cli::validate`] can
    /// reject it by name with a message pointing at `fml doctor --install`
    /// instead of clap's bare "unexpected argument".
    #[arg(short = 'i', long, hide = true)]
    install: bool,

    /// A missing required tool alone does not fail the run (still reported
    /// in the table and the summary's "(allowed)" marker); a real violation
    /// or execution error still exits non-zero
    #[arg(long)]
    allow_missing: bool,

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

    /// Removed in v0.3.0: `--install` lives only on `fml doctor` now.
    ///
    /// Declared hidden, not deleted outright, so [`Cli::validate`] can
    /// reject it by name with a message pointing at `fml doctor --install`
    /// instead of clap's bare "unexpected argument".
    #[arg(short = 'i', long, hide = true)]
    install: bool,

    /// A missing required tool alone does not fail the run (still reported
    /// in the table and the summary's "(allowed)" marker); a real violation
    /// or execution error still exits non-zero
    #[arg(long)]
    allow_missing: bool,

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

  /// Removed in v0.3.0: `fml doctor --install` is the only spelling now.
  ///
  /// Declared hidden, not deleted outright, so [`Cli::validate`] can reject
  /// it by name with a message pointing at `fml doctor --install` instead
  /// of clap's bare "unexpected argument". `--all` stays declared for the
  /// same reason: `fml install --all` must reach the tailored error too,
  /// not fail earlier on an unknown flag.
  #[command(hide = true)]
  Install {
    /// Install tools for all supported language surfaces
    #[arg(short = 'a', long)]
    all: bool,
  },

  /// Scaffold a new formality.toml or update the schema pin in an existing one
  Init {
    /// Overwrite existing configuration file if it already exists
    #[arg(short = 'f', long)]
    force: bool,

    /// Create hidden config file (.formality.toml) instead of formality.toml
    #[arg(long)]
    hidden: bool,
  },

  /// Removed in v0.3.0: `fml doctor` is the only spelling now.
  ///
  /// Declared hidden, not deleted outright, so [`Cli::validate`] can reject
  /// it by name with a message pointing at `fml doctor` instead of clap's
  /// bare "unexpected argument". Both spellings (`list-surfaces` and the
  /// `surfaces` alias) parse to this one variant, so rejecting the variant
  /// rejects both.
  #[command(name = "list-surfaces", alias = "surfaces", hide = true)]
  ListSurfaces,

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
    output: Option<PathBuf>,
  },

  /// Start formality as an LSP server (stdio transport)
  ///
  /// A document formatter and diagnostics publisher: `textDocument/formatting`
  /// runs `fml fmt` on the requested file; `did_save` / `did_open` run
  /// `fml lint` (or a structured per-surface parser) to publish diagnostics;
  /// `did_change_watched_files` invalidates the cached `formality.toml`.
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

  /// Removed in v0.3.0: use the `fml::ui::table` library API directly.
  ///
  /// Declared hidden, not deleted outright, so [`Cli::validate`] can reject
  /// it by name with a message pointing at the library API instead of
  /// clap's bare "unexpected argument".
  #[command(hide = true)]
  Table {
    /// Table specification JSON string (reads from stdin if omitted)
    #[arg(long)]
    json: Option<String>,
  },

  /// Removed in v0.3.0: `fml init` is the only spelling now.
  ///
  /// Declared hidden, not deleted outright, so [`Cli::validate`] can reject
  /// it by name with a message pointing at `fml init` instead of clap's
  /// bare "unexpected argument". `command` stays declared for the same
  /// reason: `fml migrate schema` must reach the tailored error too, not
  /// fail earlier on an unrecognized subcommand.
  #[command(hide = true)]
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

  /// Validates flag combinations that are parseable but meaningless, and
  /// flags that were removed outright but are kept declared (hidden) so
  /// they can be rejected by name.
  ///
  /// - `fml lint --check`. `--check` selects the report-only mode, and
  ///   `fml lint` is *always* report-only, so the flag is clutter rather
  ///   than a no-op and is rejected outright. It is declared as a hidden
  ///   arg purely so this can explain why; left undeclared, clap answers
  ///   with "unexpected argument '--check' found" and a "to pass '--check'
  ///   as a value, use '-- --check'" tip that points the user somewhere
  ///   actively wrong.
  /// - `fml fmt`/`fml lint`/`fml fix --install` (or `-i`). Removed in
  ///   v0.3.0: `--install` provisions the machine, which is `fml doctor`'s
  ///   one concern now, not a run command's. Declared hidden for the same
  ///   reason as `--check` above: so the error can name `fml doctor
  ///   --install` instead of clap's bare "unexpected argument".
  /// - `fml lint --fix`, `fml list-surfaces`/`fml surfaces`, `fml table`,
  ///   `fml install`. All removed outright in v0.3.0 (#255); each is
  ///   declared hidden for the same by-name-rejection reason as the flags
  ///   above, reusing the same mechanism rather than inventing a second one
  ///   (see #282).
  /// - `fml migrate`/`fml migrate schema`. Removed in v0.3.0 (#299):
  ///   `fml init` runs the same `apply_schema_pin` operation and also
  ///   scaffolds a config when none exists, so nothing `migrate schema` did
  ///   is missing. Declared hidden for the same by-name-rejection reason as
  ///   the rest of this list.
  ///
  /// # Errors
  ///
  /// Returns a [`clap::Error`] describing the rejected combination.
  ///
  /// # Panics
  ///
  /// Panics if the `lint`/`fmt`/`fix`/`list-surfaces`/`table`/`install`/
  /// `migrate` subcommand is missing from [`Commands`] — they are declared
  /// directly above, so this is a "the enum was edited without updating
  /// this" assertion, not a runtime condition.
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

    if let Commands::Lint { fix: true, .. } = &self.command {
      let mut cmd = Self::command();
      cmd.build();
      let lint = cmd
        .find_subcommand_mut("lint")
        .expect("`lint` subcommand is declared above");
      return Err(lint.error(
        clap::error::ErrorKind::ArgumentConflict,
        "`--fix` was removed from `fml lint` in v0.3.0.\n       \
         Use `fml fix` instead — it applies the same lint fixes and then \
         reformats, which `fml lint --fix` never did.",
      ));
    }

    let removed_install_subcommand = match &self.command {
      Commands::Fmt { install: true, .. } => Some(("fmt", "fml fmt")),
      Commands::Lint { install: true, .. } => Some(("lint", "fml lint")),
      Commands::Fix { install: true, .. } => Some(("fix", "fml fix")),
      _ => None,
    };
    if let Some((subcommand_name, command_name)) = removed_install_subcommand {
      let mut cmd = Self::command();
      cmd.build();
      let sub = cmd.find_subcommand_mut(subcommand_name).unwrap_or_else(|| {
        panic!("`{subcommand_name}` subcommand is declared above")
      });
      return Err(sub.error(
        clap::error::ErrorKind::ArgumentConflict,
        format!(
          "`--install` was removed from `{command_name}`.\n       \
           Provision tools with `fml doctor --install`, then run `{command_name}`.",
        ),
      ));
    }

    if let Commands::ListSurfaces = &self.command {
      // Either spelling (`list-surfaces` or the `surfaces` alias) parses to
      // this one variant; name whichever one the user actually typed.
      let spelling = if std::env::args().any(|a| a == "surfaces") {
        "fml surfaces"
      } else {
        "fml list-surfaces"
      };
      let mut cmd = Self::command();
      cmd.build();
      let sub = cmd
        .find_subcommand_mut("list-surfaces")
        .expect("`list-surfaces` subcommand is declared above");
      return Err(sub.error(
        clap::error::ErrorKind::ArgumentConflict,
        format!(
          "`{spelling}` was removed in v0.3.0.\n       Use `fml doctor` instead.",
        ),
      ));
    }

    if let Commands::Install { .. } = &self.command {
      let mut cmd = Self::command();
      cmd.build();
      let sub = cmd
        .find_subcommand_mut("install")
        .expect("`install` subcommand is declared above");
      return Err(sub.error(
        clap::error::ErrorKind::ArgumentConflict,
        "`fml install` was removed in v0.3.0.\n       \
         Use `fml doctor --install` instead.",
      ));
    }

    if let Commands::Table { .. } = &self.command {
      let mut cmd = Self::command();
      cmd.build();
      let sub = cmd
        .find_subcommand_mut("table")
        .expect("`table` subcommand is declared above");
      return Err(sub.error(
        clap::error::ErrorKind::ArgumentConflict,
        "`fml table` was removed in v0.3.0.\n       \
         Use the `fml::ui::table` library API directly instead.",
      ));
    }

    if let Commands::Migrate { .. } = &self.command {
      let mut cmd = Self::command();
      cmd.build();
      let sub = cmd
        .find_subcommand_mut("migrate")
        .expect("`migrate` subcommand is declared above");
      return Err(sub.error(
        clap::error::ErrorKind::ArgumentConflict,
        "`fml migrate` was removed in v0.3.0.\n       \
         Use `fml init` instead — it applies the same schema-pin update.",
      ));
    }

    Ok(())
  }
}

/// Subcommands of `fml migrate`.
///
/// `fml migrate` itself was removed in v0.3.0 (#299); this stays declared
/// (hidden) only so `fml migrate schema` still parses and reaches
/// [`Cli::validate`]'s tailored rejection instead of failing earlier on an
/// unrecognized subcommand.
#[derive(Subcommand, Debug)]
#[command(hide = true)]
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
  fn test_list_surfaces_and_surfaces_are_rejected_with_a_tailored_error() {
    // Both spellings still parse (declared hidden) so `validate` can name
    // the removed spelling by which one the user actually typed.
    let cli = Cli::try_parse_from(["fml", "list-surfaces"])
      .expect("list-surfaces must parse so validate can reject it by name");
    assert!(matches!(cli.command, Commands::ListSurfaces));
    let err = cli
      .validate()
      .expect_err("`fml list-surfaces` must be an error");
    let rendered = err.to_string();
    // Assert on the message's own sentence, not a bare substring: clap's
    // `Usage: fml list-surfaces [OPTIONS]` line contains
    // "fml list-surfaces" whatever the message says, so
    // `contains("fml list-surfaces")` passes even when the spelling branch
    // below picks the wrong name.
    assert!(
      rendered.contains("`fml list-surfaces` was removed"),
      "error should name the spelling actually typed, got:\n{rendered}"
    );
    assert!(
      !rendered.contains("`fml surfaces` was removed"),
      "error must not name the alias when `list-surfaces` was typed, got:\n{rendered}"
    );
    assert!(
      rendered.contains("fml doctor"),
      "error should name the replacement, got:\n{rendered}"
    );

    // The alias also parses to the same variant and is rejected the same
    // way. Which literal spelling the message names depends on the real
    // process argv (`std::env::args()`), not the parsed `Cli` here, so
    // that half is exercised against the built binary instead — see
    // `test_deprecated_list_surfaces_and_surfaces_are_rejected` in
    // tests/integration_tests.rs.
    let cli_alias = Cli::try_parse_from(["fml", "surfaces"])
      .expect("surfaces must parse so validate can reject it by name");
    assert!(matches!(cli_alias.command, Commands::ListSurfaces));
    let err_alias = cli_alias
      .validate()
      .expect_err("`fml surfaces` must be an error");
    assert!(
      err_alias.to_string().contains("was removed"),
      "error should say it was removed, got:\n{err_alias}"
    );
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
  fn test_lint_fix_is_rejected_with_a_tailored_error() {
    // `--fix` still parses (declared hidden) so `validate` can name the
    // replacement instead of clap's bare "unexpected argument".
    let cli = Cli::try_parse_from(["fml", "lint", "--fix"])
      .expect("--fix must parse so validate can reject it by name");
    let err = cli
      .validate()
      .expect_err("`fml lint --fix` must be an error");
    let rendered = err.to_string();
    assert!(
      rendered.contains("was removed"),
      "error should say `--fix` was removed, got:\n{rendered}"
    );
    assert!(
      rendered.contains("fml fix"),
      "error should name the replacement, got:\n{rendered}"
    );

    let mut cmd = Cli::command();
    cmd.build();
    let help = cmd
      .find_subcommand_mut("lint")
      .expect("lint subcommand")
      .render_help()
      .to_string();
    assert!(
      !help.contains("--fix"),
      "a removed spelling should not be advertised in --help, got:
{help}"
    );
    assert!(
      !help.contains("--check"),
      "`fml lint` has no mode flag to advertise, got:
{help}"
    );
  }

  #[test]
  fn test_migrate_schema_is_rejected_with_a_tailored_error() {
    // Still parses (declared hidden) so `validate` can name `fml init`
    // instead of clap's bare "unexpected argument".
    let cli = Cli::try_parse_from(["fml", "migrate", "schema"])
      .expect("migrate schema must parse so validate can reject it by name");
    assert!(matches!(
      cli.command,
      Commands::Migrate {
        command: MigrateCommands::Schema
      }
    ));
    let err = cli
      .validate()
      .expect_err("`fml migrate schema` must be an error");
    let rendered = err.to_string();
    assert!(
      rendered.contains("`fml migrate` was removed"),
      "error should say `fml migrate` was removed, got:\n{rendered}"
    );
    assert!(
      rendered.contains("fml init"),
      "error should name the replacement, got:\n{rendered}"
    );
    assert!(
      !rendered.contains("unexpected argument"),
      "error must not fall back to clap's bare rejection, got:\n{rendered}"
    );

    let mut cmd = Cli::command();
    cmd.build();
    let help = cmd.render_help().to_string();
    let visible_subcommands: Vec<&str> = cmd
      .get_subcommands()
      .filter(|c| !c.is_hide_set())
      .map(clap::Command::get_name)
      .collect();
    assert!(
      !visible_subcommands.contains(&"migrate"),
      "removed `migrate` should be hidden from subcommand list"
    );
    assert!(
      !help.lines().any(|l| l.trim_start().starts_with("migrate ")),
      "removed `migrate` should not appear in --help, got:\n{help}"
    );
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
      } if p == std::path::Path::new("schema.json")
    ));

    let mut cmd = Cli::command();
    cmd.build();
    let visible_subcommands: Vec<&str> = cmd
      .get_subcommands()
      .filter(|c| !c.is_hide_set())
      .map(|c| c.get_name())
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
  fn test_table_is_rejected_with_a_tailored_error() {
    // Still parses (declared hidden) so `validate` can name the library API
    // instead of clap's bare "unexpected argument".
    let cli = Cli::try_parse_from(["fml", "table"])
      .expect("table must parse so validate can reject it by name");
    assert!(matches!(cli.command, Commands::Table { json: None }));
    let err = cli.validate().expect_err("`fml table` must be an error");
    let rendered = err.to_string();
    assert!(
      rendered.contains("was removed"),
      "error should say `fml table` was removed, got:\n{rendered}"
    );
    assert!(
      rendered.contains("fml::ui::table"),
      "error should name the library API replacement, got:\n{rendered}"
    );

    let cli_json = Cli::try_parse_from(["fml", "table", "--json", "{}"])
      .expect("table --json must parse so validate can reject it by name");
    assert!(matches!(
      cli_json.command,
      Commands::Table {
        json: Some(ref s)
      } if s == "{}"
    ));
    assert!(cli_json.validate().is_err());

    let mut cmd = Cli::command();
    cmd.build();
    let help = cmd.render_help().to_string();
    assert!(
      !help.lines().any(|l| l.trim_start().starts_with("table ")),
      "removed `table` subcommand should not be advertised in --help, got:\n{help}"
    );
  }

  #[test]
  fn test_install_command_is_rejected_with_a_tailored_error() {
    // Still parses (declared hidden) so `validate` can name `fml doctor
    // --install` instead of clap's bare "unexpected argument".
    let cli_install = Cli::try_parse_from(["fml", "install"])
      .expect("install must parse so validate can reject it by name");
    assert!(matches!(
      cli_install.command,
      Commands::Install { all: false }
    ));
    let err = cli_install
      .validate()
      .expect_err("`fml install` must be an error");
    let rendered = err.to_string();
    assert!(
      rendered.contains("was removed"),
      "error should say `fml install` was removed, got:\n{rendered}"
    );
    assert!(
      rendered.contains("fml doctor --install"),
      "error should name the replacement, got:\n{rendered}"
    );
    assert!(
      !rendered.contains("unexpected argument"),
      "error must not fall back to clap's bare rejection, got:\n{rendered}"
    );

    // `--all` stays declared so `fml install --all` reaches the same
    // tailored error rather than failing earlier on an unknown flag.
    let cli_install_all = Cli::try_parse_from(["fml", "install", "-a"])
      .expect("install -a must parse so validate can reject it by name");
    assert!(matches!(
      cli_install_all.command,
      Commands::Install { all: true }
    ));
    assert!(
      cli_install_all
        .validate()
        .expect_err("`fml install --all` must be an error")
        .to_string()
        .contains("fml doctor --install"),
      "`fml install --all` should get the same tailored error"
    );

    let mut cmd = Cli::command();
    cmd.build();
    let help = cmd.render_help().to_string();

    let visible_subcommands: Vec<&str> = cmd
      .get_subcommands()
      .filter(|c| !c.is_hide_set())
      .map(|c| c.get_name())
      .collect();
    assert!(
      !visible_subcommands.contains(&"install"),
      "removed `install` should be hidden from subcommand list"
    );
    assert!(
      !help.lines().any(|l| l.trim_start().starts_with("install ")),
      "removed `install` should not appear in --help, got:\n{help}"
    );
  }

  #[test]
  fn test_removed_list_surfaces_and_surfaces_are_hidden_from_help() {
    let mut cmd = Cli::command();
    cmd.build();
    let help = cmd.render_help().to_string();

    let visible_subcommands: Vec<&str> = cmd
      .get_subcommands()
      .filter(|c| !c.is_hide_set())
      .map(|c| c.get_name())
      .collect();
    assert!(
      !visible_subcommands.contains(&"list-surfaces"),
      "removed `list-surfaces` should be hidden from subcommand list"
    );
    assert!(
      !help
        .lines()
        .any(|l| l.trim_start().starts_with("list-surfaces ")),
      "removed `list-surfaces` should not appear in --help, got:\n{help}"
    );
    assert!(
      !help
        .lines()
        .any(|l| l.trim_start().starts_with("surfaces ")),
      "removed alias `surfaces` should not appear in --help, got:\n{help}"
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
        !help.contains("--install"),
        "`fml {name} --help` must not advertise removed `--install` (v0.3.0) — see #282, got:
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
    assert!(
      !lint_help.contains("--install"),
      "`fml lint --help` must not advertise removed `--install` (v0.3.0) — see #282, got:
{lint_help}"
    );
  }

  #[test]
  fn test_install_removed_from_fmt_lint_fix_names_doctor_install() {
    for (argv, subcommand, command_name) in [
      (["fml", "fmt", "--install"], "fmt", "fml fmt"),
      (["fml", "lint", "--install"], "lint", "fml lint"),
      (["fml", "fix", "--install"], "fix", "fml fix"),
    ] {
      let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| {
        panic!("`{subcommand} --install` must still parse so validate can reject it by name: {e}")
      });
      let err = cli.validate().expect_err(&format!(
        "`{command_name} --install` must be rejected by validate()"
      ));
      let rendered = err.to_string();
      assert!(
        rendered.contains("was removed"),
        "error should say `--install` was removed from `{command_name}`, got:\n{rendered}"
      );
      assert!(
        rendered.contains("fml doctor --install"),
        "error should name `fml doctor --install` as the replacement, got:\n{rendered}"
      );
      assert!(
        rendered.contains(command_name),
        "error should name `{command_name}` itself, got:\n{rendered}"
      );
    }

    // `-i` is the same removed flag under its short spelling.
    let cli = Cli::try_parse_from(["fml", "fmt", "-i"]).unwrap();
    let err = cli.validate().expect_err("`fml fmt -i` must be rejected");
    assert!(err.to_string().contains("fml doctor --install"));
  }

  #[test]
  fn test_fmt_lint_fix_without_install_validate() {
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
