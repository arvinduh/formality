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
#[path = "glob_tests.rs"]
mod tests;
