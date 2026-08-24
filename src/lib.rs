//! Formality (`fml`) is a unified CLI for formatting, linting, and syncing configurations across multiple language surfaces.

#![warn(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]

/// Command-line argument parsing definitions.
pub mod cli;
/// CLI command implementations.
pub mod commands;
/// Configuration loading, parsing, and resolving.
pub mod config;
/// Execution engine for running formatters, linters, and version checks.
pub mod engine;
/// Crate-wide error types and exit status handling.
pub mod errors;
/// Language surface definitions and registry.
pub mod surfaces;
/// Terminal UI components and layout rendering.
pub mod ui;

// Backward-compatible top-level module aliases so existing `crate::foo::*`
// and `fml::foo::*` paths (integration tests, external consumers) keep
// working after the domain-driven `src/` reorganization.
//
// These aliases are a compatibility shim only: internal code in this crate
// always spells out the canonical, structural path (e.g.
// `crate::ui::table`, `crate::engine::version`) rather than the shortened
// alias, even where the alias would resolve to the same item. Keeping that
// distinction consistent means the alias list can eventually be trimmed or
// deprecated without touching any internal call site.
//
// DEPRECATED / STALE ALIAS: doctor
pub use commands::doctor;
// DEPRECATED / STALE ALIAS: lsp
pub use commands::lsp;
pub use commands::{get_git_changed_files, get_git_staged_files};
// DEPRECATED / STALE ALIAS: facets
pub use config::facets;
// DEPRECATED / STALE ALIAS: schema
pub use config::schema;
pub use config::schema::generate_schema;
pub use surfaces::editorconfig::generate_editorconfig;
// DEPRECATED / STALE ALIAS: diff
pub use engine::diff;
// DEPRECATED / STALE ALIAS: runner
pub use engine::runner;
// DEPRECATED / STALE ALIAS: update
pub use engine::update;
// DEPRECATED / STALE ALIAS: version
pub use engine::version;
// DEPRECATED / STALE ALIAS: editorconfig
pub use surfaces::editorconfig;
// DEPRECATED / STALE ALIAS: errors
pub use errors::{
  ConfigError, ExitStatus, FormalityError, GitError, IoError, Result,
  SurfaceError, ToolMissingError,
};
// DEPRECATED / STALE ALIAS: table
pub use ui::table;

use clap::Parser;
use cli::{Cli, Commands, MigrateCommands};
use colored::Colorize;
use config::FormalityConfig;
pub use config::SCHEMA_VERSION;
use std::path::PathBuf;

/// Parses CLI arguments from `std::env::args()` and executes the command.
#[must_use]
pub fn run() -> ExitStatus {
  let args = Cli::parse();
  run_with_args(args)
}

/// Executes the CLI command specified by the provided [`Cli`] arguments.
#[must_use]
pub fn run_with_args(args: Cli) -> ExitStatus {
  if std::env::var("FORCE_COLOR").is_ok()
    || std::env::var("CLICOLOR_FORCE").is_ok()
    || std::env::var("GITHUB_ACTIONS").is_ok()
  {
    colored::control::set_override(true);
  }

  let root = args.root.clone().unwrap_or_else(|| {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
  });

  let update_notifier = update::spawn_update_check();
  let schema_notifier = schema::spawn_schema_check(&root);
  let status = run_command_inner(args);
  schema::print_schema_notice(schema_notifier);
  update::print_update_notice(update_notifier);
  status
}

// Dispatches all top-level CLI commands (fmt, lint, sync, fix, doctor, init, lsp, schema, etc.).
fn run_command_inner(args: Cli) -> ExitStatus {
  let root = args.root.unwrap_or_else(|| {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
  });

  let (mut config, _config_path) =
    match FormalityConfig::load_layered(Some(&root)) {
      Ok(res) => res,
      Err(e) => {
        FormalityError::from(e).print_diagnostic();
        return ExitStatus::Error;
      }
    };

  if let Some(custom_cfg) = args.config {
    match FormalityConfig::load_file(&custom_cfg) {
      Ok(custom) => config.merge(custom),
      Err(e) => {
        FormalityError::from(e).print_diagnostic();
        return ExitStatus::Error;
      }
    }
  }

  warn_unrecognized_lang_sections(&config);

  match args.command {
    Commands::Schema { output } => commands::schema::run_schema(output),

    Commands::Doctor { all, install } => {
      commands::doctor::run_doctor(&root, all, install, &config)
    }

    Commands::Install { all } => {
      commands::doctor::run_doctor(&root, all, true, &config)
    }

    Commands::Init { force, hidden } => {
      commands::init::run_init(&root, &config, force, hidden)
    }

    Commands::ListSurfaces => commands::surfaces::run_surfaces(&root, &config),

    Commands::Fmt {
      check,
      staged,
      changed,
      lang,
      install,
      paths,
    } => commands::fmt::run_fmt(
      &root, &config, check, staged, changed, lang, install, paths,
    ),

    Commands::Fix {
      staged,
      changed,
      lang,
      install,
      paths,
    } => commands::fix::run_fix(
      &root, &config, staged, changed, lang, install, paths,
    ),

    Commands::Lint {
      fix,
      staged,
      changed,
      lang,
      install,
      paths,
    } => commands::lint::run_lint(
      &root, &config, fix, staged, changed, lang, install, paths,
    ),

    Commands::Sync { check, lang } => {
      commands::sync::run_sync(&root, &config, check, lang)
    }

    Commands::Lsp => {
      commands::lsp::run_lsp_server(Some(root));
      ExitStatus::Clean
    }

    Commands::Table { json } => commands::table::run_table(json),

    Commands::Migrate { command } => match command {
      MigrateCommands::Schema => commands::migrate::run_migrate_schema(&root),
    },
  }
}

/// Warns (non-fatal, to stderr) about any `[lang.X]` sections in the
/// resolved config whose `X` isn't a recognized surface name or alias —
/// almost always a typo (e.g. `[lang.pythonn]`) that would otherwise be
/// silently ignored, leaving the user's override never applied and no
/// signal as to why. Runs once at config-load time so every subcommand
/// benefits, mirroring the `Unknown language surface` error already given
/// for an unrecognized `--lang` CLI flag value.
///
/// Deliberately does not flag a section that names a real surface which
/// simply isn't detected/active in the current workspace (e.g.
/// `[lang.rust]` in a Python-only repo) — that's a valid
/// pre-configuration, not a mistake.
fn warn_unrecognized_lang_sections(config: &FormalityConfig) {
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
      "fml list-surfaces".cyan()
    );
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  // Tier-2 enforcement for the module/file hierarchy rule documented in
  // docs/style-guide.md ("`*_tests.rs` vs `#[cfg(test)] mod tests`"): a
  // `#[test]` walking the filesystem, same mechanism `registry.rs`'s fleet
  // side-table checks established (#113) — reused here, not reinvented.
  //
  // The rule: test modules live inline (`#[cfg(test)] mod tests { ... }`) in
  // the file under test. The one sanctioned exception is a directory module
  // (`some/mod.rs`) large enough that its tests live in a sibling `tests.rs`
  // declared via `mod tests;` — never any other `*_tests.rs` name. #120
  // deliberately collapsed every previous `<name>_tests.rs` file back inline;
  // this test keeps that convention from silently drifting back.
  #[test]
  fn test_no_stray_test_files_outside_sanctioned_pattern() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();
    for entry in walkdir::WalkDir::new(&src_dir)
      .into_iter()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_file())
    {
      let path = entry.path();
      let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        continue;
      };
      if file_name == "tests.rs" {
        // Sanctioned only as a sibling of a directory module's `mod.rs`.
        let has_sibling_mod_rs = path.with_file_name("mod.rs").is_file();
        if !has_sibling_mod_rs {
          violations.push(format!(
            "{}: `tests.rs` with no sibling `mod.rs` — inline the tests in \
             the module file instead",
            path.display()
          ));
        }
      } else if file_name.ends_with("_tests.rs") {
        violations.push(format!(
          "{}: `*_tests.rs` naming is not the sanctioned pattern — inline \
           `#[cfg(test)] mod tests {{ ... }}` in the module file, or (only \
           for a directory module) use a sibling file named exactly \
           `tests.rs`",
          path.display()
        ));
      }
    }

    assert!(
      violations.is_empty(),
      "module/file hierarchy violation(s) — see docs/style-guide.md §1:\n{}",
      violations.join("\n")
    );
  }
}
