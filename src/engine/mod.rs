//! Formatting/linting engine: subprocess orchestration, diffing, tool
//! version detection, and self-update checking.

/// Unified diff generation and rendering.
pub mod diff;
/// Execution runner for dispatching pass plans across surfaces.
pub mod runner;
/// Asynchronous self-update checker and release notice renderer.
pub mod update;
/// Tool version probing, semver parsing, and compatibility policy evaluation.
pub mod version;

pub use diff::render_diff;
pub use runner::{Mode, Pass, Plan, Runner};
pub use update::{UpdateNotifier, print_update_notice, spawn_update_check};

use std::path::PathBuf;

/// Returns the cross-platform cache directory for formality.
#[must_use]
pub fn cache_dir() -> PathBuf {
  if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
    PathBuf::from(local_app_data).join("formality")
  } else if let Ok(cache_home) = std::env::var("XDG_CACHE_HOME") {
    PathBuf::from(cache_home).join("formality")
  } else if let Ok(home) =
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
  {
    PathBuf::from(home).join(".cache").join("formality")
  } else {
    std::env::temp_dir().join("formality")
  }
}

/// Returns the full path to a named cache file in formality's cache directory.
#[must_use]
pub fn cache_path(filename: &str) -> PathBuf {
  cache_dir().join(filename)
}
