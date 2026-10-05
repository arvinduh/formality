//! `fml` binary entry point and Process Host per rust-guide §3H.
//!
//! Owns terminal color detection (`NO_COLOR`, `CLICOLOR_FORCE`), argument
//! parsing via [`cli::Cli::parse_checked`], command dispatch to
//! `fml::commands`, background update checks, and process exit code mapping.

mod cli;

use std::env;
use std::path;
use std::process;

use colored;
use colored::Colorize;
use fml::commands;
use fml::config;
use fml::engine;
use fml::errors;
use fml::surfaces;
use fml::ui;

fn main() -> process::ExitCode {
  let cli = cli::Cli::parse_checked();
  match run(cli) {
    errors::ExitStatus::Clean => process::ExitCode::SUCCESS,
    errors::ExitStatus::Violations => process::ExitCode::from(1),
    errors::ExitStatus::Error => process::ExitCode::from(2),
  }
}

/// Dispatches the parsed CLI arguments to the corresponding library command.
fn run(args: cli::Cli) -> errors::ExitStatus {
  // NO_COLOR wins over every force-color signal, matching the precedence
  // `ui::table::Palette::detect` already applies to this crate's own escape
  // codes.
  if ui::no_color_requested() {
    colored::control::set_override(false);
  } else if ui::color_forced() {
    colored::control::set_override(true);
  }

  let root = resolve_root(args.root);
  let update_notifier = engine::update::spawn_update_check();

  // The server loads and reports its own config at `initialize`.
  if matches!(args.command, cli::Commands::Lsp) {
    let status = commands::lsp::run_lsp_server(Some(&root));
    engine::update::print_update_notice(update_notifier);
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
        engine::update::print_update_notice(update_notifier);
        return errors::ExitStatus::Error;
      }
    };

  if let Some(custom_cfg) = args.config {
    match config::FormalityConfig::load_file(&custom_cfg) {
      Ok(custom) => config.merge(custom),
      Err(e) => {
        errors::FormalityError::from(e).print_diagnostic();
        engine::update::print_update_notice(update_notifier);
        return errors::ExitStatus::Error;
      }
    }
  }

  warn_unrecognized_lang_sections(&config);

  let status = dispatch(args.command, &root, &config);

  engine::update::print_update_notice(update_notifier);
  status
}

/// Dispatches the subcommand against the loaded configuration.
fn dispatch(
  command: cli::Commands,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  match command {
    cli::Commands::Schema { output } => commands::schema::run_schema(output),

    cli::Commands::Doctor { all, install } => {
      commands::doctor::run_doctor(root, all, install, config)
    }

    cli::Commands::Init { force, hidden } => {
      commands::init::run_init(root, config, force, hidden)
    }

    cli::Commands::Fmt {
      check,
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => commands::fmt::run_fmt(
      root,
      config,
      check,
      staged,
      changed,
      &lang,
      paths,
      allow_missing,
    ),

    cli::Commands::Fix {
      check,
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => commands::fix::run_fix(
      root,
      config,
      check,
      staged,
      changed,
      &lang,
      paths,
      allow_missing,
    ),

    cli::Commands::Lint {
      staged,
      changed,
      lang,
      allow_missing,
      paths,
      ..
    } => commands::lint::run_lint(
      root,
      config,
      staged,
      changed,
      &lang,
      paths,
      allow_missing,
    ),

    cli::Commands::Sync { check, lang } => {
      commands::sync::run_sync(root, config, check, &lang)
    }

    cli::Commands::Lsp => {
      unreachable!("`lsp` is dispatched before the config load")
    }
  }
}

/// Resolves `--root` (or the current directory when it is absent) to an
/// absolute path, so every command sees the same root however it was spelled.
fn resolve_root(root: Option<path::PathBuf>) -> path::PathBuf {
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
