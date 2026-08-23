use colored::Colorize;
use std::path::Path;

use crate::config::{
  DEFAULT_CONFIG_FILE_NAME, FormalityConfig, find_project_config,
};
use crate::errors::{ExitStatus, FormalityError, IoError};
use crate::surfaces::detect_surfaces_smart;

/// Runs the `fml init` command: writes a starter config file (`formality.toml`
/// by default, or the dotfile variant with `hidden`) pre-populated with the
/// auto-detected surfaces, refusing to overwrite an existing config unless
/// `force` is set.
pub fn run_init(
  root: &Path,
  config: &FormalityConfig,
  force: bool,
  hidden: bool,
) -> ExitStatus {
  let target_file_name = if hidden {
    ".formality.toml"
  } else {
    DEFAULT_CONFIG_FILE_NAME
  };
  let target = root.join(target_file_name);

  if let Some(existing) = find_project_config(root) {
    if !force {
      eprintln!(
        "{} Config file already exists at {}. Use {} to overwrite.",
        "[ERR]".red().bold(),
        existing.display(),
        "--force".bold()
      );
      return ExitStatus::Violations;
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

  let detected = detect_surfaces_smart(root, config);
  let detected_names: Vec<&str> = detected.iter().map(|s| s.name()).collect();
  let template = FormalityConfig::generate_init_template(&detected_names);

  match std::fs::write(&target, template) {
    Ok(()) => {
      println!(
        "{} Initialized {} with {} detected surface(s).",
        "[OK]".green().bold(),
        target.display().to_string().cyan(),
        detected.len()
      );
      ExitStatus::Clean
    }
    Err(e) => {
      FormalityError::Io(IoError::new(Some(target), e)).print_diagnostic();
      ExitStatus::Error
    }
  }
}
