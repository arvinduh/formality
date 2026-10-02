//! File-discovery helpers: extension-based directory walking, exclude-list
//! matching, and a small dependency-free glob matcher for `exclude` patterns.

use std::path::{Path, PathBuf};

/// Standard directories ignored across all surfaces during file discovery.
pub const STANDARD_IGNORED_DIRS: &[&str] = &[
  "target",
  "node_modules",
  ".git",
  ".venv",
  "vendor",
  "fixtures",
];

/// Returns `true` if `path` has a filename matching temporary file patterns.
#[must_use]
pub fn is_temp_file(path: &Path) -> bool {
  let is_tmp_ext = path
    .extension()
    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"));
  let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
  is_tmp_ext
    || name.eq_ignore_ascii_case(".tmp")
    || name.contains(".fml-check-tmp.")
}

/// Returns `true` if any component of `path` (relative to `root`) matches a standard ignored directory.
#[must_use]
pub fn is_standard_ignored(path: &Path, root: &Path) -> bool {
  let rel = path.strip_prefix(root).unwrap_or(path);
  rel.components().any(|c| {
    let s = c.as_os_str().to_string_lossy();
    STANDARD_IGNORED_DIRS.iter().any(|&ignored| s == ignored)
  })
}

/// Builds a [`ignore::gitignore::Gitignore`] matcher for the given repository root,
/// loading the root `.gitignore`, `.git/info/exclude`, and any nested `.gitignore` files for specific targets.
#[must_use]
pub fn build_repo_gitignore(
  root: &Path,
  targets: &[PathBuf],
) -> Option<ignore::gitignore::Gitignore> {
  let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
  let gitignore_path = root.join(".gitignore");
  if gitignore_path.is_file() {
    let _ = builder.add(&gitignore_path);
  }
  let git_info_exclude = root.join(".git").join("info").join("exclude");
  if git_info_exclude.is_file() {
    let _ = builder.add(&git_info_exclude);
  }
  for p in targets {
    let full_p = if p.is_absolute() {
      p.clone()
    } else {
      root.join(p)
    };
    let rel = full_p.strip_prefix(root).unwrap_or(&full_p);
    let mut current = root.to_path_buf();
    for comp in rel.components() {
      if let std::path::Component::Normal(c) = comp {
        current.push(c);
        let nested = current.join(".gitignore");
        if nested.is_file() {
          let _ = builder.add(&nested);
        }
      }
    }
  }
  builder.build().ok()
}

/// Walks the workspace filesystem once, discovering all regular candidate files
/// respecting gitignore rules, standard ignored directories (`target`, `node_modules`, etc.),
/// and global exclude patterns.
#[must_use]
pub fn walk_candidate_files(
  root: &Path,
  global_excludes: &[PathBuf],
) -> Vec<PathBuf> {
  let results: Vec<PathBuf> = candidate_file_paths(root)
    .map(ignore::DirEntry::into_path)
    .collect();

  if global_excludes.is_empty() {
    results
  } else {
    let normalized_exclude: Vec<NormalizedExclude<'_>> = global_excludes
      .iter()
      .map(|ex| NormalizedExclude::new(ex, root))
      .collect();
    results
      .into_iter()
      .filter(|file| !is_excluded_normalized(file, root, &normalized_exclude))
      .collect()
  }
}

/// Yields every regular candidate file under `root`. This is the one place
/// the candidate ignore rules live: gitignore, standard ignored dirs, temp
/// files.
fn candidate_file_paths(root: &Path) -> impl Iterator<Item = ignore::DirEntry> {
  ignore::WalkBuilder::new(root)
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .filter_entry(|entry| {
      let name = entry.file_name().to_string_lossy();
      if STANDARD_IGNORED_DIRS.iter().any(|&d| name == d) {
        return false;
      }
      if is_temp_file(entry.path()) {
        return false;
      }
      true
    })
    .build()
    .filter_map(Result::ok)
    .filter(|entry| entry.path().is_file())
}

/// The set of file extensions present among a workspace's candidate files,
/// compared ASCII-case-insensitively, as every surface's extension match is.
///
/// Built by one walk so that auto-detecting every surface costs one walk,
/// not one per surface.
pub struct PresentExtensions(std::collections::HashSet<String>);

impl PresentExtensions {
  /// Walks `root` once with [`walk_candidate_files`]' ignore rules and
  /// records every UTF-8 file extension seen.
  #[must_use]
  pub fn scan(root: &Path) -> Self {
    let mut present = Self(std::collections::HashSet::new());
    for entry in candidate_file_paths(root) {
      present.record(entry.path());
    }
    present
  }

  /// Records the extensions of `paths`, a candidate list the caller already
  /// walked, so detection and the runner share that one walk.
  #[must_use]
  pub fn from_paths(paths: &[PathBuf]) -> Self {
    let mut present = Self(std::collections::HashSet::new());
    for path in paths {
      present.record(path);
    }
    present
  }

  /// Adds `path`'s UTF-8 extension, allocating only for a new one.
  fn record(&mut self, path: &Path) {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
      return;
    };
    let ext = ascii_lowercase(ext);
    if !self.0.contains(ext.as_ref()) {
      self.0.insert(ext.into_owned());
    }
  }

  /// Whether a candidate file with extension `ext` exists, ignoring ASCII
  /// case.
  #[must_use]
  pub fn contains(&self, ext: &str) -> bool {
    self.0.contains(ascii_lowercase(ext).as_ref())
  }
}

/// Lowercases `s`, allocating only when it holds an ASCII uppercase letter.
fn ascii_lowercase(s: &str) -> std::borrow::Cow<'_, str> {
  if s.bytes().any(|b| b.is_ascii_uppercase()) {
    std::borrow::Cow::Owned(s.to_ascii_lowercase())
  } else {
    std::borrow::Cow::Borrowed(s)
  }
}

/// Filters in-memory candidate files matching surface extensions, explicit include patterns, and exclude patterns.
#[must_use]
pub fn filter_candidates_with_ext(
  candidates: &[PathBuf],
  extensions: &[&str],
  includes: &[String],
  excludes: &[PathBuf],
) -> Vec<PathBuf> {
  if extensions.is_empty() {
    return Vec::new();
  }

  candidates
    .iter()
    .filter(|path| {
      let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
      };
      if !extensions
        .iter()
        .any(|&target| target.eq_ignore_ascii_case(ext))
      {
        return false;
      }

      if !includes.is_empty()
        && !includes.iter().any(|inc| matches_pattern(path, inc))
      {
        return false;
      }

      if !excludes.is_empty()
        && excludes
          .iter()
          .any(|ex| matches_pattern(path, &ex.to_string_lossy()))
      {
        return false;
      }

      true
    })
    .cloned()
    .collect()
}

/// Matches a file path against a pattern (glob, exact filename, directory, or path suffix).
#[must_use]
pub fn matches_pattern(path: &Path, pattern: &str) -> bool {
  let norm_pattern = pattern.replace('\\', "/");
  let trimmed = norm_pattern.trim_matches('/');
  let slash_path = path.to_string_lossy().replace('\\', "/");
  let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

  // 1. Direct path, filename, or trimmed match
  if file_name == trimmed || file_name == norm_pattern || slash_path == trimmed
  {
    return true;
  }

  // 2. Relative prefix, suffix, or directory match
  if slash_path.ends_with(&format!("/{trimmed}"))
    || slash_path.starts_with(&format!("{trimmed}/"))
    || slash_path.contains(&format!("/{trimmed}/"))
  {
    return true;
  }

  // 3. Path component match
  if path
    .components()
    .any(|c| c.as_os_str().to_string_lossy() == trimmed)
  {
    return true;
  }

  // 4. Glob pattern match
  if trimmed.contains('*') || trimmed.contains('?') {
    if simple_glob_match(trimmed, file_name)
      || simple_glob_match(trimmed, &slash_path)
    {
      return true;
    }
    let glob_with_star = format!("**/{trimmed}");
    if simple_glob_match(&glob_with_star, &slash_path) {
      return true;
    }
  }

  // 5. Standard Path starts_with
  if path.starts_with(pattern) {
    return true;
  }

  false
}

/// Helper function to find matching files within a directory ignoring .git, target, `node_modules`, etc.
#[must_use]
pub fn find_files_with_ext(
  root: &Path,
  extensions: &[&str],
  specific_paths: &[PathBuf],
  files_override: &[PathBuf],
  exclude: &[PathBuf],
) -> Vec<PathBuf> {
  let targets = if !specific_paths.is_empty() {
    specific_paths
  } else if !files_override.is_empty() {
    files_override
  } else {
    &[]
  };

  let raw_files = if targets.is_empty() {
    walk_dir_ext(root, extensions)
  } else {
    let repo_gitignore = build_repo_gitignore(root, targets);
    let mut out = Vec::new();
    for p in targets {
      let full_p = if p.is_absolute() {
        p.clone()
      } else {
        root.join(p)
      };
      if is_standard_ignored(&full_p, root) || is_temp_file(&full_p) {
        continue;
      }
      if let Some(ref gi) = repo_gitignore
        && gi
          .matched_path_or_any_parents(&full_p, full_p.is_dir())
          .is_ignore()
      {
        continue;
      }
      if full_p.is_file()
        && let Some(ext) = full_p.extension().and_then(|e| e.to_str())
        && extensions
          .iter()
          .any(|&target| target.eq_ignore_ascii_case(ext))
      {
        out.push(full_p);
      } else if full_p.is_dir() {
        out.extend(walk_dir_ext(&full_p, extensions));
      }
    }
    out
  };

  if exclude.is_empty() {
    raw_files
  } else {
    // Normalize each exclude pattern once up front instead of re-deriving
    // `to_string_lossy()` / `replace('\\', "/")` allocations for every
    // (file, pattern) pair: O(excludes) allocations, not O(files * excludes).
    let normalized_exclude: Vec<NormalizedExclude<'_>> = exclude
      .iter()
      .map(|ex| NormalizedExclude::new(ex, root))
      .collect();
    raw_files
      .into_iter()
      .filter(|file| !is_excluded_normalized(file, root, &normalized_exclude))
      .collect()
  }
}

struct NormalizedExclude<'a> {
  raw: &'a Path,
  /// `raw` re-joined against `root` when it was relative, so an absolute
  /// prefix check can be done without reallocating per file.
  absolute: PathBuf,
  slash_normalized: String,
  trimmed: String,
}

impl<'a> NormalizedExclude<'a> {
  fn new(raw: &'a PathBuf, root: &Path) -> Self {
    let slash_normalized = raw.to_string_lossy().replace('\\', "/");
    let trimmed = slash_normalized.trim_matches('/').to_string();
    let absolute = if raw.is_absolute() {
      raw.clone()
    } else {
      root.join(raw)
    };
    NormalizedExclude {
      raw,
      absolute,
      slash_normalized,
      trimmed,
    }
  }
}

#[must_use]
fn is_excluded_normalized(
  path: &Path,
  root: &Path,
  exclude: &[NormalizedExclude<'_>],
) -> bool {
  if exclude.is_empty() {
    return false;
  }
  let rel_path = path.strip_prefix(root).unwrap_or(path);
  let rel_str = rel_path.to_string_lossy().replace('\\', "/");
  let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

  for ex in exclude {
    // 1. Direct path prefix or exact match with full / root-relative path
    if path.starts_with(ex.raw) || rel_path.starts_with(ex.raw) {
      return true;
    }
    if path.starts_with(&ex.absolute) {
      return true;
    }

    // 2. Relative prefix, exact relative string match, or directory match
    if rel_str == ex.trimmed
      || rel_str
        .strip_prefix(ex.trimmed.as_str())
        .is_some_and(|rest| rest.starts_with('/'))
      || rel_str.contains(&format!("/{}/", ex.trimmed))
      || rel_str.ends_with(&format!("/{}", ex.trimmed))
    {
      return true;
    }

    // 3. Filename match
    if file_name == ex.trimmed || file_name == ex.slash_normalized {
      return true;
    }

    // 4. Any path component matches
    if rel_path.components().any(|c| {
      c.as_os_str().to_string_lossy() == ex.trimmed
        || c.as_os_str() == ex.raw.as_os_str()
    }) {
      return true;
    }

    // 5. Glob / wildcard pattern matching
    if (ex.trimmed.contains('*') || ex.trimmed.contains('?'))
      && (simple_glob_match(&ex.trimmed, &rel_str)
        || simple_glob_match(&ex.trimmed, file_name)
        || simple_glob_match(&format!("**/{}", ex.trimmed), &rel_str))
    {
      return true;
    }

    // 6. Direct pattern match via matches_pattern
    if matches_pattern(rel_path, &ex.slash_normalized)
      || matches_pattern(path, &ex.slash_normalized)
    {
      return true;
    }
  }

  false
}

/// Performs simple glob matching supporting `*` and `?` wildcard patterns.
#[must_use]
pub fn simple_glob_match(pattern: &str, text: &str) -> bool {
  let norm_pattern = pattern.replace('\\', "/");
  let norm_text = text.replace('\\', "/");
  glob_match_slices(norm_pattern.as_bytes(), norm_text.as_bytes())
}

fn glob_match_slices(pattern: &[u8], text: &[u8]) -> bool {
  if pattern.is_empty() {
    return text.is_empty();
  }

  if pattern.starts_with(b"**") {
    let mut rest_pat = &pattern[2..];
    if rest_pat.starts_with(b"/") {
      rest_pat = &rest_pat[1..];
    }
    for i in 0..=text.len() {
      if glob_match_slices(rest_pat, &text[i..]) {
        return true;
      }
    }
    return false;
  }

  if pattern[0] == b'*' {
    let rest_pat = &pattern[1..];
    for i in 0..=text.len() {
      if i > 0 && text[i - 1] == b'/' {
        break;
      }
      if glob_match_slices(rest_pat, &text[i..]) {
        return true;
      }
    }
    return false;
  }

  if text.is_empty() {
    return false;
  }

  if pattern[0] == b'?' {
    if text[0] == b'/' {
      return false;
    }
    return glob_match_slices(&pattern[1..], &text[1..]);
  }

  if pattern[0] == text[0] {
    return glob_match_slices(&pattern[1..], &text[1..]);
  }

  false
}

/// Walks `start` and each of its ancestor directories looking for a manifest
/// file named `filename`, mirroring how build tools (`cargo`, `go`) resolve a
/// project root from a subdirectory. Shared by [`crate::surfaces::rust`]'s
/// `Cargo.toml` guard and [`crate::surfaces::go`]'s `go.mod` guard (Fixes
/// #185) so a subdirectory of a real project isn't mistaken for one with no
/// manifest at all.
///
/// Uses `.is_file()`, not `.exists()`, so a directory that happens to share
/// the manifest's name isn't mistaken for one.
#[must_use]
pub fn find_manifest_upwards(start: &Path, filename: &str) -> bool {
  start.ancestors().any(|dir| dir.join(filename).is_file())
}

fn walk_dir_ext(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
  walk_candidate_files(dir, &[])
    .into_iter()
    .filter(|path| {
      path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
          extensions
            .iter()
            .any(|&target| target.eq_ignore_ascii_case(ext))
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  #[test]
  fn test_present_extensions_ignores_case_and_ignored_dirs() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::write(root.join("a/b/Main.RS"), "").unwrap();
    std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
    std::fs::write(root.join("node_modules/x/a.js"), "").unwrap();
    std::fs::create_dir(root.join("dir.py")).unwrap();

    let present = PresentExtensions::scan(root);
    assert!(present.contains("rs"));
    assert!(present.contains("Rs"));
    assert!(!present.contains("js"));
    assert!(!present.contains("py"));
  }

  #[test]
  fn test_present_extensions_from_paths_reads_only_the_given_list() {
    let paths = [PathBuf::from("a/Main.RS"), PathBuf::from("b/notes")];
    let present = PresentExtensions::from_paths(&paths);
    assert!(present.contains("rs"));
    assert!(!present.contains("md"));
  }

  #[test]
  fn test_find_manifest_upwards_walks_parent_directories() {
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("Cargo.toml"), "[package]\n").unwrap();
    let nested = temp.path().join("src").join("deep");
    std::fs::create_dir_all(&nested).unwrap();

    assert!(find_manifest_upwards(&nested, "Cargo.toml"));
  }

  #[test]
  fn test_find_manifest_upwards_no_manifest_anywhere() {
    let temp = tempfile::TempDir::new().unwrap();
    let nested = temp.path().join("src");
    std::fs::create_dir_all(&nested).unwrap();

    assert!(!find_manifest_upwards(&nested, "Cargo.toml"));
  }

  #[test]
  fn test_find_manifest_upwards_directory_named_like_manifest_is_ignored() {
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(temp.path().join("Cargo.toml")).unwrap();

    assert!(!find_manifest_upwards(temp.path(), "Cargo.toml"));
  }

  #[test]
  fn test_find_files_with_ext_files_override() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let file_a = root.join("a.rs");
    let file_b = root.join("b.rs");
    let file_c = root.join("c.rs");
    std::fs::write(&file_a, "fn a() {}").unwrap();
    std::fs::write(&file_b, "fn b() {}").unwrap();
    std::fs::write(&file_c, "fn c() {}").unwrap();

    let files_override = vec![PathBuf::from("a.rs"), PathBuf::from("c.rs")];
    let matched = find_files_with_ext(root, &["rs"], &[], &files_override, &[]);
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&file_a));
    assert!(matched.contains(&file_c));
    assert!(!matched.contains(&file_b));
  }

  #[test]
  fn test_find_files_with_ext_exclude_patterns() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let src_dir = root.join("src");
    let gen_dir = src_dir.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();

    let normal = src_dir.join("main.rs");
    let generated = gen_dir.join("api.rs");
    let ignored = src_dir.join("ignored.rs");
    std::fs::write(&normal, "fn main() {}").unwrap();
    std::fs::write(&generated, "fn api() {}").unwrap();
    std::fs::write(&ignored, "fn ignored() {}").unwrap();

    let exclude =
      vec![PathBuf::from("src/generated"), PathBuf::from("ignored.rs")];
    let matched = find_files_with_ext(root, &["rs"], &[], &[], &exclude);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0], normal);
  }

  #[test]
  fn test_find_files_with_ext_specific_paths_precedence() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let file_a = root.join("a.rs");
    let file_b = root.join("b.rs");
    std::fs::write(&file_a, "fn a() {}").unwrap();
    std::fs::write(&file_b, "fn b() {}").unwrap();

    let specific = vec![PathBuf::from("a.rs")];
    let files_override = vec![PathBuf::from("b.rs")];
    let matched =
      find_files_with_ext(root, &["rs"], &specific, &files_override, &[]);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0], file_a);
  }

  #[test]
  fn test_find_files_with_ext_default_walk_finds_nested_files() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let nested = root.join("src").join("nested");
    std::fs::create_dir_all(&nested).unwrap();

    let top = root.join("main.rs");
    let deep = nested.join("deep.rs");
    let wrong_ext = root.join("readme.md");
    std::fs::write(&top, "fn main() {}").unwrap();
    std::fs::write(&deep, "fn deep() {}").unwrap();
    std::fs::write(&wrong_ext, "# readme").unwrap();

    let matched = find_files_with_ext(root, &["rs"], &[], &[], &[]);
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&top));
    assert!(matched.contains(&deep));
    assert!(!matched.contains(&wrong_ext));
  }

  #[test]
  fn test_walk_dir_ext_skips_conventional_ignored_directories() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let real = root.join("src");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("lib.rs"), "fn lib() {}").unwrap();

    for ignored_dir in ["target", "node_modules", ".venv", "vendor", "fixtures"]
    {
      let dir = root.join(ignored_dir);
      std::fs::create_dir_all(&dir).unwrap();
      std::fs::write(dir.join("should_not_be_found.rs"), "fn x() {}").unwrap();
    }

    let matched = find_files_with_ext(root, &["rs"], &[], &[], &[]);
    assert_eq!(
      matched.len(),
      1,
      "only src/lib.rs should be found; ignored dirs must be skipped: {matched:?}"
    );
    assert!(matched[0].ends_with("lib.rs"));
  }

  #[test]
  fn test_walk_dir_ext_skips_temporary_files() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let real_rs = root.join("main.rs");
    let temp_fml = root.join("main.fml-check-tmp.rs");
    let temp_ext = root.join("main.rs.tmp");
    let temp_bare = root.join("scratch.tmp");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let nested_real = nested.join("lib.rs");
    let nested_temp = nested.join("lib.fml-check-tmp.rs");

    std::fs::write(&real_rs, "fn main() {}").unwrap();
    std::fs::write(&temp_fml, "fn main() {}").unwrap();
    std::fs::write(&temp_ext, "fn main() {}").unwrap();
    std::fs::write(&temp_bare, "fn main() {}").unwrap();
    std::fs::write(&nested_real, "fn lib() {}").unwrap();
    std::fs::write(&nested_temp, "fn lib() {}").unwrap();

    let matched = find_files_with_ext(root, &["rs", "tmp"], &[], &[], &[]);
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&real_rs));
    assert!(matched.contains(&nested_real));
    assert!(!matched.contains(&temp_fml));
    assert!(!matched.contains(&temp_ext));
    assert!(!matched.contains(&temp_bare));
    assert!(!matched.contains(&nested_temp));
  }

  #[test]
  fn test_is_excluded_normalized_matches_directory_prefix() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let excluded_file = root.join("build").join("out.rs");
    let kept_file = root.join("src").join("main.rs");

    let build = PathBuf::from("build");
    let exclude = [NormalizedExclude::new(&build, root)];
    assert!(is_excluded_normalized(&excluded_file, root, &exclude));
    assert!(!is_excluded_normalized(&kept_file, root, &exclude));

    assert!(!is_excluded_normalized(&excluded_file, root, &[]));
  }

  #[test]
  fn test_simple_glob_match() {
    assert!(simple_glob_match("*.rs", "main.rs"));
    assert!(!simple_glob_match("*.rs", "src/main.rs"));
    assert!(!simple_glob_match("*.rs", "src\\main.rs"));
    assert!(simple_glob_match("src/*.rs", "src/main.rs"));
    assert!(simple_glob_match("src/*.rs", "src/lib.rs"));
    assert!(simple_glob_match("src/*.rs", "src\\lib.rs"));
    assert!(simple_glob_match("src\\*.rs", "src/lib.rs"));
    assert!(!simple_glob_match("src/*.rs", "src/sub/lib.rs"));
    assert!(!simple_glob_match("src/*.rs", "src\\sub\\lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src/lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src\\lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src/sub/lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src\\sub\\lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src/gen/api.rs"));
    assert!(simple_glob_match("src/**/api.rs", "src/gen/api.rs"));
    assert!(simple_glob_match("*.toml", "Cargo.toml"));
    assert!(!simple_glob_match("*.toml", "src/Cargo.toml"));
    assert!(simple_glob_match("target/*", "target/debug"));
    assert!(simple_glob_match("target/*", "target\\debug"));
    assert!(!simple_glob_match("target/*", "target/debug/app"));
    assert!(!simple_glob_match("target/*", "target\\debug\\app"));
    assert!(simple_glob_match("target/**", "target/debug/app"));
    assert!(simple_glob_match("target/**", "target\\debug\\app"));
    assert!(simple_glob_match("**/*.rs", "main.rs"));
    assert!(simple_glob_match("**/*.rs", "src/lib.rs"));
    assert!(simple_glob_match("**/*.rs", "src/sub/lib.rs"));
    assert!(simple_glob_match("test?.rs", "test1.rs"));
    assert!(!simple_glob_match("*.py", "main.rs"));
    assert!(!simple_glob_match("test?.rs", "test12.rs"));
    assert!(!simple_glob_match("test?.rs", "test/a.rs"));
  }

  #[test]
  fn test_walk_candidate_files_discovers_files_and_respects_global_exclude() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let src = root.join("src");
    let build = root.join("build");
    let target = root.join("target");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&build).unwrap();
    std::fs::create_dir_all(&target).unwrap();

    let main_rs = src.join("main.rs");
    let build_rs = build.join("generated.rs");
    let target_rs = target.join("lib.rs");
    let readme = root.join("README.md");

    std::fs::write(&main_rs, "fn main() {}").unwrap();
    std::fs::write(&build_rs, "fn gen() {}").unwrap();
    std::fs::write(&target_rs, "fn ignored() {}").unwrap();
    std::fs::write(&readme, "# Readme").unwrap();

    // 1. Without global excludes (target directory is automatically skipped)
    let candidates = walk_candidate_files(root, &[]);
    assert_eq!(candidates.len(), 3);
    assert!(candidates.contains(&main_rs));
    assert!(candidates.contains(&build_rs));
    assert!(candidates.contains(&readme));
    assert!(!candidates.contains(&target_rs));

    // 2. With global exclude for build directory
    let candidates_excluded =
      walk_candidate_files(root, &[PathBuf::from("build")]);
    assert_eq!(candidates_excluded.len(), 2);
    assert!(candidates_excluded.contains(&main_rs));
    assert!(candidates_excluded.contains(&readme));
    assert!(!candidates_excluded.contains(&build_rs));
  }

  #[test]
  fn test_filter_candidates_with_ext_in_memory() {
    let candidates = vec![
      PathBuf::from("/repo/src/main.rs"),
      PathBuf::from("/repo/src/lib.rs"),
      PathBuf::from("/repo/src/generated/api.rs"),
      PathBuf::from("/repo/scripts/run.py"),
      PathBuf::from("/repo/README.md"),
    ];

    let rust_ext = crate::surfaces::LanguageSurface::file_extensions(
      &crate::surfaces::rust::RustSurface,
    );
    let python_ext = crate::surfaces::LanguageSurface::file_extensions(
      &crate::surfaces::python::PythonSurface,
    );

    // 1. Rust surface default matching
    let rust_files =
      filter_candidates_with_ext(&candidates, rust_ext, &[], &[]);
    assert_eq!(rust_files.len(), 3);
    assert!(rust_files.contains(&PathBuf::from("/repo/src/main.rs")));
    assert!(rust_files.contains(&PathBuf::from("/repo/src/lib.rs")));
    assert!(rust_files.contains(&PathBuf::from("/repo/src/generated/api.rs")));

    // 2. Rust surface with exclude
    let rust_filtered = filter_candidates_with_ext(
      &candidates,
      rust_ext,
      &[],
      &[PathBuf::from("src/generated")],
    );
    assert_eq!(rust_filtered.len(), 2);
    assert!(rust_filtered.contains(&PathBuf::from("/repo/src/main.rs")));
    assert!(rust_filtered.contains(&PathBuf::from("/repo/src/lib.rs")));

    // 3. Rust surface with explicit includes
    let rust_included = filter_candidates_with_ext(
      &candidates,
      rust_ext,
      &["src/main.rs".to_string()],
      &[],
    );
    assert_eq!(rust_included.len(), 1);
    assert_eq!(rust_included[0], PathBuf::from("/repo/src/main.rs"));

    // 4. Python surface
    let py_files =
      filter_candidates_with_ext(&candidates, python_ext, &[], &[]);
    assert_eq!(py_files.len(), 1);
    assert_eq!(py_files[0], PathBuf::from("/repo/scripts/run.py"));
  }

  #[test]
  fn test_matches_pattern_variants() {
    let p = Path::new("/workspace/src/generated/api.rs");
    assert!(matches_pattern(p, "src/generated"));
    assert!(matches_pattern(p, "generated"));
    assert!(matches_pattern(p, "api.rs"));
    assert!(matches_pattern(p, "src/**/*.rs"));
    assert!(matches_pattern(p, "*.rs"));
    assert!(!matches_pattern(p, "main.rs"));
    assert!(!matches_pattern(p, "src/other"));
  }

  #[test]
  fn test_standard_ignored_and_temp_files() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let ignored = |p: &Path| is_standard_ignored(p, root) || is_temp_file(p);

    assert!(ignored(
      &root.join("editors/vscode/test/fixtures/bin/mock-fml.js")
    ));
    assert!(ignored(&root.join("target/debug/fml")));
    assert!(ignored(&root.join("node_modules/pkg/index.js")));
    assert!(ignored(&root.join(".venv/lib/python.py")));
    assert!(ignored(&root.join("vendor/bundle/x")));
    assert!(ignored(&root.join(".git/config")));
    assert!(ignored(&root.join("scratch.tmp")));
    assert!(ignored(&root.join("scratch.TMP")));
    assert!(ignored(&root.join(".tmp")));
    assert!(ignored(&root.join("main.fml-check-tmp.rs")));
    assert!(!ignored(&root.join("src/main.rs")));
    assert!(!ignored(&root.join("editors/vscode/src/extension.ts")));
  }

  #[test]
  fn test_find_files_with_ext_staged_scope_filtering() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let src = root.join("src");
    let fixtures = root.join("fixtures");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&fixtures).unwrap();

    let main_rs = src.join("main.rs");
    let excluded_rs = src.join("generated.rs");
    let fixture_rs = fixtures.join("mock.rs");
    let ignored_rs = root.join("ignored.rs");

    std::fs::write(&main_rs, "fn main() {}\n").unwrap();
    std::fs::write(&excluded_rs, "fn gen() {}\n").unwrap();
    std::fs::write(&fixture_rs, "fn mock() {}\n").unwrap();
    std::fs::write(&ignored_rs, "fn ig() {}\n").unwrap();
    std::fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();

    let specific_staged =
      vec![main_rs.clone(), excluded_rs, fixture_rs, ignored_rs];
    let exclude = vec![PathBuf::from("src/generated.rs")];

    let matched =
      find_files_with_ext(root, &["rs"], &specific_staged, &[], &exclude);

    // Only main.rs survives: excluded_rs is filtered by exclude,
    // fixture_rs is filtered by conventional dir, and ignored_rs is filtered by .gitignore
    assert_eq!(matched, vec![main_rs]);
  }
}
