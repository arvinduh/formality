//! `clap`-derived CLI argument definitions and top-level execution coordinator.
//!
//! Owns argument parsing, validation, terminal configuration, and command
//! dispatch to modular adapters under `crate::cli::*`.

/// CLI adapter for `fml doctor`.
pub mod doctor;
/// CLI adapter for `fml fix`.
pub mod fix;
/// CLI adapter for `fml fmt`.
pub mod fmt;
/// CLI adapter for `fml init`.
pub mod init;
/// CLI adapter for `fml lint`.
pub mod lint;
/// CLI adapter for `fml lsp`.
pub mod lsp;
/// CLI adapter for `fml schema`.
pub mod schema;
/// CLI adapter for `fml sync`.
pub mod sync;

use std::env;
use std::path;

use clap;
use colored;
use colored::Colorize;

use crate::config;
use crate::engine::update;
use crate::errors;
use crate::surfaces;
use crate::ui;

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
  Fmt(fmt::Args),

  /// Lint source files. Never writes -- use `fml fix` to apply fixes
  Lint(lint::Args),

  /// Apply lint fixes, then reformat. Writes changes; --check reports without writing
  Fix(fix::Args),

  /// Sync native tool configs (.rustfmt.toml, ruff.toml, .clang-format, etc.) from canonical globals
  Sync(sync::Args),

  /// Diagnose installed toolchains and binaries with installation hints
  Doctor(doctor::Args),

  /// Scaffold a new formality.toml
  Init(init::Args),

  /// Write the JSON Schema for formality.toml to stdout or a file
  ///
  /// Briefly deprecated in favour of `UPDATE_SCHEMA=1 cargo test --test
  /// schema_drift`, un-deprecated in v0.3.0 (#255): that replacement needs
  /// a Rust toolchain *and* a checkout of this repository, so anyone who
  /// installed `fml` as a released binary could not run it. It also
  /// generates the published schema asset in the release pipeline, which a
  /// test cannot do.
  Schema(schema::Args),

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
  Lsp(lsp::Args),
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
    if let Commands::Lint(lint::Args { check: true, .. }) = &self.command {
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

/// Dispatches the parsed CLI arguments to the corresponding library command.
#[must_use]
pub fn run(args: Cli) -> errors::ExitStatus {
  // NO_COLOR wins over every force-color signal, matching the precedence
  // `ui::table::Palette::detect` already applies to this crate's own escape
  // codes.
  if ui::no_color_requested() {
    colored::control::set_override(false);
  } else if ui::color_forced() {
    colored::control::set_override(true);
  }

  let root = resolve_root(args.root);
  let update_notifier = update::spawn_update_check();

  // The server loads and reports its own config at `initialize`.
  if let Commands::Lsp(lsp_args) = args.command {
    let status = lsp::run(lsp_args, Some(&root));
    update::print_update_notice(update_notifier);
    return status;
  }

  let project_config_path = config::find_project_config(&root);
  let (mut config, _config_path) =
    match config::FormalityConfig::load_layered_with_path(
      project_config_path.as_deref(),
    ) {
      Ok(res) => res,
      Err(e) => {
        errors::FormalityError::from(e).print_diagnostic();
        update::print_update_notice(update_notifier);
        return errors::ExitStatus::Error;
      }
    };

  if let Some(custom_cfg) = args.config {
    match config::FormalityConfig::load_file(&custom_cfg) {
      Ok(custom) => config.merge(custom),
      Err(e) => {
        errors::FormalityError::from(e).print_diagnostic();
        update::print_update_notice(update_notifier);
        return errors::ExitStatus::Error;
      }
    }
  }

  warn_unrecognized_lang_sections(&config);

  let status = dispatch(args.command, &root, &config);

  update::print_update_notice(update_notifier);
  status
}

/// Dispatches the subcommand against the loaded configuration.
fn dispatch(
  command: Commands,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  match command {
    Commands::Schema(args) => schema::run(args),
    Commands::Doctor(args) => doctor::run(args, root, config),
    Commands::Init(args) => init::run(args, root, config),
    Commands::Fmt(args) => fmt::run(args, root, config),
    Commands::Fix(args) => fix::run(args, root, config),
    Commands::Lint(args) => lint::run(args, root, config),
    Commands::Sync(args) => sync::run(&args, root, config),
    Commands::Lsp(_) => {
      unreachable!("`lsp` is dispatched before the config load")
    }
  }
}

/// Resolves `--root` (or the current directory when it is absent) to an
/// absolute path, so every command sees the same root however it was spelled.
#[must_use]
pub fn resolve_root(root: Option<path::PathBuf>) -> path::PathBuf {
  let root = root.unwrap_or_else(|| {
    env::current_dir().unwrap_or_else(|_| path::PathBuf::from("."))
  });
  path::absolute(&root).unwrap_or_else(|_| {
    env::current_dir().map_or_else(|_| root.clone(), |cwd| cwd.join(&root))
  })
}

/// Warns about any `[lang.X]` sections in the resolved config whose `X` is
/// not a recognized surface name or alias.
fn warn_unrecognized_lang_sections(config: &config::FormalityConfig) {
  let registry = surfaces::SurfaceRegistry::default();
  let unrecognized = config.unrecognized_lang_sections(&registry);
  if unrecognized.is_empty() {
    return;
  }

  for name in unrecognized {
    eprintln!(
      "{} Unrecognized language section '[lang.{}]' in formality.toml — \
       this override will not be applied. Run '{}' to see supported \
       languages.",
      "[WARN]".yellow().bold(),
      name.bold(),
      "fml doctor".cyan()
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::CommandFactory;
  use clap::Parser;

  #[test]
  fn test_lint_check_is_rejected_with_a_tailored_error() {
    let cli = Cli::try_parse_from(["fml", "lint", "--check"])
      .expect("--check must parse so validate can reject it by name");
    let err = cli
      .validate()
      .expect_err("`fml lint --check` must be an error");
    let rendered = err.to_string();
    assert!(
      rendered.contains("never writes"),
      "error should say why lint has no mode flag, got:\n{rendered}"
    );
    assert!(
      rendered.contains("fml fix --check"),
      "error should name the spelling that does what the user wanted, got:\n{rendered}"
    );
    assert!(
      !rendered.contains("-- --check"),
      "error must not reproduce clap's misleading passthrough tip, got:\n{rendered}"
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
    assert!(matches!(
      cli.command,
      Commands::Fix(fix::Args { check: true, .. })
    ));
    assert!(cli.validate().is_ok());
  }

  #[test]
  fn test_schema_parses_and_is_advertised_in_help() {
    let cli = Cli::try_parse_from(["fml", "schema"]).unwrap();
    assert!(matches!(
      cli.command,
      Commands::Schema(schema::Args { output: None })
    ));
    assert!(cli.validate().is_ok());

    let cli =
      Cli::try_parse_from(["fml", "schema", "-o", "schema.json"]).unwrap();
    assert!(matches!(
      cli.command,
      Commands::Schema(schema::Args {
        output: Some(ref p)
      }) if p == path::Path::new("schema.json")
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
        "`fml {name} --help` should describe --check as reporting, got:\n{help}"
      );
      assert!(
        help.contains("Only act on files staged for git commit"),
        "`fml {name} --help` should use the shared --staged wording, got:\n{help}"
      );
    }

    let lint_help = cmd
      .find_subcommand_mut("lint")
      .expect("lint subcommand")
      .render_help()
      .to_string();
    assert!(
      lint_help.contains("Only act on files staged for git commit"),
      "`fml lint --help` should use the shared --staged wording, got:\n{lint_help}"
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

  #[test]
  fn test_relative_root_resolves_to_absolute() {
    let cwd = env::current_dir().expect("current dir");
    for (relative, expected) in [(".", cwd.clone()), ("src", cwd.join("src"))] {
      let resolved = resolve_root(Some(path::PathBuf::from(relative)));
      assert!(resolved.is_absolute(), "`{relative}` stayed relative");
      assert_eq!(resolved, expected);
    }
  }
}
