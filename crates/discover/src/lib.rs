//! Finds the files fml acts on and groups them by file extension.
//!
//! Owns walking the requested paths and pruning what the caller's gitignore
//! patterns exclude. It reads no ignore files and knows no languages: callers
//! pass the patterns in, and decide which extensions mean what.

#![deny(clippy::print_stdout, clippy::print_stderr)]

/// Directories fml skips in every workspace: VCS metadata, build output,
/// dependency trees and caches.
///
/// Not applied implicitly. Pass it to [`discover`], alongside any patterns
/// read from the workspace's `.gitignore`.
pub const DEFAULT_IGNORES: &[&str] = &[
  ".git",
  "target",
  "node_modules",
  ".venv",
  "__pycache__",
  "dist",
  "build",
];
