//! File-discovery helpers: extension-based directory walking, exclude-list
//! matching, and a small dependency-free glob matcher for `exclude` patterns.

use std::path::{Path, PathBuf};

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
    let mut out = Vec::new();
    for p in targets {
      let full_p = if p.is_absolute() {
        p.clone()
      } else {
        root.join(p)
      };
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
    // Normalize each exclude pattern once up front instead of inside
    // `is_excluded`, which used to re-derive `to_string_lossy()` /
    // `replace('\\', "/")` allocations for every (file, pattern) pair —
    // O(files * excludes) allocations for what only needs to happen
    // O(excludes) times per invocation.
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
    if rel_str == ex.trimmed || rel_str.starts_with(&format!("{}/", ex.trimmed))
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
        || simple_glob_match(&ex.trimmed, file_name))
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

/// Checks a single path against a raw exclude list.
///
/// This normalizes `exclude` on every call, so callers checking many paths
/// against the same exclude list (e.g. [`find_files_with_ext`]'s internal
/// filter) should normalize once via [`NormalizedExclude`] and call
/// [`is_excluded_normalized`] directly instead of this function in a loop.
#[must_use]
pub fn is_excluded(path: &Path, root: &Path, exclude: &[PathBuf]) -> bool {
  if exclude.is_empty() {
    return false;
  }
  let normalized: Vec<NormalizedExclude<'_>> = exclude
    .iter()
    .map(|ex| NormalizedExclude::new(ex, root))
    .collect();
  is_excluded_normalized(path, root, &normalized)
}

fn walk_dir_ext(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
  let mut results = Vec::new();
  let walker = ignore::WalkBuilder::new(dir)
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .filter_entry(|entry| {
      let name = entry.file_name().to_string_lossy();
      name != "target"
        && name != "node_modules"
        && name != ".git"
        && name != ".venv"
        && name != "vendor"
        && name != "fixtures"
    })
    .build();

  for entry in walker.filter_map(Result::ok) {
    let path = entry.path();
    if path.is_file()
      && let Some(ext) = path.extension().and_then(|e| e.to_str())
      && extensions
        .iter()
        .any(|&target| target.eq_ignore_ascii_case(ext))
    {
      results.push(path.to_path_buf());
    }
  }

  results
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use std::path::PathBuf;

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
  fn test_is_excluded_standalone_function() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let excluded_file = root.join("build").join("out.rs");
    let kept_file = root.join("src").join("main.rs");

    let exclude = vec![PathBuf::from("build")];
    assert!(is_excluded(&excluded_file, root, &exclude));
    assert!(!is_excluded(&kept_file, root, &exclude));

    assert!(!is_excluded(&excluded_file, root, &[]));
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
}
