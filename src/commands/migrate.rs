//! `fml migrate schema` — rewrites the `#:schema` directive line in the
//! user's `formality.toml` / `.formality.toml` to point at the current
//! release's schema URL. Only that single line is touched; the rest of the
//! file (including any config content that may no longer match a breaking
//! schema change) is left untouched — that's a human decision, not something
//! this command attempts to reconcile.

use colored::Colorize;
use std::path::Path;

use crate::config::find_project_config;
use crate::config::schema::{
  SCHEMA_VERSION, SchemaStatus, SchemaVersion, check_schema_version_content,
};
use crate::errors::{ExitStatus, FormalityError, IoError};

/// Builds the canonical schema release download URL for a given
/// `s{major}.{minor}` tag.
#[must_use]
fn schema_url(version: SchemaVersion) -> String {
  format!(
    "https://github.com/arvinduh/formality/releases/download/s{version}/formality.schema.json"
  )
}

/// Rewrites (or inserts) the `#:schema` directive line in `content` to point
/// at `new_version`'s schema URL.
///
/// Only the first line containing `#:schema` is replaced; every other line is
/// preserved exactly, including the file's existing line-ending style (LF or
/// CRLF) — `content.lines()` strips `\r`, so naively rejoining with `\n`
/// would silently normalize a CRLF file to LF; the original separator is
/// detected and reused to avoid that. If no `#:schema` line exists, a fresh
/// one is inserted as the first line of the file, since `#:schema` directives
/// conventionally appear at the top.
#[must_use]
pub fn rewrite_schema_line(
  content: &str,
  new_version: SchemaVersion,
) -> String {
  let new_line = format!("#:schema {}", schema_url(new_version));
  let separator = if content.contains("\r\n") {
    "\r\n"
  } else {
    "\n"
  };

  let mut found = false;
  let mut lines: Vec<String> = content
    .lines()
    .map(|line| {
      if !found && line.trim_start().contains("#:schema") {
        found = true;
        new_line.clone()
      } else {
        line.to_string()
      }
    })
    .collect();

  if !found {
    lines.insert(0, new_line);
  }

  let mut result = lines.join(separator);
  result.push_str(separator);
  result
}

/// Runs `fml migrate schema`: locates the project config, rewrites its
/// `#:schema` directive to the current `SCHEMA_VERSION`, and reports what
/// changed (old version -> new version, a no-op if already current, or an
/// inserted directive if none was present).
pub fn run_migrate_schema(root: &Path) -> ExitStatus {
  let Some(config_path) = find_project_config(root) else {
    eprintln!(
      "{} No formality.toml or .formality.toml found in {}",
      "[ERR]".red().bold(),
      root.display()
    );
    return ExitStatus::Error;
  };

  let content = match std::fs::read_to_string(&config_path) {
    Ok(c) => c,
    Err(e) => {
      FormalityError::Io(IoError::new(Some(config_path), e)).print_diagnostic();
      return ExitStatus::Error;
    }
  };

  let filename = config_path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("formality.toml")
    .to_string();

  match check_schema_version_content(&content) {
    SchemaStatus::UpToDate { version } => {
      println!(
        "{} {} already references the current schema version {}.",
        "[OK]".green().bold(),
        filename.bold(),
        format!("s{version}").cyan()
      );
      ExitStatus::Clean
    }
    SchemaStatus::Stale { version, expected } => {
      let updated = rewrite_schema_line(&content, expected);
      write_and_report(
        &config_path,
        &updated,
        &format!(
          "{} Updated {} schema reference: {} -> {}",
          "[OK]".green().bold(),
          filename.bold(),
          format!("s{version}").yellow(),
          format!("s{expected}").green().bold()
        ),
      )
    }
    SchemaStatus::Missing => {
      let updated = rewrite_schema_line(&content, SCHEMA_VERSION);
      write_and_report(
        &config_path,
        &updated,
        &format!(
          "{} Inserted #:schema directive into {} pointing at {}.",
          "[OK]".green().bold(),
          filename.bold(),
          format!("s{SCHEMA_VERSION}").green().bold()
        ),
      )
    }
  }
}

fn write_and_report(
  config_path: &Path,
  updated: &str,
  message: &str,
) -> ExitStatus {
  match std::fs::write(config_path, updated) {
    Ok(()) => {
      println!("{message}");
      ExitStatus::Clean
    }
    Err(e) => {
      FormalityError::Io(IoError::new(Some(config_path.to_path_buf()), e))
        .print_diagnostic();
      ExitStatus::Error
    }
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::schema::SCHEMA_VERSION;
  use std::io::Write;

  #[test]
  fn test_rewrite_schema_line_replaces_stale_directive() {
    let content = "#:schema https://github.com/arvinduh/formality/releases/download/s0.9/formality.schema.json\n[global]\nindent_size = 2\n";
    let updated =
      rewrite_schema_line(content, SchemaVersion { major: 1, minor: 0 });
    assert_eq!(
      updated,
      "#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json\n[global]\nindent_size = 2\n"
    );
  }

  #[test]
  fn test_rewrite_schema_line_preserves_rest_of_file() {
    let content = "# a leading comment\n#:schema s0.9\n[global]\nindent_size = 2\n\n[lang.rust]\nline_width = 100\n";
    let updated =
      rewrite_schema_line(content, SchemaVersion { major: 3, minor: 1 });
    assert!(updated.contains("# a leading comment\n"));
    assert!(updated.contains("[lang.rust]\nline_width = 100\n"));
    assert!(updated.contains(
      "#:schema https://github.com/arvinduh/formality/releases/download/s3.1/formality.schema.json"
    ));
    // Only the schema line changed.
    assert_eq!(updated.lines().count(), content.lines().count());
  }

  #[test]
  fn test_rewrite_schema_line_inserts_when_missing() {
    let content = "[global]\nindent_size = 2\n";
    let updated =
      rewrite_schema_line(content, SchemaVersion { major: 1, minor: 0 });
    let mut lines = updated.lines();
    assert_eq!(
      lines.next(),
      Some(
        "#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json"
      )
    );
    assert_eq!(lines.next(), Some("[global]"));
    assert_eq!(lines.next(), Some("indent_size = 2"));
  }

  #[test]
  fn test_rewrite_schema_line_preserves_crlf_line_endings() {
    let content = "#:schema s0.9\r\n[global]\r\nindent_size = 2\r\n";
    let updated =
      rewrite_schema_line(content, SchemaVersion { major: 1, minor: 0 });
    assert!(
      !updated.contains('\n') || updated.matches("\r\n").count() == 3,
      "CRLF file must stay CRLF throughout: {updated:?}"
    );
    assert!(updated.contains("[global]\r\nindent_size = 2\r\n"));
    assert!(!updated.contains("\n[global]\n"), "must not degrade to LF");
  }

  #[test]
  fn test_rewrite_schema_line_only_touches_first_match() {
    // Pathological input with two `#:schema`-looking lines; only the first
    // should be rewritten, matching `parse_schema_version`'s "first match
    // wins" behavior.
    let content = "#:schema s0.9\n#:schema s5.2\n[global]\n";
    let updated =
      rewrite_schema_line(content, SchemaVersion { major: 2, minor: 0 });
    let mut lines = updated.lines();
    assert_eq!(
      lines.next(),
      Some(
        "#:schema https://github.com/arvinduh/formality/releases/download/s2.0/formality.schema.json"
      )
    );
    assert_eq!(lines.next(), Some("#:schema s5.2"));
  }

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
