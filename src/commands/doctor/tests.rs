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

#[test]
fn test_doctor_schema_version_check_stale() {
  let temp = tempdir().unwrap();
  let config_file = temp.path().join("formality.toml");
  std::fs::write(
    &config_file,
    "#:schema https://github.com/arvinduh/formality/releases/download/s0.9/formality.schema.json\n[global]\n",
  )
  .unwrap();

  let status = crate::config::schema::check_schema_version_file(&config_file);
  assert_eq!(
    status,
    crate::config::schema::SchemaStatus::Stale {
      version: crate::config::schema::SchemaVersion { major: 0, minor: 9 },
      expected: crate::config::SCHEMA_VERSION,
    }
  );
}

#[test]
fn test_command_ran_successfully_true_on_zero_exit() {
  let output = std::process::Command::new("sh")
    .args(["-c", "exit 0"])
    .output();
  assert!(command_ran_successfully(&output));
}

#[test]
fn test_command_ran_successfully_false_on_nonzero_exit() {
  let output = std::process::Command::new("sh")
    .args(["-c", "exit 1"])
    .output();
  assert!(!command_ran_successfully(&output));
}

#[test]
fn test_command_ran_successfully_false_on_spawn_error() {
  // A binary name that should never exist on PATH — spawning it fails
  // outright, which must not be mistaken for a successful run.
  let output =
    std::process::Command::new("definitely_not_a_real_binary_xyz_192").output();
  assert!(!command_ran_successfully(&output));
}

/// Regression test for #192: `clippy_probe_succeeds` (what `lookup_tool_info`
/// gates `is_installed` on for every alias clippy can be registered under —
/// `"clippy"`, `"clippy-driver"`, `"cargo-clippy"`) must require an actual
/// successful invocation, not just presence on `PATH`. Parameterized over
/// the binary names lets this substitute the real `true`/`false` binaries as
/// deterministic stand-ins for a functional vs. a present-but-broken shim,
/// with no `PATH` mutation needed.
#[test]
fn test_clippy_probe_succeeds_requires_functional_driver() {
  // Both the driver and the cargo fallback are broken (`false` always exits
  // 1) — this is the shim-present-but-component-missing case from #192, and
  // must be reported as NOT installed.
  assert!(!clippy_probe_succeeds("false", "false"));

  // Driver alone works.
  assert!(clippy_probe_succeeds("true", "false"));

  // Driver is broken but the `cargo clippy` fallback works.
  assert!(clippy_probe_succeeds("false", "true"));

  // Neither binary exists at all (not even a broken shim on PATH).
  assert!(!clippy_probe_succeeds(
    "definitely_not_a_real_binary_xyz_192_driver",
    "definitely_not_a_real_binary_xyz_192_cargo"
  ));
}

/// Regression test for #192: `lookup_tool_info` must dispatch to the
/// functional clippy probe for the actual binary name production code
/// passes it (`src/surfaces/rust.rs` registers the tool as
/// `"clippy-driver"`, never the bare string `"clippy"`) — a guard that only
/// matched `"clippy"` would silently fall through to the unfixed bare
/// `which` check on the real call path.
#[test]
fn test_lookup_tool_info_dispatches_clippy_probe_for_all_aliases() {
  for alias in ["clippy", "clippy-driver", "cargo-clippy"] {
    let result = lookup_tool_info(alias);
    assert_eq!(
      result.is_installed,
      clippy_probe_succeeds("clippy-driver", "cargo"),
      "lookup_tool_info({alias:?}) did not use the functional clippy probe"
    );
  }
}

#[test]
fn test_lookup_tool_info_clippy_live_probe() {
  // Live-probe style (see `test_live_probe_rustfmt` in
  // `engine/version/tests.rs`): only assert when clippy is actually usable
  // in this environment, but when it is, `lookup_tool_info` must agree with
  // a direct functional check rather than a bare `which` presence check.
  let clippy_functional = clippy_probe_succeeds("clippy-driver", "cargo");
  let result = lookup_tool_info("clippy-driver");
  assert_eq!(result.is_installed, clippy_functional);
}

/// Regression coverage for #5: `fml install`'s "already satisfied, skip"
/// decision (`preflight_install`, and `fml doctor`'s auto-install path) must
/// treat a `[STALE]` tool the same as a genuinely `[MISS]`ing one, and must
/// leave a matching `[READY]` tool alone. `needs_install` is the pure
/// decision function both paths route through -- tested directly here so
/// the reinstall trigger doesn't depend on a real stale/pinned binary
/// existing on the test machine's `PATH`.
#[test]
fn test_needs_install_true_for_missing_tool() {
  assert!(needs_install(false, None));
  assert!(needs_install(false, Some(&ToolStatus::NotFound)));
}

#[test]
fn test_needs_install_true_for_stale_tool() {
  let stale = ToolStatus::Stale {
    current: Version::new(3, 8, 1),
    pinned: Version::new(3, 9, 6),
  };
  assert!(needs_install(true, Some(&stale)));
}

#[test]
fn test_needs_install_false_for_version_matched_ready_tool() {
  let compatible = ToolStatus::Compatible {
    current: Version::new(3, 9, 6),
    minimum: Version::new(2, 0, 0),
  };
  assert!(!needs_install(true, Some(&compatible)));
  // Present with no MSTV/pin registered at all (status: None) -- still
  // just READY, never reinstalled.
  assert!(!needs_install(true, None));
}

#[test]
fn test_needs_install_false_for_outdated_tool() {
  // Below the MSTV floor is a real problem `fml doctor` already surfaces as
  // `[WARN]`, but it is not what `fml install`'s missing/stale reinstall
  // path is for -- unaffected by this change, same as before.
  let outdated = ToolStatus::Outdated {
    current: Version::new(1, 0, 0),
    minimum: Version::new(1, 4, 0),
  };
  assert!(!needs_install(true, Some(&outdated)));
}

#[test]
fn test_doctor_schema_version_check_uptodate() {
  let temp = tempdir().unwrap();
  let config_file = temp.path().join("formality.toml");
  std::fs::write(
    &config_file,
    format!(
      "#:schema https://github.com/arvinduh/formality/releases/download/s{}/formality.schema.json\n[global]\n",
      crate::config::SCHEMA_VERSION
    ),
  )
  .unwrap();

  let status = crate::config::schema::check_schema_version_file(&config_file);
  assert_eq!(
    status,
    crate::config::schema::SchemaStatus::UpToDate {
      version: crate::config::SCHEMA_VERSION
    }
  );
}

#[test]
fn test_pinned_version_for_golangci_lint() {
  assert_eq!(
    crate::surfaces::pinned_version_for("golangci-lint"),
    Some(Version::new(2, 13, 1))
  );
}
