use super::*;
use tempfile::tempdir;

#[test]
fn test_detect_virtualenv_from_env_var() {
  let temp = tempdir().unwrap();
  let mock_venv = temp.path().join("custom_venv");
  std::fs::create_dir_all(&mock_venv).unwrap();

  let info = detect_virtualenv_with_env(temp.path(), Some(mock_venv.clone()));
  assert!(info.is_active);
  assert_eq!(info.venv_path, Some(mock_venv));
  assert_eq!(info.source, VirtualEnvSource::EnvVar);
}

#[test]
fn test_detect_virtualenv_from_workspace_dirs() {
  for dir_name in &[".venv", "venv", "env", ".env"] {
    let temp = tempdir().unwrap();
    let venv_dir = temp.path().join(dir_name);
    std::fs::create_dir_all(&venv_dir).unwrap();

    let info = detect_virtualenv_with_env(temp.path(), None);
    assert!(!info.is_active);
    assert_eq!(info.venv_path, Some(venv_dir));
    assert_eq!(
      info.source,
      VirtualEnvSource::Workspace(dir_name.to_string())
    );
  }
}

#[test]
fn test_detect_virtualenv_precedence() {
  let temp = tempdir().unwrap();
  let dot_venv = temp.path().join(".venv");
  let venv = temp.path().join("venv");
  std::fs::create_dir_all(&dot_venv).unwrap();
  std::fs::create_dir_all(&venv).unwrap();

  let info = detect_virtualenv_with_env(temp.path(), None);
  assert_eq!(info.venv_path, Some(dot_venv));
  assert_eq!(
    info.source,
    VirtualEnvSource::Workspace(".venv".to_string())
  );
}

#[test]
fn test_detect_virtualenv_none() {
  let temp = tempdir().unwrap();
  let info = detect_virtualenv_with_env(temp.path(), None);
  assert!(!info.is_active);
  assert_eq!(info.venv_path, None);
  assert_eq!(info.source, VirtualEnvSource::None);
}

#[test]
fn test_find_venv_interpreter() {
  let temp = tempdir().unwrap();
  let bin_dir = temp.path().join("bin");
  std::fs::create_dir_all(&bin_dir).unwrap();
  let python_bin = bin_dir.join("python");
  std::fs::write(&python_bin, "#!/bin/sh\n").unwrap();

  let found = find_venv_interpreter(temp.path());
  assert_eq!(found, Some(python_bin));
}

#[test]
fn test_is_pattern_ignored() {
  let lines = vec![
    "# Comments should be ignored",
    "",
    "target/",
    "/.ruff_cache/",
    "__pycache__",
    "**/node_modules/**",
    "!not_ignored",
  ];

  assert!(is_pattern_ignored(&lines, "target"));
  assert!(is_pattern_ignored(&lines, ".ruff_cache"));
  assert!(is_pattern_ignored(&lines, "__pycache__"));
  assert!(is_pattern_ignored(&lines, "node_modules"));
  assert!(!is_pattern_ignored(&lines, ".pytest_cache"));
  assert!(!is_pattern_ignored(&lines, "not_ignored"));
}

#[test]
fn test_is_pattern_ignored_pyc_alias() {
  let lines = vec!["*.pyc"];
  assert!(is_pattern_ignored(&lines, "__pycache__"));
}

#[test]
fn test_check_gitignore_hygiene_all_satisfied() {
  let gitignore = r"
/target/
.ruff_cache/
__pycache__/
.pytest_cache/
node_modules/
";
  let report = check_gitignore_hygiene_content(
    Some(gitignore),
    true, // has_python
    true, // has_rust
    true, // has_js
  );
  assert!(report.gitignore_exists);
  assert!(report.issues.is_empty());
}

#[test]
fn test_check_gitignore_hygiene_missing_entries() {
  let gitignore = r"
target/
";
  let report = check_gitignore_hygiene_content(
    Some(gitignore),
    true, // has_python
    true, // has_rust
    true, // has_js
  );
  assert!(report.gitignore_exists);
  assert_eq!(report.issues.len(), 2);
  let py_issue = report
    .issues
    .iter()
    .find(|i| i.category == "Python")
    .unwrap();
  assert_eq!(
    py_issue.missing_patterns,
    vec![".ruff_cache", "__pycache__", ".pytest_cache"]
  );
  let js_issue = report
    .issues
    .iter()
    .find(|i| i.category == "JavaScript / Node")
    .unwrap();
  assert_eq!(js_issue.missing_patterns, vec!["node_modules"]);
}

#[test]
fn test_check_gitignore_hygiene_no_file() {
  let report = check_gitignore_hygiene_content(
    None, true,  // has_python
    true,  // has_rust
    false, // has_js
  );
  assert!(!report.gitignore_exists);
  assert_eq!(report.issues.len(), 2);
  assert!(report.issues.iter().any(|i| i.category == "Python"));
  assert!(report.issues.iter().any(|i| i.category == "Rust"));
}
