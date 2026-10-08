//! Background self-update check for newer releases.
//!
//! Probes GitHub releases asynchronously for updates. Installing a release
//! over the running binary is `install`'s. Toolchain version
//! compatibility checks for installed linters/formatters are owned by `super::version`.

/// Downloading, verifying and installing a release over the running binary.
pub mod install;

use std::path;
use std::time;

use log;
use serde;

use crate::engine;
use crate::engine::version;

const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

// A curl-level failure (missing binary, DNS failure, or the connect/max-time
// budget exceeded -- offline, captive portal, flaky link) gets a much shorter
// backoff than a successful check. This keeps `fml` from re-spawning curl on
// every single invocation while offline, without silently disabling update
// checks for a full day once the network comes back.
const UPDATE_CHECK_FAILURE_BACKOFF_SECS: u64 = 15 * 60; // 15 minutes

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct UpdateCache {
  last_checked_unix: u64,
  latest_tag: Option<String>,
  /// Set when this entry records a curl-level failure (no response at all)
  /// rather than a completed check, so it can be retried after the shorter
  /// [`UPDATE_CHECK_FAILURE_BACKOFF_SECS`] instead of the full 24h TTL.
  #[serde(default)]
  failed: bool,
}

fn get_cache_path() -> path::PathBuf {
  super::cache_path("update_check.json")
}

// Outer Option represents cache validity (fresh vs expired); inner Option is the cached latest tag.
#[expect(
  clippy::option_option,
  reason = "outer Option represents cache validity; inner Option is the cached latest tag"
)]
fn read_cached_tag() -> Option<Option<String>> {
  let now = time::SystemTime::now()
    .duration_since(time::UNIX_EPOCH)
    .ok()?
    .as_secs();
  read_cached_tag_at(&get_cache_path(), now)
}

/// Reads and validates the update-check cache at an explicit `path` as of
/// `now` (Unix seconds). Takes both explicitly (rather than calling
/// [`get_cache_path`] and reading the clock itself) so tests can point it at
/// a temp file and judge freshness against the same instant they stamped,
/// with no second rollover between the two clock reads.
#[expect(
  clippy::option_option,
  reason = "outer Option represents cache validity; inner Option is the cached latest tag"
)]
fn read_cached_tag_at(path: &path::Path, now: u64) -> Option<Option<String>> {
  let data = std::fs::read_to_string(path).ok()?;
  let cache: UpdateCache = serde_json::from_str(&data).ok()?;

  let interval = if cache.failed {
    UPDATE_CHECK_FAILURE_BACKOFF_SECS
  } else {
    UPDATE_CHECK_INTERVAL_SECS
  };

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
fn write_cached_tag_at(path: &path::Path, tag: Option<&str>) {
  let now = time::SystemTime::now()
    .duration_since(time::UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  let cache = UpdateCache {
    last_checked_unix: now,
    latest_tag: tag.map(ToString::to_string),
    failed: false,
  };
  engine::write_cache(path, &cache);
}

/// Writes the update-check cache to record a curl-level failure (no response
/// body at all) at `path`, stamping `last_checked_unix` so the next
/// invocation retries after [`UPDATE_CHECK_FAILURE_BACKOFF_SECS`] instead of
/// re-spawning curl immediately.
fn write_failed_check_at(path: &path::Path) {
  log::debug!("update check failed; retrying after the backoff");
  let now = time::SystemTime::now()
    .duration_since(time::UNIX_EPOCH)
    .map_or(0, |d| d.as_secs());
  let cache = UpdateCache {
    last_checked_unix: now,
    latest_tag: None,
    failed: true,
  };
  engine::write_cache(path, &cache);
}

/// Processes a `GitHub` releases API response body: parses the latest release
/// tag and caches the check timestamp regardless of whether that parse
/// succeeds, so a persistently malformed/unexpected API response only
/// triggers a network call once per [`UPDATE_CHECK_INTERVAL_SECS`] instead of
/// on every invocation — the same throttling the success path already gets.
/// Returns the latest tag only when it represents a version newer than
/// `current_version`.
fn process_release_response_at(
  cache_path: &path::Path,
  body: &str,
  current_version: &str,
) -> Option<String> {
  let tag = parse_latest_tag_from_json(body);
  write_cached_tag_at(cache_path, tag.as_deref());
  tag.filter(|t| is_newer_version(t, current_version))
}

/// Safely parse the `tag_name` field from `GitHub` release JSON response.
#[must_use]
fn parse_latest_tag_from_json(body: &str) -> Option<String> {
  let value: serde_json::Value = serde_json::from_str(body).ok()?;
  let tag = value.get("tag_name")?.as_str()?;
  Some(tag.to_string())
}

/// Compares a release tag (e.g. "v0.2.0" or "0.2.0") with the current version.
///
/// Both sides are scraped by [`version::Version::parse`] (the custom extraction layer —
/// it tolerates the `v` prefix `GitHub` tags carry); the `>` that decides the
/// banner is `semver`-backed via [`version::Version`]'s `Ord`. An unparseable tag can
/// never trip the banner: it yields `false`, not a spurious "update available".
#[must_use]
fn is_newer_version(latest_tag: &str, current_version: &str) -> bool {
  match (
    version::Version::parse(latest_tag),
    version::Version::parse(current_version),
  ) {
    (Some(latest), Some(curr)) => latest > curr,
    _ => false,
  }
}

/// Handle for background update check result.
pub struct UpdateNotifier {
  handle: Option<std::thread::JoinHandle<Option<String>>>,
  cached_tag: Option<String>,
}

/// Spawns a background update check or uses cached result.
/// Returns an `UpdateNotifier` whose [`UpdateNotifier::latest_tag`] the CLI
/// reads at the end of the session, so the notice never interleaves output.
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

impl UpdateNotifier {
  /// Returns the newer release tag, if any, joining the background check.
  ///
  /// Blocks until the check thread finishes, so the CLI calls this only after
  /// all command output is written.
  #[must_use]
  pub fn latest_tag(self) -> Option<String> {
    match (self.cached_tag, self.handle) {
      (Some(tag), _) => Some(tag),
      (None, Some(handle)) => handle.join().ok().flatten(),
      (None, None) => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_latest_tag_minified() {
    let minified_json = r#"{"url":"https://api.github.com/repos/arvinduh/formality/releases/1","tag_name":"v0.2.0","name":"v0.2.0"}"#;
    assert_eq!(
      parse_latest_tag_from_json(minified_json),
      Some("v0.2.0".to_string())
    );
  }

  #[test]
  fn parse_latest_tag_multiline() {
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
  fn is_newer_version_comparison() {
    assert!(is_newer_version("v0.2.0", "0.1.0"));
    assert!(is_newer_version("0.1.1", "0.1.0"));
    assert!(is_newer_version("v1.0.0", "0.9.9"));
    assert!(!is_newer_version("v0.1.0", "0.1.0"));
    assert!(!is_newer_version("v0.0.9", "0.1.0"));
    assert!(!is_newer_version("https", "0.1.0"));
    assert!(!is_newer_version("", "0.1.0"));
  }

  #[test]
  fn is_newer_version_prerelease_transitions() {
    // The semver-backed ordering the doc-comment now relies on: a final
    // release supersedes its own prereleases; a prerelease never supersedes
    // the matching final release; prereleases order among themselves.
    assert!(is_newer_version("v1.0.0", "1.0.0-rc.1"));
    assert!(!is_newer_version("v1.0.0-rc.1", "1.0.0"));
    assert!(is_newer_version("v1.0.0-rc.2", "1.0.0-rc.1"));
    assert!(!is_newer_version("v1.0.0-rc.1", "1.0.0-rc.2"));
    assert!(!is_newer_version("v1.0.0-rc.1", "1.0.0-rc.1"));
    // A higher release with a prerelease tag still beats a lower release.
    assert!(is_newer_version("v1.2.0-beta", "1.1.0"));
  }

  #[test]
  fn is_newer_version_inverted_prerelease_conventions() {
    // #171: Suffixes outside the packaging blocklist (-m1, -M1, -next,
    // -devel, etc.) classify as genuine prereleases, fixing the fail-unsafe
    // direction where prereleases would be advertised as stable or a local
    // prerelease build would suppress a legitimate stable update notice.
    assert!(is_newer_version("v1.0.0", "1.0.0-m1"));
    assert!(is_newer_version("v1.0.0", "1.0.0-M1"));
    assert!(is_newer_version("v1.0.0", "1.0.0-a1"));
    assert!(is_newer_version("v1.0.0", "1.0.0-b2"));
    assert!(is_newer_version("v1.0.0", "1.0.0-next"));
    assert!(is_newer_version("v1.0.0", "1.0.0-next.5"));
    assert!(is_newer_version("v1.0.0", "1.0.0-experimental"));
    assert!(is_newer_version("v1.0.0", "1.0.0-unstable"));
    assert!(is_newer_version("v1.0.0", "1.0.0-insiders"));
    assert!(is_newer_version("v1.0.0", "1.0.0-devel"));
    assert!(is_newer_version("v1.0.0", "1.0.0-milestone1"));

    // A prerelease never supersedes its matching final release.
    assert!(!is_newer_version("v1.0.0-m1", "1.0.0"));
    assert!(!is_newer_version("v1.0.0-next", "1.0.0"));

    // Prereleases order among themselves.
    assert!(is_newer_version("v1.0.0-m2", "1.0.0-m1"));
    assert!(is_newer_version("v1.0.0-next.2", "1.0.0-next.1"));

    // Packaging blocklist entries are salvaged to bare core, so neither
    // side is newer than the other when base versions match.
    assert!(!is_newer_version("v1.0.0", "1.0.0-ubuntu1"));
    assert!(!is_newer_version("v1.0.0-ubuntu1", "1.0.0"));
    assert!(!is_newer_version("v1.0.0", "1.0.0-deb1"));
    assert!(!is_newer_version("v1.0.0", "1.0.0-fc39"));
  }

  #[test]
  fn process_release_response_caches_timestamp_on_malformed_json() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    let before = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
      .unwrap()
      .as_secs();
    let result =
      process_release_response_at(&cache_path, "not valid json {{{", "0.1.0");
    let after = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
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
  fn process_release_response_caches_timestamp_on_missing_tag_name() {
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
  fn process_release_response_caches_and_returns_newer_tag() {
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
  fn write_failed_check_stamps_cache_with_short_backoff_marker() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("update_check.json");

    let before = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
      .unwrap()
      .as_secs();
    write_failed_check_at(&cache_path);
    let after = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
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
  fn recent_failed_check_suppresses_recheck_without_full_day_ttl() {
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

    let now = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
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
      read_cached_tag_at(&cache_path, now),
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
      read_cached_tag_at(&cache_path, now),
      None,
      "a failure older than the short backoff window must expire well \
       before the 24h success TTL would"
    );
  }

  #[test]
  fn is_newer_version_multi_digit_components() {
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
