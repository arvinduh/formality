//! `.gitignore` cache hygiene diagnostics.

use crate::surfaces::LanguageSurface;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitignoreHygieneIssue {
  pub category: &'static str,
  pub missing_patterns: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitignoreHygieneReport {
  pub gitignore_exists: bool,
  pub issues: Vec<GitignoreHygieneIssue>,
}

/// Checks whether a specific pattern is ignored given `.gitignore` lines.
#[must_use]
pub fn is_pattern_ignored(lines: &[&str], entry: &str) -> bool {
  let normalized_entry = entry.trim_matches('/');
  for raw_line in lines {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
      continue;
    }
    let trimmed = line
      .trim_start_matches("**/")
      .trim_start_matches('/')
      .trim_end_matches("/**")
      .trim_end_matches('/');
    if trimmed == normalized_entry {
      return true;
    }
    if normalized_entry == "__pycache__"
      && (line == "*.py[cod]" || line == "*.pyc" || line == "*$py.class")
    {
      return true;
    }
  }
  false
}

/// Validates that cache/artifact directories for active language toolchains are ignored in `.gitignore`.
#[must_use]
pub fn check_gitignore_hygiene_content(
  gitignore_content: Option<&str>,
  has_python: bool,
  has_rust: bool,
  has_js: bool,
) -> GitignoreHygieneReport {
  let gitignore_exists = gitignore_content.is_some();
  let lines: Vec<&str> = gitignore_content.unwrap_or("").lines().collect();
  let mut issues = Vec::new();

  if has_python {
    let python_patterns: &[&'static str] =
      &[".ruff_cache", "__pycache__", ".pytest_cache"];
    let mut missing = Vec::new();
    for &pattern in python_patterns {
      if !is_pattern_ignored(&lines, pattern) {
        missing.push(pattern);
      }
    }
    if !missing.is_empty() {
      issues.push(GitignoreHygieneIssue {
        category: "Python",
        missing_patterns: missing,
      });
    }
  }

  if has_rust {
    let rust_patterns: &[&'static str] = &["target"];
    let mut missing = Vec::new();
    for &pattern in rust_patterns {
      if !is_pattern_ignored(&lines, pattern) {
        missing.push(pattern);
      }
    }
    if !missing.is_empty() {
      issues.push(GitignoreHygieneIssue {
        category: "Rust",
        missing_patterns: missing,
      });
    }
  }

  if has_js {
    let js_patterns: &[&'static str] = &["node_modules"];
    let mut missing = Vec::new();
    for &pattern in js_patterns {
      if !is_pattern_ignored(&lines, pattern) {
        missing.push(pattern);
      }
    }
    if !missing.is_empty() {
      issues.push(GitignoreHygieneIssue {
        category: "JavaScript / Node",
        missing_patterns: missing,
      });
    }
  }

  GitignoreHygieneReport {
    gitignore_exists,
    issues,
  }
}

#[must_use]
pub fn check_gitignore_hygiene(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
) -> GitignoreHygieneReport {
  let gitignore_path = root.join(".gitignore");
  let gitignore_content = std::fs::read_to_string(&gitignore_path).ok();

  let has_python = surfaces
    .iter()
    .any(|s| s.name() == "python" || s.aliases().contains(&"py"))
    || root.join("pyproject.toml").is_file()
    || root.join("requirements.txt").is_file()
    || root.join("setup.py").is_file()
    || root.join("Pipfile").is_file()
    || root.join("ruff.toml").is_file()
    || root.join(".ruff.toml").is_file();

  let has_rust = surfaces
    .iter()
    .any(|s| s.name() == "rust" || s.aliases().contains(&"rs"))
    || root.join("Cargo.toml").is_file();

  let has_js = root.join("package.json").is_file()
    || root.join("node_modules").is_dir()
    || root.join("package-lock.json").is_file()
    || root.join("yarn.lock").is_file()
    || root.join("pnpm-lock.yaml").is_file()
    || root.join("bun.lockb").is_file()
    || root.join("bun.lock").is_file()
    || surfaces.iter().any(|s| {
      let n = s.name();
      n == "markdown" || n == "yaml" || n == "json"
    });

  check_gitignore_hygiene_content(
    gitignore_content.as_deref(),
    has_python,
    has_rust,
    has_js,
  )
}
