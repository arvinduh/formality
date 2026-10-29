//! Finds the files fml acts on and groups them by file extension.
//!
//! Owns walking the requested paths and pruning what the caller's gitignore
//! patterns exclude. It reads no ignore files and knows no languages: callers
//! pass the patterns in, and decide which extensions mean what.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::path;

use ignore;
use ignore::gitignore;

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

/// Builds a walk over `paths`, resolved against `root`, that skips whatever
/// the gitignore patterns in `ignores` match, anchored at `root`.
///
/// Hidden files are walked like any other, and no ignore file is read.
#[expect(dead_code, reason = "`discover` calls it in the next commit")]
fn walk(
  root: &path::Path,
  paths: &[path::PathBuf],
  ignores: &[&str],
) -> Result<ignore::Walk, ignore::Error> {
  let mut patterns = gitignore::GitignoreBuilder::new(root);
  for pattern in ignores {
    patterns.add_line(None, pattern)?;
  }
  let patterns = patterns.build()?;

  let mut starts = paths.iter().map(|path| root.join(path));
  let first = starts.next().unwrap_or_else(|| root.to_path_buf());
  let mut walk = ignore::WalkBuilder::new(first);
  for start in starts {
    walk.add(start);
  }
  walk.standard_filters(false).filter_entry(move |entry| {
    let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
    entry.depth() == 0 || !patterns.matched(entry.path(), is_dir).is_ignore()
  });
  Ok(walk.build())
}
