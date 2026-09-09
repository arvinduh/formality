//! `fml migrate schema` (deprecated) — rewrites the `#:schema` directive line
//! in the user's `formality.toml` / `.formality.toml` to point at the current
//! release's schema URL. Deprecated in favour of `fml init`.

use colored::Colorize;
use std::path::Path;

use crate::config::find_project_config;
use crate::config::schema::apply_schema_pin;
pub use crate::config::schema::rewrite_schema_line;
use crate::errors::ExitStatus;

/// Runs `fml migrate schema`: locates the project config and delegates to
/// [`apply_schema_pin`] to rewrite or insert its `#:schema` directive.
pub fn run_migrate_schema(root: &Path) -> ExitStatus {
  let Some(config_path) = find_project_config(root) else {
    eprintln!(
      "{} No formality.toml or .formality.toml found in {}",
      "[ERR]".red().bold(),
      root.display()
    );
    return ExitStatus::Error;
  };

  apply_schema_pin(&config_path)
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::schema::SCHEMA_VERSION;
  use std::io::Write;

  #[test]
  fn test_run_migrate_schema_no_config_found() {
    let temp = tempfile::TempDir::new().unwrap();
    let status = run_migrate_schema(temp.path());
    assert_eq!(status, ExitStatus::Error);
  }

  #[test]
  fn test_run_migrate_schema_already_up_to_date() {
    let temp = tempfile::TempDir::new().unwrap();
    let config_path = temp.path().join("formality.toml");
    let mut f = std::fs::File::create(&config_path).unwrap();
    writeln!(
      f,
      "#:schema https://github.com/arvinduh/formality/releases/download/s{SCHEMA_VERSION}/formality.schema.json"
    )
    .unwrap();
    writeln!(f, "[global]").unwrap();
    drop(f);

    let before = std::fs::read_to_string(&config_path).unwrap();
    let status = run_migrate_schema(temp.path());
    let after = std::fs::read_to_string(&config_path).unwrap();

    assert_eq!(status, ExitStatus::Clean);
    assert_eq!(before, after, "no-op must not modify the file");
  }

  #[test]
  fn test_run_migrate_schema_rewrites_stale_version() {
    let temp = tempfile::TempDir::new().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(
      &config_path,
      "#:schema https://github.com/arvinduh/formality/releases/download/s0/formality.schema.json\n[global]\nindent_size = 4\n",
    )
    .unwrap();

    let status = run_migrate_schema(temp.path());
    let after = std::fs::read_to_string(&config_path).unwrap();

    assert_eq!(status, ExitStatus::Clean);
    assert!(
      after.contains(&format!("s{SCHEMA_VERSION}/formality.schema.json"))
    );
    assert!(after.contains("[global]\nindent_size = 4\n"));
  }

  #[test]
  fn test_run_migrate_schema_inserts_missing_directive() {
    let temp = tempfile::TempDir::new().unwrap();
    let config_path = temp.path().join("formality.toml");
    std::fs::write(&config_path, "[global]\nindent_size = 2\n").unwrap();

    let status = run_migrate_schema(temp.path());
    let after = std::fs::read_to_string(&config_path).unwrap();

    assert_eq!(status, ExitStatus::Clean);
    assert!(after.starts_with("#:schema "));
    assert!(
      after.contains(&format!("s{SCHEMA_VERSION}/formality.schema.json"))
    );
    assert!(after.contains("[global]\nindent_size = 2\n"));
  }

  #[test]
  fn test_run_migrate_schema_uses_hidden_config_when_present() {
    let temp = tempfile::TempDir::new().unwrap();
    let config_path = temp.path().join(".formality.toml");
    std::fs::write(&config_path, "[global]\n").unwrap();

    let status = run_migrate_schema(temp.path());
    let after = std::fs::read_to_string(&config_path).unwrap();

    assert_eq!(status, ExitStatus::Clean);
    assert!(after.starts_with("#:schema "));
  }
}
