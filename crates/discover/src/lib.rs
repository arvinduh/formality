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

#[cfg(test)]
mod tests {
  use std::fs;

  use tempfile;

  use super::*;

  /// Creates each of `files`, a `/`-separated path, empty in a new temp dir.
  fn tree(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for file in files {
      let path = dir.path().join(file);
      fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
      fs::write(&path, "").expect("write");
    }
    dir
  }

  /// Lists the files discovered under `ext`, relative to `root`, with `/`
  /// separators on every OS.
  fn names(
    files: &collections::HashMap<String, Vec<path::PathBuf>>,
    ext: &str,
    root: &path::Path,
  ) -> Vec<String> {
    let relative = |path: &path::PathBuf| {
      let path = path.strip_prefix(root).expect("under root");
      path.to_string_lossy().replace('\\', "/")
    };
    files
      .get(ext)
      .map_or_else(Vec::new, |list| list.iter().map(relative).collect())
  }

  #[test]
  fn groups_by_lowercase_extension_and_skips_bare_names() {
    let dir = tree(&["a.rs", "B.RS", "c.py", "Makefile"]);
    let files = discover(dir.path(), &[], &[]).expect("discover");
    assert_eq!(names(&files, "rs", dir.path()), ["B.RS", "a.rs"]);
    assert_eq!(names(&files, "py", dir.path()), ["c.py"]);
    assert_eq!(files.len(), 2, "only rs and py: {files:?}");
  }

  #[test]
  fn prunes_ignored_directories_but_walks_hidden_files() {
    let dir = tree(&["src/a.rs", "target/b.rs", "node_modules/c.js", ".d.rs"]);
    let files = discover(dir.path(), &[], DEFAULT_IGNORES).expect("discover");
    assert_eq!(names(&files, "rs", dir.path()), [".d.rs", "src/a.rs"]);
    assert!(!files.contains_key("js"), "node_modules pruned: {files:?}");
  }

  #[test]
  fn walks_a_named_path_even_when_a_pattern_matches_it() {
    let dir = tree(&["target/a.rs", "src/b.rs"]);
    let paths = ["target".into()];
    let files =
      discover(dir.path(), &paths, DEFAULT_IGNORES).expect("discover");
    assert_eq!(names(&files, "rs", dir.path()), ["target/a.rs"]);
  }

  #[test]
  fn overlapping_paths_yield_each_file_once() {
    let dir = tree(&["src/a.rs"]);
    let paths = ["src".into(), "src/a.rs".into()];
    let files = discover(dir.path(), &paths, &[]).expect("discover");
    assert_eq!(names(&files, "rs", dir.path()), ["src/a.rs"]);
  }

  #[test]
  fn a_missing_named_path_is_an_error() {
    let dir = tree(&[]);
    let paths = ["missing".into()];
    assert!(discover(dir.path(), &paths, &[]).is_err(), "missing path");
  }
}
