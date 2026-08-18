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
    raw_files
      .into_iter()
      .filter(|file| !is_excluded(file, root, exclude))
      .collect()
  }
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

#[must_use]
pub fn is_excluded(path: &Path, root: &Path, exclude: &[PathBuf]) -> bool {
  if exclude.is_empty() {
    return false;
  }
  let rel_path = path.strip_prefix(root).unwrap_or(path);
  let rel_str = rel_path.to_string_lossy().replace('\\', "/");
  let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

  for ex in exclude {
    let ex_str_raw = ex.to_string_lossy();
    let ex_str = ex_str_raw.replace('\\', "/");
    let ex_trimmed = ex_str.trim_matches('/');

    // 1. Direct path prefix or exact match with full / root-relative path
    if path.starts_with(ex) || rel_path.starts_with(ex) {
      return true;
    }
    let full_ex = if ex.is_absolute() {
      ex.clone()
    } else {
      root.join(ex)
    };
    if path.starts_with(&full_ex) {
      return true;
    }

    // 2. Relative prefix, exact relative string match, or directory match
    if rel_str == ex_trimmed || rel_str.starts_with(&format!("{ex_trimmed}/")) {
      return true;
    }

    // 3. Filename match
    if file_name == ex_trimmed || file_name == ex_str_raw {
      return true;
    }

    // 4. Any path component matches
    if rel_path.components().any(|c| {
      c.as_os_str().to_string_lossy() == ex_trimmed
        || c.as_os_str() == ex.as_os_str()
    }) {
      return true;
    }

    // 5. Glob / wildcard pattern matching
    if (ex_trimmed.contains('*') || ex_trimmed.contains('?'))
      && (simple_glob_match(ex_trimmed, &rel_str)
        || simple_glob_match(ex_trimmed, file_name))
    {
      return true;
    }
  }

  false
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
