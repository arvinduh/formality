//! CLI argument definitions and scaffolding execution for `fml init`.
//!
//! Owns argument definitions for config initialization and writes starter
//! configuration files directly.

use std::path;

use clap;
use colored::Colorize;

use crate::config;
use crate::errors;
use crate::surfaces;

/// Arguments for `fml init`.
#[derive(clap::Args, Clone, Copy, Debug)]
pub struct Args {
  /// Overwrite existing configuration file if it already exists
  #[arg(short = 'f', long)]
  pub force: bool,

  /// Create hidden config file (.formality.toml) instead of formality.toml
  #[arg(long)]
  pub hidden: bool,
}

/// Executes the `init` command: writes a starter config file (`formality.toml`
/// by default, or the dotfile variant with `hidden`) pre-populated with the
/// auto-detected surfaces, refusing to overwrite an existing config unless
/// `force` is set.
#[must_use]
pub fn run(
  args: Args,
  root: &path::Path,
  config: &config::FormalityConfig,
) -> errors::ExitStatus {
  let target_file_name = if args.hidden {
    ".formality.toml"
  } else {
    config::DEFAULT_CONFIG_FILE_NAME
  };
  let target = root.join(target_file_name);

  if let Some(existing) = config::find_project_config(root) {
    if !args.force {
      eprintln!(
        "{} Config file already exists at {}. Use {} to overwrite.",
        "[ERR]".red().bold(),
        existing.display(),
        "--force".bold()
      );
      return errors::ExitStatus::Violations;
    }
    // Warn when --force would create a file that is shadowed by an existing
    // higher-priority config (e.g. creating .formality.toml while
    // formality.toml already exists).
    if existing != target && existing.exists() {
      eprintln!(
        "{} '{}' already exists and takes precedence over '{}'. \
         The new file will be shadowed and ignored unless '{}' is removed.",
        "[WARN]".yellow().bold(),
        existing.display(),
        target_file_name,
        existing.display(),
      );
    }
  }

  let detected = surfaces::detect_surfaces_smart(root, config);
  let detected_names: Vec<&str> = detected.iter().map(|s| s.name()).collect();
  let template =
    config::FormalityConfig::generate_init_template(&detected_names);

  match std::fs::write(&target, template) {
    Ok(()) => {
      println!(
        "{} Initialized {} with {} detected surface(s).",
        "[OK]".green().bold(),
        target.display().to_string().cyan(),
        detected.len()
      );
      errors::ExitStatus::Clean
    }
    Err(e) => {
      errors::FormalityError::Io(errors::IoError::new(Some(target), e))
        .print_diagnostic();
      errors::ExitStatus::Error
    }
  }
}
