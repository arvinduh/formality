//! JSON Schema generation for `formality.toml`, plus schema-version drift
//! detection: a background check (mirroring [`crate::engine::update`]'s
//! pattern) that warns when a workspace's config predates the schema
//! version this binary generates.

use crate::config::FormalityConfig;
use crate::engine::cache_path;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A schema release version: `major.minor`. A major bump means a breaking
/// schema change; a minor bump means an additive/compatible one.
/// Deliberately two components, not full semver — unlike the binary/
/// extension's `v{semver}` tag, a schema has no meaningful patch-level
/// distinction (there's no such thing as a schema patch that changes
/// nothing schema-relevant), and it's tracked independently of the binary
/// version rather than mirroring it, since the two change at different
/// rates (see #126).
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct SchemaVersion {
  /// Bumped on a breaking schema change.
  pub major: u32,
  /// Bumped on an additive/compatible schema change.
  pub minor: u32,
}

impl std::fmt::Display for SchemaVersion {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}.{}", self.major, self.minor)
  }
}

/// The current `s{major}.{minor}` schema version this build of `fml`
/// expects a project's `#:schema` directive to reference.
pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion { major: 1, minor: 2 };
const SCHEMA_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

/// A config file's schema version status relative to [`SCHEMA_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaStatus {
  /// The config's `#:schema` directive references the current version.
  UpToDate {
    /// The version found in the config's `#:schema` directive.
    version: SchemaVersion,
  },
  /// The config's `#:schema` directive references an older version.
  Stale {
    /// The version found in the config's `#:schema` directive.
    version: SchemaVersion,
    /// The current [`SCHEMA_VERSION`].
    expected: SchemaVersion,
  },
  /// The config has no `#:schema` directive at all.
  Missing,
}

/// On-disk cache of the last schema-staleness check, used to throttle
/// [`spawn_schema_check`] to once per [`SCHEMA_CHECK_INTERVAL_SECS`].
#[derive(Serialize, Deserialize, Debug)]
struct SchemaCheckCache {
  last_checked_unix: u64,
  stale_version: Option<SchemaVersion>,
}

/// Holds a pending stale-schema notice to be printed later via
/// [`print_schema_notice`], once command output has settled.
pub struct SchemaNotifier {
  stale_info: Option<(PathBuf, SchemaVersion, SchemaVersion)>,
}

/// Generates the JSON Schema for formality configuration dynamically using schemars.
#[must_use]
pub fn generate_schema() -> String {
  let schema = schemars::schema_for!(FormalityConfig);
  serde_json::to_string_pretty(&schema).unwrap_or_default()
}

/// Parses a `major.minor` pair out of a schema tag's digits (the part after
/// the `s`/`S` prefix, e.g. `"1.0"` from `"s1.0"`).
fn parse_version_digits(digits: &str) -> Option<SchemaVersion> {
  let (major_str, minor_str) = digits.split_once('.')?;
  if major_str.is_empty()
    || minor_str.is_empty()
    || !major_str.chars().all(|c| c.is_ascii_digit())
    || !minor_str.chars().all(|c| c.is_ascii_digit())
  {
    return None;
  }
  Some(SchemaVersion {
    major: major_str.parse().ok()?,
    minor: minor_str.parse().ok()?,
  })
}

/// Parses the `s{major}.{minor}` version from a `#:schema` directive in
/// config content.
#[must_use]
pub fn parse_schema_version(content: &str) -> Option<SchemaVersion> {
  for line in content.lines() {
    let trimmed = line.trim();
    if let Some(idx) = trimmed.find("#:schema") {
      let after = &trimmed[idx + "#:schema".len()..];
      for token in after.split_whitespace() {
        for segment in token.split(['/', '\\']) {
          let clean_seg =
            segment.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
          if let Some(digits) = clean_seg
            .strip_prefix('s')
            .or_else(|| clean_seg.strip_prefix('S'))
            && !digits.is_empty()
            && let Some(version) = parse_version_digits(digits)
          {
            return Some(version);
          }
        }
      }
    }
  }
  None
}

/// Evaluates the schema version status of configuration file content against `SCHEMA_VERSION`.
#[must_use]
pub fn check_schema_version_content(content: &str) -> SchemaStatus {
  if let Some(version) = parse_schema_version(content) {
    if version < SCHEMA_VERSION {
      SchemaStatus::Stale {
        version,
        expected: SCHEMA_VERSION,
      }
    } else {
      SchemaStatus::UpToDate { version }
    }
  } else {
    SchemaStatus::Missing
  }
}

/// Evaluates the schema version status of a configuration file path against `SCHEMA_VERSION`.
#[must_use]
pub fn check_schema_version_file(path: &Path) -> SchemaStatus {
  if let Ok(content) = std::fs::read_to_string(path) {
    check_schema_version_content(&content)
  } else {
    SchemaStatus::Missing
  }
}

fn read_schema_cache() -> Option<SchemaCheckCache> {
  let path = cache_path("schema_check.json");
  let data = std::fs::read_to_string(path).ok()?;
  serde_json::from_str(&data).ok()
}

fn write_schema_cache(stale_version: Option<SchemaVersion>) {
  let path = cache_path("schema_check.json");
  if let Some(parent) = path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  let cache = SchemaCheckCache {
    last_checked_unix: now,
    stale_version,
  };
  if let Ok(json) = serde_json::to_string(&cache) {
    let _ = std::fs::write(path, json);
  }
}

/// Spawns or performs a schema version check for `run_with_args()`.
///
/// Accepts an optional pre-discovered config path, skipping directory search if provided.
/// Respects `FORMALITY_NO_SCHEMA_CHECK` and CI environments, and is throttled by a TTL cache.
#[must_use]
pub fn spawn_schema_check(
  config_path: Option<&Path>,
) -> Option<SchemaNotifier> {
  if std::env::var("CI").is_ok()
    || std::env::var("GITHUB_ACTIONS").is_ok()
    || std::env::var("FORMALITY_NO_SCHEMA_CHECK").is_ok()
  {
    return None;
  }

  let config_path = config_path?;

  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());

  let config_mtime = std::fs::metadata(config_path)
    .and_then(|m| m.modified())
    .ok()
    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    .map_or(0, |d| d.as_secs());

  if let Some(cache) = read_schema_cache()
    && now.saturating_sub(cache.last_checked_unix) < SCHEMA_CHECK_INTERVAL_SECS
    && config_mtime <= cache.last_checked_unix
  {
    if let Some(version) = cache.stale_version
      && version < SCHEMA_VERSION
    {
      return Some(SchemaNotifier {
        stale_info: Some((config_path.to_path_buf(), version, SCHEMA_VERSION)),
      });
    }
    return None;
  }

  let status = check_schema_version_file(config_path);
  match status {
    SchemaStatus::Stale { version, expected } => {
      write_schema_cache(Some(version));
      Some(SchemaNotifier {
        stale_info: Some((config_path.to_path_buf(), version, expected)),
      })
    }
    _ => {
      write_schema_cache(None);
      None
    }
  }
}

/// Prints schema version warning banner if project schema reference is stale.
pub fn print_schema_notice(notifier: Option<SchemaNotifier>) {
  let Some(notifier) = notifier else {
    return;
  };
  let Some((path, version, expected)) = notifier.stale_info else {
    return;
  };

  let filename = path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("formality.toml");

  eprintln!(
    "\n{} {} references an outdated schema version 's{}' (current: 's{}')\n   Update #:schema directive to: {}",
    "⚡".yellow().bold(),
    filename.bold(),
    version.to_string().yellow().bold(),
    expected.to_string().green().bold(),
    format!("https://github.com/arvinduh/formality/releases/download/s{expected}/formality.schema.json").cyan()
  );
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn test_generate_schema_valid_json() {
    let schema_str = generate_schema();
    assert!(!schema_str.is_empty());
    let parsed: serde_json::Value =
      serde_json::from_str(&schema_str).expect("Valid JSON schema");
    assert_eq!(parsed["title"], "FormalityConfig");
    assert!(parsed.get("properties").is_some());
  }

  #[test]
  fn test_schema_version_constant() {
    assert_eq!(SCHEMA_VERSION, SchemaVersion { major: 1, minor: 2 });
  }

  #[test]
  fn test_schema_version_display_and_ord() {
    assert_eq!(SchemaVersion { major: 1, minor: 0 }.to_string(), "1.0");
    assert!(
      SchemaVersion { major: 1, minor: 0 }
        < SchemaVersion { major: 1, minor: 5 }
    );
    assert!(
      SchemaVersion { major: 1, minor: 9 }
        < SchemaVersion { major: 2, minor: 0 }
    );
  }

  #[test]
  fn test_parse_schema_version() {
    let sample = "#:schema https://github.com/arvinduh/formality/releases/download/s1.0/formality.schema.json";
    assert_eq!(
      parse_schema_version(sample),
      Some(SchemaVersion { major: 1, minor: 0 })
    );

    let stale = "#:schema https://github.com/arvinduh/formality/releases/download/s0.9/formality.schema.json";
    assert_eq!(
      parse_schema_version(stale),
      Some(SchemaVersion { major: 0, minor: 9 })
    );

    let tag_only = "#:schema s5.2";
    assert_eq!(
      parse_schema_version(tag_only),
      Some(SchemaVersion { major: 5, minor: 2 })
    );

    let no_schema = "[global]\nindent_size = 2\n";
    assert_eq!(parse_schema_version(no_schema), None);

    let invalid_schema = "#:schema https://example.com/schema.json";
    assert_eq!(parse_schema_version(invalid_schema), None);

    let no_minor = "#:schema s1";
    assert_eq!(parse_schema_version(no_minor), None);
  }

  #[test]
  fn test_check_schema_version_content() {
    let stale_content = "#:schema https://github.com/arvinduh/formality/releases/download/s0.9/formality.schema.json\n[global]\n";
    assert_eq!(
      check_schema_version_content(stale_content),
      SchemaStatus::Stale {
        version: SchemaVersion { major: 0, minor: 9 },
        expected: SCHEMA_VERSION,
      }
    );

    let current_content = "#:schema https://github.com/arvinduh/formality/releases/download/s1.2/formality.schema.json\n[global]\n";
    assert_eq!(
      check_schema_version_content(current_content),
      SchemaStatus::UpToDate {
        version: SchemaVersion { major: 1, minor: 2 }
      }
    );

    let future_content = "#:schema https://github.com/arvinduh/formality/releases/download/s1.5/formality.schema.json\n[global]\n";
    assert_eq!(
      check_schema_version_content(future_content),
      SchemaStatus::UpToDate {
        version: SchemaVersion { major: 1, minor: 5 }
      }
    );

    let missing_content = "[global]\nindent_size = 2\n";
    assert_eq!(
      check_schema_version_content(missing_content),
      SchemaStatus::Missing
    );
  }

  #[test]
  fn test_spawn_schema_check_none() {
    assert!(spawn_schema_check(None).is_none());
  }
}
