//! Formatting/linting engine orchestration.
//!
//! Coordinates subprocess dispatch, diffing, and tool version detection.
//! Pass dispatch lives in `runner`, diffing lives in `diff`, target resolution
//! lives in `target`, and version checking lives in `version`.

/// Unified diff generation and rendering.
pub mod diff;
/// Toolchain probing for `fml doctor` and the stale-tool preflight.
pub mod doctor;
/// Editor-integration primitives for `fml lsp`.
pub mod lsp;
/// Execution runner for dispatching pass plans across surfaces.
pub mod runner;
/// Target resolution for paths, scopes, and language surfaces.
pub mod target;
/// The background release check and `fml update`'s install steps.
pub mod update;
/// Tool version probing, semver parsing, and compatibility policy evaluation.
pub mod version;

use std::fs;
use std::io;
use std::path;

use log;
use serde;
use serde_json;

/// Returns the cross-platform cache directory for formality.
#[must_use]
fn cache_dir() -> path::PathBuf {
  if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
    path::PathBuf::from(local_app_data).join("formality")
  } else if let Ok(cache_home) = std::env::var("XDG_CACHE_HOME") {
    path::PathBuf::from(cache_home).join("formality")
  } else if let Ok(home) =
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
  {
    path::PathBuf::from(home).join(".cache").join("formality")
  } else {
    std::env::temp_dir().join("formality")
  }
}

/// Returns the full path to a named cache file in formality's cache directory.
#[must_use]
fn cache_path(filename: &str) -> path::PathBuf {
  cache_dir().join(filename)
}

/// Writes `value` as JSON to the cache file `path`, creating its directory.
///
/// A cache only saves work, so a failure is logged rather than returned.
pub fn write_cache(path: &path::Path, value: &impl serde::Serialize) {
  let written = path
    .parent()
    .map_or(Ok(()), fs::create_dir_all)
    .and_then(|()| serde_json::to_string(value).map_err(io::Error::other))
    .and_then(|json| fs::write(path, json));
  if let Err(err) = written {
    log::debug!("cannot write cache {}: {err}", path.display());
  }
}
