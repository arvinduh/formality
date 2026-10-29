//! Finds the files fml acts on and groups them by file extension.
//!
//! Owns walking the requested paths and pruning what the caller's gitignore
//! patterns exclude. It reads no ignore files and knows no languages: callers
//! pass the patterns in, and decide which extensions mean what.

#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::collections;
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

/// Finds every file under `paths` and groups it by lowercase extension.
///
/// `paths` are resolved against `root`; when `paths` is empty, `root` itself
/// is walked. A file matching a pattern in `ignores` is skipped and a matching
/// directory is not entered, but a path named in `paths` is always walked.
/// Files with no extension, or a non-UTF-8 one, are left out. Each list is
/// sorted and free of duplicates, so overlapping `paths` are harmless.
///
/// # Errors
///
/// Returns an error when a pattern is not valid gitignore syntax, or when a
/// path cannot be read, including a named path that does not exist.
pub fn discover(
  root: &path::Path,
  paths: &[path::PathBuf],
  ignores: &[&str],
) -> Result<collections::HashMap<String, Vec<path::PathBuf>>, ignore::Error> {
  let mut files = collections::HashMap::<String, Vec<path::PathBuf>>::new();
  for entry in walk(root, paths, ignores)? {
    let entry = entry?;
    if !entry.file_type().is_some_and(|kind| kind.is_file()) {
      continue;
    }
    let path = entry.into_path();
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
      continue;
    };
    // Reuse the key when it exists, so most files allocate no new `String`.
    if let Some(list) = files.get_mut(ext) {
      list.push(path);
    } else {
      let key = ext.to_ascii_lowercase();
      files.entry(key).or_default().push(path);
    }
  }
  for list in files.values_mut() {
    list.sort_unstable();
    list.dedup();
  }
  Ok(files)
}

/// Builds a walk over `paths`, resolved against `root`, that skips whatever
/// the gitignore patterns in `ignores` match, anchored at `root`.
///
/// Hidden files are walked like any other, and no ignore file is read.
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
