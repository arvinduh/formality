use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

// A curl-level failure (missing binary, DNS failure, or the connect/max-time
// budget exceeded -- offline, captive portal, flaky link) gets a much shorter
// backoff than a successful check. This keeps `fml` from re-spawning curl on
// every single invocation while offline, without silently disabling update
// checks for a full day once the network comes back.
const UPDATE_CHECK_FAILURE_BACKOFF_SECS: u64 = 15 * 60; // 15 minutes

#[derive(Serialize, Deserialize, Debug)]
struct UpdateCache {
  last_checked_unix: u64,
  latest_tag: Option<String>,
  /// Set when this entry records a curl-level failure (no response at all)
  /// rather than a completed check, so it can be retried after the shorter
  /// [`UPDATE_CHECK_FAILURE_BACKOFF_SECS`] instead of the full 24h TTL.
  #[serde(default)]
  failed: bool,
}

fn get_cache_path() -> PathBuf {
  super::cache_path("update_check.json")
}

// Outer Option represents cache validity (fresh vs expired); inner Option is the cached latest tag.
#[allow(clippy::option_option)]
fn read_cached_tag() -> Option<Option<String>> {
  read_cached_tag_at(&get_cache_path())
}

/// Reads and validates the update-check cache at an explicit `path`. Takes
/// the path explicitly (rather than calling [`get_cache_path`] itself) so
/// tests can point it at a temp file instead of the real per-user cache
/// directory.
#[allow(clippy::option_option)]
fn read_cached_tag_at(path: &Path) -> Option<Option<String>> {
  let data = std::fs::read_to_string(path).ok()?;
  let cache: UpdateCache = serde_json::from_str(&data).ok()?;

  let interval = if cache.failed {
    UPDATE_CHECK_FAILURE_BACKOFF_SECS
  } else {
    UPDATE_CHECK_INTERVAL_SECS
  };

  let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
  if now.saturating_sub(cache.last_checked_unix) < interval {
    Some(cache.latest_tag)
  } else {
    None
  }
}

/// Writes the update-check cache to an explicit `path`, unconditionally
/// stamping `last_checked_unix` regardless of whether `tag` is present.
/// Takes the path explicitly (rather than calling [`get_cache_path`]
/// itself) so tests can point it at a temp file instead of the real
/// per-user cache directory.
fn write_cached_tag_at(path: &Path, tag: Option<&str>) {
  if let Some(parent) = path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  let cache = UpdateCache {
    last_checked_unix: now,
    latest_tag: tag.map(std::string::ToString::to_string),
    failed: false,
  };
  if let Ok(json) = serde_json::to_string(&cache) {
    let _ = std::fs::write(path, json);
  }
}

/// Writes the update-check cache to record a curl-level failure (no response
/// body at all) at `path`, stamping `last_checked_unix` so the next
/// invocation retries after [`UPDATE_CHECK_FAILURE_BACKOFF_SECS`] instead of
/// re-spawning curl immediately.
fn write_failed_check_at(path: &Path) {
  if let Some(parent) = path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  let cache = UpdateCache {
    last_checked_unix: now,
    latest_tag: None,
    failed: true,
  };
  if let Ok(json) = serde_json::to_string(&cache) {
    let _ = std::fs::write(path, json);
  }
}

/// Processes a GitHub releases API response body: parses the latest release
/// tag and caches the check timestamp regardless of whether that parse
/// succeeds, so a persistently malformed/unexpected API response only
/// triggers a network call once per [`UPDATE_CHECK_INTERVAL_SECS`] instead of
/// on every invocation — the same throttling the success path already gets.
/// Returns the latest tag only when it represents a version newer than
/// `current_version`.
fn process_release_response_at(
  cache_path: &Path,
  body: &str,
  current_version: &str,
) -> Option<String> {
  let tag = parse_latest_tag_from_json(body);
  write_cached_tag_at(cache_path, tag.as_deref());
  tag.filter(|t| is_newer_version(t, current_version))
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

/// Handle for background update check result.
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
      return process_release_response_at(
        &get_cache_path(),
        &body,
        current_version,
      );
    }
    // curl itself failed to produce a response (binary missing, DNS
    // failure, or the connect/max-time budget exceeded). Still stamp the
    // cache -- with a short failure backoff, not the full 24h TTL -- so the
    // very next invocation doesn't re-spawn curl and eat the same latency.
    write_failed_check_at(&get_cache_path());
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
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
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
  fn test_process_release_response_caches_timestamp_on_malformed_json() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    let before = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();
    let result =
      process_release_response_at(&cache_path, "not valid json {{{", "0.1.0");
    let after = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();

    assert_eq!(result, None);

    let data = std::fs::read_to_string(&cache_path)
      .expect("cache file must be written even when the tag fails to parse");
    let cache: UpdateCache =
      serde_json::from_str(&data).expect("cache file must be valid JSON");
    assert_eq!(cache.latest_tag, None);
    assert!(
      cache.last_checked_unix >= before && cache.last_checked_unix <= after,
      "last_checked_unix should be stamped with the current time even on parse failure"
    );
  }

  #[test]
  fn test_process_release_response_caches_timestamp_on_missing_tag_name() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    // Valid JSON, but no `tag_name` field -- parse_latest_tag_from_json
    // returns None even though the response body itself parsed fine.
    let body = r#"{"message": "rate limited"}"#;
    let result = process_release_response_at(&cache_path, body, "0.1.0");
    assert_eq!(result, None);

    let data = std::fs::read_to_string(&cache_path)
      .expect("cache file must be written even without a tag_name field");
    let cache: UpdateCache =
      serde_json::from_str(&data).expect("cache file must be valid JSON");
    assert_eq!(cache.latest_tag, None);
    assert!(cache.last_checked_unix > 0);
  }

  #[test]
  fn test_process_release_response_caches_and_returns_newer_tag() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    let body = r#"{"tag_name":"v9.9.9"}"#;
    let result = process_release_response_at(&cache_path, body, "0.1.0");
    assert_eq!(result, Some("v9.9.9".to_string()));

    let data = std::fs::read_to_string(&cache_path).unwrap();
    let cache: UpdateCache = serde_json::from_str(&data).unwrap();
    assert_eq!(cache.latest_tag, Some("v9.9.9".to_string()));
  }

  #[test]
  fn test_write_failed_check_stamps_cache_with_short_backoff_marker() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    let before = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();
    write_failed_check_at(&cache_path);
    let after = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();

    let data = std::fs::read_to_string(&cache_path)
      .expect("cache file must be written even when curl itself fails");
    let cache: UpdateCache =
      serde_json::from_str(&data).expect("cache file must be valid JSON");
    assert_eq!(cache.latest_tag, None);
    assert!(cache.failed);
    assert!(
      cache.last_checked_unix >= before && cache.last_checked_unix <= after,
      "last_checked_unix should be stamped with the current time on a curl failure"
    );
  }

  #[test]
  fn test_recent_failed_check_suppresses_recheck_without_full_day_ttl() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    const {
      assert!(
        UPDATE_CHECK_FAILURE_BACKOFF_SECS < UPDATE_CHECK_INTERVAL_SECS,
        "failure backoff must be distinguishable from (shorter than) the \
         24h success TTL, so a transient outage doesn't silently disable \
         update checks for a full day"
      );
    }

    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();

    // A failure recorded just under the failure backoff ago is still
    // "fresh" -- the very next invocation must not re-spawn curl.
    let fresh_failure = UpdateCache {
      last_checked_unix: now - (UPDATE_CHECK_FAILURE_BACKOFF_SECS - 1),
      latest_tag: None,
      failed: true,
    };
    std::fs::write(&cache_path, serde_json::to_string(&fresh_failure).unwrap())
      .unwrap();
    assert_eq!(
      read_cached_tag_at(&cache_path),
      Some(None),
      "a recent curl-level failure should be treated as a valid (empty) \
       cache entry, not force a fresh curl spawn"
    );

    // A failure recorded longer ago than the failure backoff -- but well
    // within the 24h success TTL -- must expire and allow a fresh check.
    let stale_failure = UpdateCache {
      last_checked_unix: now - (UPDATE_CHECK_FAILURE_BACKOFF_SECS + 1),
      latest_tag: None,
      failed: true,
    };
    std::fs::write(&cache_path, serde_json::to_string(&stale_failure).unwrap())
      .unwrap();
    assert_eq!(
      read_cached_tag_at(&cache_path),
      None,
      "a failure older than the short backoff window must expire well \
       before the 24h success TTL would"
    );
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
