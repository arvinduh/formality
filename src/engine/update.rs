use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

#[derive(Serialize, Deserialize, Debug)]
struct UpdateCache {
  last_checked_unix: u64,
  latest_tag: Option<String>,
}

fn get_cache_path() -> PathBuf {
  super::cache_path("update_check.json")
}

// Outer Option represents cache validity (fresh vs expired); inner Option is the cached latest tag.
#[allow(clippy::option_option)]
fn read_cached_tag() -> Option<Option<String>> {
  let path = get_cache_path();
  let data = std::fs::read_to_string(path).ok()?;
  let cache: UpdateCache = serde_json::from_str(&data).ok()?;

  let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
  if now.saturating_sub(cache.last_checked_unix) < UPDATE_CHECK_INTERVAL_SECS {
    Some(cache.latest_tag)
  } else {
    None
  }
}

fn write_cached_tag(tag: Option<&str>) {
  let path = get_cache_path();
  if let Some(parent) = path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  let cache = UpdateCache {
    last_checked_unix: now,
    latest_tag: tag.map(std::string::ToString::to_string),
  };
  if let Ok(json) = serde_json::to_string(&cache) {
    let _ = std::fs::write(path, json);
  }
}

/// Safely parse the `tag_name` field from GitHub release JSON response.
#[must_use]
pub fn parse_latest_tag_from_json(body: &str) -> Option<String> {
  let value: serde_json::Value = serde_json::from_str(body).ok()?;
  let tag = value.get("tag_name")?.as_str()?;
  Some(tag.to_string())
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
  let main_part = s.split('-').next().unwrap_or(s);
  let parts: Vec<&str> = main_part.split('.').collect();
  if parts.len() >= 3 {
    let major = parts[0].parse().ok()?;
    let minor = parts[1].parse().ok()?;
    let patch = parts[2].parse().ok()?;
    Some((major, minor, patch))
  } else if parts.len() == 2 {
    let major = parts[0].parse().ok()?;
    let minor = parts[1].parse().ok()?;
    Some((major, minor, 0))
  } else if parts.len() == 1 {
    let major = parts[0].parse().ok()?;
    Some((major, 0, 0))
  } else {
    None
  }
}

/// Compares a release tag (e.g. "v0.2.0" or "0.2.0") with the current version.
#[must_use]
pub fn is_newer_version(latest_tag: &str, current_version: &str) -> bool {
  let clean_latest = latest_tag.trim_start_matches('v');
  let clean_curr = current_version.trim_start_matches('v');
  if clean_latest.is_empty() || clean_curr.is_empty() {
    return false;
  }

  if let (Some(latest_nums), Some(curr_nums)) =
    (parse_semver(clean_latest), parse_semver(clean_curr))
  {
    latest_nums > curr_nums
  } else {
    false
  }
}

pub struct UpdateNotifier {
  handle: Option<std::thread::JoinHandle<Option<String>>>,
  cached_tag: Option<String>,
}

/// Spawns a background update check or uses cached result.
/// Returns an `UpdateNotifier` which should be passed to `print_update_notice()`
/// at the end of the CLI session to avoid interleaving output.
#[must_use]
pub fn spawn_update_check() -> Option<UpdateNotifier> {
  // Suppress update checks in CI/CD environments or when explicitly disabled
  if std::env::var("CI").is_ok()
    || std::env::var("GITHUB_ACTIONS").is_ok()
    || std::env::var("FORMALITY_NO_UPDATE_CHECK").is_ok()
  {
    return None;
  }

  let current_version = env!("CARGO_PKG_VERSION");

  // Check 24-hour cache first
  if let Some(cached_opt) = read_cached_tag() {
    if let Some(cached_tag) = cached_opt
      && is_newer_version(&cached_tag, current_version)
    {
      return Some(UpdateNotifier {
        handle: None,
        cached_tag: Some(cached_tag),
      });
    }
    return None;
  }

  // Spawn background check without blocking CLI execution
  let handle = std::thread::spawn(move || {
    if let Ok(output) = std::process::Command::new("curl")
      .args([
        "-s",
        "--connect-timeout",
        "1",
        "--max-time",
        "2",
        "-H",
        "User-Agent: formality-cli",
        "https://api.github.com/repos/arvinduh/formality/releases/latest",
      ])
      .output()
      && output.status.success()
    {
      let body = String::from_utf8_lossy(&output.stdout);
      let tag = parse_latest_tag_from_json(&body);
      write_cached_tag(tag.as_deref());
      if let Some(ref tag) = tag
        && is_newer_version(tag, current_version)
      {
        return Some(tag.clone());
      }
    }
    None
  });

  Some(UpdateNotifier {
    handle: Some(handle),
    cached_tag: None,
  })
}

/// Prints update banner if an update is available.
/// Should be called after all CLI command outputs (tables, diagnostics) are done.
pub fn print_update_notice(notifier: Option<UpdateNotifier>) {
  let Some(notifier) = notifier else {
    return;
  };

  let current_version = env!("CARGO_PKG_VERSION");

  let latest_tag = if let Some(tag) = notifier.cached_tag {
    Some(tag)
  } else if let Some(handle) = notifier.handle {
    handle.join().ok().flatten()
  } else {
    None
  };

  if let Some(tag) = latest_tag {
    eprintln!(
      "\n{} A new version of formality is available: {} (current: {})\n   Update via: {}",
      "⚡".yellow().bold(),
      tag.green().bold(),
      format!("v{current_version}").dimmed(),
      "cargo install --git https://github.com/arvinduh/formality".cyan()
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_latest_tag_minified() {
    let minified_json = r#"{"url":"https://api.github.com/repos/arvinduh/formality/releases/1","tag_name":"v0.2.0","name":"v0.2.0"}"#;
    assert_eq!(
      parse_latest_tag_from_json(minified_json),
      Some("v0.2.0".to_string())
    );
  }

  #[test]
  fn test_parse_latest_tag_multiline() {
    let multiline_json = r#"{
      "url": "https://api.github.com/repos/arvinduh/formality/releases/1",
      "tag_name": "v0.1.5",
      "published_at": "2026-08-17T00:00:00Z"
    }"#;
    assert_eq!(
      parse_latest_tag_from_json(multiline_json),
      Some("v0.1.5".to_string())
    );
  }

  #[test]
  fn test_is_newer_version_comparison() {
    assert!(is_newer_version("v0.2.0", "0.1.0"));
    assert!(is_newer_version("0.1.1", "0.1.0"));
    assert!(is_newer_version("v1.0.0", "0.9.9"));
    assert!(!is_newer_version("v0.1.0", "0.1.0"));
    assert!(!is_newer_version("v0.0.9", "0.1.0"));
    assert!(!is_newer_version("https", "0.1.0"));
    assert!(!is_newer_version("", "0.1.0"));
  }

  #[test]
  fn test_is_newer_version_multi_digit_components() {
    // Guards against a naive lexicographic/string comparison, which would
    // incorrectly rank "0.9.0" above "0.10.0" and "0.15.2".
    assert!(is_newer_version("v0.10.0", "0.9.0"));
    assert!(is_newer_version("v0.15.2", "0.9.9"));
    assert!(is_newer_version("v0.15.2", "0.15.1"));
    assert!(is_newer_version("v1.2.10", "1.2.9"));
    assert!(!is_newer_version("v0.9.0", "0.10.0"));
    assert!(!is_newer_version("v0.15.2", "0.15.2"));
    assert!(is_newer_version("v0.15.10", "0.15.9"));
  }
}
