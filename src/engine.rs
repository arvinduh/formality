//! Formatting/linting engine orchestration.
//!
//! Coordinates subprocess dispatch, diffing, and tool version detection.
//! Pass dispatch lives in `runner`, diffing lives in `diff`, target resolution
//! lives in `target`, and version checking lives in `version`.

/// Unified diff generation and rendering.
pub mod diff;
/// Environment and tooling diagnostics.
pub mod doctor;
/// Language Server Protocol server.
pub mod lsp;
/// Execution runner for dispatching pass plans across surfaces.
pub mod runner;
/// Target resolution for paths, scopes, and language surfaces.
pub mod target;
/// Asynchronous self-update checker and release notice renderer.
pub mod update;
/// Tool version probing, semver parsing, and compatibility policy evaluation.
pub mod version;

use std::path;

/// Returns the cross-platform cache directory for formality.
#[must_use]
pub fn cache_dir() -> path::PathBuf {
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
pub fn cache_path(filename: &str) -> path::PathBuf {
  cache_dir().join(filename)
}
