//! JSON Schema generation for `formality.toml`, plus schema-version drift
//! detection: a check that warns when a workspace's config predates the
//! schema version this binary generates. Unlike [`crate::engine::update`],
//! whose remote version check runs off-thread behind a TTL cache, this one
//! only reads a local file and so runs inline, uncached (see
//! [`schema_notice_for`]).

use crate::config::FormalityConfig;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion { major: 1, minor: 1 };

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

/// Decides whether `config_path`'s `#:schema` directive is behind
/// [`SCHEMA_VERSION`], returning the notice to print if so.
///
/// Always reads the file. There is no TTL cache in front of this on
/// purpose: unlike [`crate::engine::update`], whose equivalent check pays
/// for a network round-trip and therefore has something worth throttling,
/// this one is a single small local read, and the notice it produces is
/// printed on every run regardless -- so a cache could only ever make the
/// result *wrong*. It did: the previous implementation replayed a cached
/// `stale_version` for 24 hours whenever the config's mtime wasn't newer
/// than the last check (an ordinary `git checkout` restoring a file with an
/// older mtime is enough), so a user who had already fixed their directive
/// kept being told their config referenced a version it no longer
/// contained.
#[must_use]
fn schema_notice_for(config_path: &Path) -> Option<SchemaNotifier> {
  match check_schema_version_file(config_path) {
    SchemaStatus::Stale { version, expected } => Some(SchemaNotifier {
      stale_info: Some((config_path.to_path_buf(), version, expected)),
    }),
    SchemaStatus::UpToDate { .. } | SchemaStatus::Missing => None,
  }
}

/// Performs the schema version check for `run_with_args()`.
///
/// Accepts an optional pre-discovered config path, skipping directory
/// search if provided. Returns `None` -- checking nothing at all -- under
/// `CI`/`GITHUB_ACTIONS` or `FORMALITY_NO_SCHEMA_CHECK`, where a nudge
/// aimed at a human editing the config is just log noise.
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

  schema_notice_for(config_path?)
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
    assert_eq!(SCHEMA_VERSION, SchemaVersion { major: 1, minor: 1 });
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

    let current_content = "#:schema https://github.com/arvinduh/formality/releases/download/s1.1/formality.schema.json\n[global]\n";
    assert_eq!(
      check_schema_version_content(current_content),
      SchemaStatus::UpToDate {
        version: SchemaVersion { major: 1, minor: 1 }
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

  // The staleness notice used to be served from a 24h on-disk cache keyed
  // on the config's mtime, which meant a config the user had *already*
  // fixed kept producing a warning quoting a version the file no longer
  // contains (any mtime not newer than the last check -- a plain `git
  // checkout` is enough -- kept replaying the cached verdict). These lock
  // in that the verdict always reflects the file as it is on disk right
  // now. `s0.1` is used as the stale pin because it stays behind
  // SCHEMA_VERSION no matter how far that is bumped later.
  #[test]
  fn test_schema_notice_reflects_current_file_contents() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("formality.toml");

    let stale =
      "#:schema https://example.com/s0.1/formality.schema.json\n[global]\n";
    std::fs::write(&config, stale).unwrap();
    let notice = schema_notice_for(&config)
      .expect("a config pinned to an older schema must warn");
    assert_eq!(
      notice
        .stale_info
        .map(|(_, version, expected)| (version, expected)),
      Some((SchemaVersion { major: 0, minor: 1 }, SCHEMA_VERSION))
    );

    // Fixing the directive must clear the warning on the very next call --
    // no TTL, no mtime comparison, no stale replay.
    let current = format!(
      "#:schema https://example.com/s{SCHEMA_VERSION}/formality.schema.json\n[global]\n"
    );
    std::fs::write(&config, current).unwrap();
    assert!(
      schema_notice_for(&config).is_none(),
      "a config updated to the current schema version must stop warning \
       immediately, not once some cache expires"
    );
  }

  #[test]
  fn test_schema_notice_absent_for_config_without_directive() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("formality.toml");
    std::fs::write(&config, "[global]\nindent_size = 2\n").unwrap();
    assert!(
      schema_notice_for(&config).is_none(),
      "a config with no #:schema directive at all has no version to be \
       behind"
    );
  }
}
