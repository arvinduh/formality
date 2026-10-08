//! Command definitions, routing, and the per-run context.
//!
//! Owns the top-level parser and its global flags, loading the config every
//! command shares, and the closing update notice. Each subcommand's
//! arguments and pipeline live in its own submodule; terminal output lives in
//! `ui`.

mod doctor;
mod init;
mod lsp;
mod pass;
mod schema;
mod sync;
mod ui;
mod update;

use std::env;
use std::path;

use clap;
use clap::builder::styling;
use colored::Colorize;
use log;

use fml::config;
use fml::engine::runner;
use fml::surfaces;

/// Cargo-style help colors; clap drops them for non-terminals and `NO_COLOR`.
const STYLES: clap::builder::Styles = clap::builder::Styles::styled()
  .header(styling::AnsiColor::Green.on_default().bold())
  .usage(styling::AnsiColor::Green.on_default().bold())
  .literal(styling::AnsiColor::Cyan.on_default().bold())
  .placeholder(styling::AnsiColor::Cyan.on_default())
  .error(styling::AnsiColor::Red.on_default().bold())
  .valid(styling::AnsiColor::Cyan.on_default().bold())
  .invalid(styling::AnsiColor::Yellow.on_default().bold());

/// One CLI to format, lint, and sync configs across all languages.
#[derive(clap::Parser, Debug)]
#[command(
  name = "formality",
  bin_name = "fml",
  version,
  styles = STYLES,
  long_about = "formality (fml) runs the best-in-class formatters and linters across Rust, Python, C/C++, Java, Go, JavaScript/TypeScript, Kotlin, Markdown, YAML, JSON, TOML, and Typst from one canonical config."
)]
pub struct Cli {
  /// Extra config file merged over formality.toml
  #[arg(short = 'c', long, global = true, value_name = "FILE")]
  config: Option<path::PathBuf>,

  /// Workspace root (defaults to the current directory)
  #[arg(short = 'w', long, global = true, value_name = "DIR")]
  root: Option<path::PathBuf>,

  /// Log more about what fml does (-v info, -vv debug, -vvv trace)
  #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
  verbose: u8,

  #[command(subcommand)]
  command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
  /// Format source files; --check reports without writing
  Fmt(pass::Fmt),
  /// Lint source files; never writes (use `fml fix` to apply fixes)
  Lint(pass::Selection),
  /// Apply lint fixes, then reformat; --check reports without writing
  Fix(pass::Fix),
  /// Write native tool configs (.rustfmt.toml, ruff.toml, ...) from the
  /// canonical config
  Sync(sync::Args),
  /// Check installed toolchains, optionally installing missing ones
  Doctor(doctor::Args),
  /// Write a starter formality.toml
  Init(init::Args),
  /// Write the JSON Schema for formality.toml
  Schema(schema::Args),
  /// Serve formatting and diagnostics over LSP (stdio)
  Lsp,
  /// Replace this fml with the latest release
  Update,
}

/// What every config-reading command runs against.
struct Context {
  /// Absolute workspace root.
  root: path::PathBuf,
  /// The layered config, with any `--config` file merged over it.
  config: config::FormalityConfig,
}

impl Cli {
  /// The log level `-v` asks for. `fml lsp` logs at least at info, since
  /// stderr is the server log an editor shows in its output channel.
  pub fn log_level(&self) -> log::LevelFilter {
    let level = match self.verbose {
      0 => log::LevelFilter::Warn,
      1 => log::LevelFilter::Info,
      2 => log::LevelFilter::Debug,
      _ => log::LevelFilter::Trace,
    };
    if matches!(self.command, Command::Lsp) {
      level.max(log::LevelFilter::Info)
    } else {
      level
    }
  }

  /// Runs the parsed command, then prints the update notice if one is due.
  pub fn run(self) -> runner::ExitStatus {
    // Needs no project, and is itself the answer to the update notice.
    if matches!(self.command, Command::Update) {
      return update::run();
    }
    let root = absolute_root(self.root);
    let notifier = fml::engine::update::spawn_update_check();
    let status = match self.command {
      Command::Lsp => lsp::run(&root),
      command => match load_config(&root, self.config) {
        Ok(config) => command.run(&Context { root, config }),
        Err(err) => {
          ui::error(&err);
          runner::ExitStatus::Error
        }
      },
    };
    if let Some(tag) =
      notifier.and_then(fml::engine::update::UpdateNotifier::latest_tag)
    {
      ui::update_available(&tag);
    }
    status
  }
}

impl Command {
  fn run(self, ctx: &Context) -> runner::ExitStatus {
    match self {
      Self::Fmt(args) => args.run(ctx),
      Self::Lint(args) => args.lint(ctx),
      Self::Fix(args) => args.run(ctx),
      Self::Sync(args) => args.run(ctx),
      Self::Doctor(args) => args.run(ctx),
      Self::Init(args) => args.run(ctx),
      Self::Schema(args) => args.run(),
      Self::Lsp => lsp::run(&ctx.root),
      Self::Update => update::run(),
    }
  }
}

/// Loads the layered project config, merges `extra` over it, and warns about
/// `[lang.X]` sections no surface recognizes.
fn load_config(
  root: &path::Path,
  extra: Option<path::PathBuf>,
) -> Result<config::FormalityConfig, config::Error> {
  let project = config::resolve::find_project_config(root);
  let (mut config, _) =
    config::FormalityConfig::load_layered_with_path(project.as_deref())?;
  if let Some(extra) = extra {
    config.merge(config::FormalityConfig::load_file(&extra)?);
  }
  let registry = surfaces::registry::SurfaceRegistry::default();
  for name in config.unrecognized_lang_sections(&registry) {
    ui::warn(&format!(
      "unrecognized section '[lang.{}]' is ignored; run '{}' to list \
       supported languages",
      name.bold(),
      "fml doctor --all".cyan()
    ));
  }
  Ok(config)
}

/// Resolves `--root` (or the current directory) to an absolute path, so every
/// command sees the same root however it was spelled.
fn absolute_root(root: Option<path::PathBuf>) -> path::PathBuf {
  let root = root.unwrap_or_else(|| path::PathBuf::from("."));
  path::absolute(&root)
    .or_else(|_| env::current_dir().map(|cwd| cwd.join(&root)))
    .unwrap_or(root)
}
