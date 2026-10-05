//! Git path resolution and repository change discovery.
//!
//! Owns discovering staged or modified uncommitted files via the `git` CLI,
//! path normalization, and filtering git-discovered files against explicit
//! path arguments.

use std::path;

use crate::errors;

/// Normalizes path components by resolving `.` and `..` segments lexically.
#[must_use]
pub fn normalize_path(path: &path::Path) -> path::PathBuf {
  let mut components = Vec::new();
  for component in path.components() {
    match component {
      path::Component::CurDir => {}
      path::Component::ParentDir => {
        if let Some(path::Component::Normal(_)) = components.last() {
          components.pop();
        } else {
          components.push(component);
        }
      }
      c => components.push(c),
    }
  }
  components.iter().collect()
}

/// Resolves the target file paths for a command from its `--staged`/
/// `--changed`/explicit-path flags. `staged` and `changed` are mutually
/// exclusive; if neither is set, `explicit_paths` is returned as-is.
/// When `staged` or `changed` is set alongside `explicit_paths`, the
/// git-discovered file list is filtered to only include files matching
/// the explicit paths.
///
/// # Errors
///
/// Returns a [`errors::FormalityError`] if both `staged` and `changed` are set, or if
/// the underlying git query fails.
pub fn resolve_git_paths(
  root: &path::Path,
  staged: bool,
  changed: bool,
  explicit_paths: Vec<path::PathBuf>,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  if staged && changed {
    return Err(errors::FormalityError::Git(
      errors::GitError::MutuallyExclusiveFlags,
    ));
  }
  if !staged && !changed {
    return Ok(explicit_paths);
  }

  let git_files = if staged {
    get_git_staged_files(root)?
  } else {
    get_git_changed_files(root)?
  };

  if explicit_paths.is_empty() {
    return Ok(git_files);
  }

  let norm_root = normalize_path(root);
  let normalized_explicit: Vec<(path::PathBuf, path::PathBuf)> = explicit_paths
    .iter()
    .map(|p| {
      let abs = if p.is_absolute() {
        normalize_path(p)
      } else {
        normalize_path(&norm_root.join(p))
      };
      let rel = normalize_path(p);
      (abs, rel)
    })
    .collect();

  let filtered = git_files
    .into_iter()
    .filter(|file| {
      let norm_file = normalize_path(file);
      let norm_rel_file =
        norm_file.strip_prefix(&norm_root).unwrap_or(&norm_file);
      normalized_explicit.iter().any(|(abs_exp, rel_exp)| {
        norm_file.starts_with(abs_exp)
          || norm_rel_file.starts_with(rel_exp)
          || norm_rel_file.starts_with(abs_exp)
      })
    })
    .collect();

  Ok(filtered)
}

fn get_git_diff_files(
  root: &path::Path,
  staged: bool,
  error_context: &str,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  let mut cmd = std::process::Command::new("git");
  cmd.arg("diff").arg("--name-only");
  if staged {
    cmd.arg("--cached");
  }
  cmd.arg("--diff-filter=ACMR").current_dir(root);

  let output = cmd.output().map_err(|e| {
    errors::FormalityError::Git(errors::GitError::ExecutionFailed(
      e.to_string(),
    ))
  })?;

  if !output.status.success() {
    return Err(errors::FormalityError::Git(
      errors::GitError::CommandFailed(format!(
        "Failed to query git {error_context} files."
      )),
    ));
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let files: Vec<path::PathBuf> = stdout
    .lines()
    .map(|l| root.join(l.trim()))
    .filter(|p| p.is_file())
    .collect();

  Ok(files)
}

/// Returns the list of staged git files relative to `root`.
///
/// # Errors
///
/// Returns a [`errors::FormalityError`] if git execution fails or the git command cannot be run.
pub fn get_git_staged_files(
  root: &path::Path,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  get_git_diff_files(root, true, "staged")
}

/// Returns the list of changed git files relative to `root`.
///
/// # Errors
///
/// Returns a [`errors::FormalityError`] if git execution fails or the git command cannot be run.
pub fn get_git_changed_files(
  root: &path::Path,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  get_git_diff_files(root, false, "changed")
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path;

  use super::*;

  #[test]
  fn test_normalize_path_components() {
    let p = path::Path::new("a/b/../c/./d");
    let norm = normalize_path(p);
    assert_eq!(norm, path::PathBuf::from("a/c/d"));

    let p2 = path::Path::new("./a/b");
    let norm2 = normalize_path(p2);
    assert_eq!(norm2, path::PathBuf::from("a/b"));
  }

  #[test]
  fn test_resolve_git_paths_mutual_exclusion() {
    let res = resolve_git_paths(path::Path::new("."), true, true, vec![]);
    assert!(matches!(
      res,
      Err(errors::FormalityError::Git(
        errors::GitError::MutuallyExclusiveFlags
      ))
    ));
  }

  #[test]
  fn test_resolve_git_paths_no_git_flags_returns_explicit() {
    let explicit = vec![
      path::PathBuf::from("src/main.rs"),
      path::PathBuf::from("README.md"),
    ];
    let res =
      resolve_git_paths(path::Path::new("."), false, false, explicit.clone())
        .unwrap();
    assert_eq!(res, explicit);
  }

  #[test]
  fn test_resolve_git_paths_staged_filtering() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    // Initialize git repository
    let init_ok = std::process::Command::new("git")
      .arg("init")
      .current_dir(root)
      .output()
      .is_ok_and(|o| o.status.success());
    if !init_ok {
      return;
    }

    // Configure user name/email for commit
    let _ = std::process::Command::new("git")
      .args(["config", "user.name", "test"])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["config", "user.email", "test@example.com"])
      .current_dir(root)
      .output();

    let src = root.join("src");
    let tests = root.join("tests");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&tests).unwrap();

    let file_a = src.join("a.rs");
    let file_b = src.join("b.rs");
    let file_c = tests.join("c.rs");
    fs::write(&file_a, "fn a() {}\n").unwrap();
    fs::write(&file_b, "fn b() {}\n").unwrap();
    fs::write(&file_c, "fn c() {}\n").unwrap();

    // Initial commit so HEAD exists
    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["commit", "-m", "initial"])
      .current_dir(root)
      .output();

    // Modify all 3 files and stage file_a and file_c
    fs::write(&file_a, "fn a_mod() {}\n").unwrap();
    fs::write(&file_b, "fn b_mod() {}\n").unwrap();
    fs::write(&file_c, "fn c_mod() {}\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "src/a.rs", "tests/c.rs"])
      .current_dir(root)
      .output();

    // 1. Staged without explicit paths returns both staged files
    let staged_all = resolve_git_paths(root, true, false, vec![]).unwrap();
    assert_eq!(staged_all.len(), 2);
    assert!(staged_all.contains(&file_a));
    assert!(staged_all.contains(&file_c));
    assert!(!staged_all.contains(&file_b));

    // 2. Staged filtered by explicit directory "src"
    let staged_src =
      resolve_git_paths(root, true, false, vec![path::PathBuf::from("src")])
        .unwrap();
    assert_eq!(staged_src, vec![file_a]);

    // 3. Staged filtered by explicit file "tests/c.rs"
    let staged_file = resolve_git_paths(
      root,
      true,
      false,
      vec![path::PathBuf::from("tests/c.rs")],
    )
    .unwrap();
    assert_eq!(staged_file, vec![file_c]);

    // 4. Staged filtered by non-matching explicit path returns empty
    let staged_none = resolve_git_paths(
      root,
      true,
      false,
      vec![path::PathBuf::from("nonexistent")],
    )
    .unwrap();
    assert!(staged_none.is_empty());
  }

  #[test]
  fn test_resolve_git_paths_changed_filtering() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let init_ok = std::process::Command::new("git")
      .arg("init")
      .current_dir(root)
      .output()
      .is_ok_and(|o| o.status.success());
    if !init_ok {
      return;
    }

    let _ = std::process::Command::new("git")
      .args(["config", "user.name", "test"])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["config", "user.email", "test@example.com"])
      .current_dir(root)
      .output();

    let file_tracked = root.join("tracked.txt");
    fs::write(&file_tracked, "v1\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["commit", "-m", "initial"])
      .current_dir(root)
      .output();

    // Modify tracked file (unstaged)
    fs::write(&file_tracked, "v2\n").unwrap();

    let changed = resolve_git_paths(root, false, true, vec![]).unwrap();
    assert_eq!(changed, vec![file_tracked.clone()]);

    let changed_filtered = resolve_git_paths(
      root,
      false,
      true,
      vec![path::PathBuf::from("tracked.txt")],
    )
    .unwrap();
    assert_eq!(changed_filtered, vec![file_tracked]);

    let changed_unmatched = resolve_git_paths(
      root,
      false,
      true,
      vec![path::PathBuf::from("other.txt")],
    )
    .unwrap();
    assert!(changed_unmatched.is_empty());
  }
}
