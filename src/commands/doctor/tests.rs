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
  #[cfg(unix)]
  let output = std::process::Command::new("sh")
    .args(["-c", "exit 0"])
    .output();
  #[cfg(windows)]
  let output = std::process::Command::new("cmd")
    .args(["/C", "exit 0"])
    .output();
  assert!(command_ran_successfully(&output));
}

#[test]
fn test_command_ran_successfully_false_on_nonzero_exit() {
  #[cfg(unix)]
  let output = std::process::Command::new("sh")
    .args(["-c", "exit 1"])
    .output();
  #[cfg(windows)]
  let output = std::process::Command::new("cmd")
    .args(["/C", "exit 1"])
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
#[cfg(unix)]
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

/// Regression coverage for #5 and #11: `fml install`'s "already satisfied, skip"
/// decision (`preflight_install`, and `fml doctor`'s auto-install path) must
/// treat a `[STALE]` tool as needing reinstall *only* if the selected installer
/// carries a matching inline pin. A stale tool with an unpinned selected installer
/// (or mismatched pin) must NOT schedule a futile reinstall (#11).
#[test]
fn test_needs_install_true_for_missing_tool() {
  assert!(needs_install(false, None, None));
  assert!(needs_install(false, Some(&ToolStatus::NotFound), None));
  assert!(needs_install(
    false,
    Some(&ToolStatus::NotFound),
    Some(&Version::new(3, 9, 6))
  ));
}

#[test]
fn test_needs_install_true_for_stale_tool_with_matching_pin() {
  let stale = ToolStatus::Stale {
    current: Version::new(3, 8, 1),
    pinned: Version::new(3, 9, 6),
  };
  let matching_pin = Version::new(3, 9, 6);
  assert!(needs_install(true, Some(&stale), Some(&matching_pin)));
}

#[test]
fn test_needs_install_false_for_stale_tool_with_unpinned_selected_installer() {
  // Fixes #11: When the selected installer has no inline pin (e.g. `brew`),
  // running `fml install` cannot produce the pinned version — reinstall is skipped.
  let stale = ToolStatus::Stale {
    current: Version::new(3, 8, 1),
    pinned: Version::new(3, 9, 6),
  };
  assert!(!needs_install(true, Some(&stale), None));
}

#[test]
fn test_needs_install_false_for_stale_tool_with_mismatched_selected_installer_pin()
 {
  // If the available installer pins a different version than expected, reinstall is skipped.
  let stale = ToolStatus::Stale {
    current: Version::new(3, 8, 1),
    pinned: Version::new(3, 9, 6),
  };
  let different_pin = Version::new(3, 8, 0);
  assert!(!needs_install(true, Some(&stale), Some(&different_pin)));
}

#[test]
fn test_needs_install_false_for_version_matched_ready_tool() {
  let compatible = ToolStatus::Compatible {
    current: Version::new(3, 9, 6),
    minimum: Version::new(2, 0, 0),
  };
  let pin = Version::new(3, 9, 6);
  assert!(!needs_install(true, Some(&compatible), Some(&pin)));
  // Present with no MSTV/pin registered at all (status: None) -- still
  // just READY, never reinstalled.
  assert!(!needs_install(true, None, None));
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
  let pin = Version::new(1, 4, 0);
  assert!(!needs_install(true, Some(&outdated), Some(&pin)));
}

#[test]
fn test_stale_unpinnable_explanation() {
  let expl = stale_unpinnable_explanation(
    "prettier",
    &Version::new(3, 8, 1),
    &Version::new(3, 9, 6),
  );
  assert!(expl.contains("prettier is stale (v3.8.1 != pinned v3.9.6)"));
  assert!(expl.contains("can't pin to v3.9.6"));
  assert!(expl.contains("or accept this drift"));
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

#[test]
fn test_needs_install_false_for_unknown_version_tool() {
  let unknown_with_raw =
    ToolStatus::UnknownVersion("nightly-build".to_string());
  let unknown_empty = ToolStatus::UnknownVersion(String::new());
  let pin = Version::new(1, 0, 0);

  assert!(!needs_install(true, Some(&unknown_with_raw), Some(&pin)));
  assert!(!needs_install(true, Some(&unknown_with_raw), None));
  assert!(!needs_install(true, Some(&unknown_empty), Some(&pin)));
  assert!(!needs_install(true, Some(&unknown_empty), None));
}

#[test]
fn test_scan_tools_and_build_table_surfaces_unprobeable_status_not_ready() {
  // Use a system binary that exists on PATH but does not produce semver on `--version`
  let binary_name: &'static str = if cfg!(windows) { "where" } else { "false" };
  if which::which(binary_name).is_err() {
    return;
  }

  #[derive(Clone)]
  struct UnprobeableSurface {
    bin: &'static str,
  }

  impl crate::config::facets::DeclaresFacets for UnprobeableSurface {
    fn facet_support(
      &self,
      _facet: crate::config::facets::Facet,
    ) -> crate::config::facets::FacetSupport {
      crate::config::facets::FacetSupport::Unsupported
    }
  }

  impl LanguageSurface for UnprobeableSurface {
    fn name(&self) -> &'static str {
      "mock_unprobeable"
    }
    fn detect(&self, _root: &Path) -> bool {
      true
    }
    fn tool_info(
      &self,
      _resolved: &crate::config::ResolvedLangConfig,
    ) -> Vec<ToolInfo> {
      vec![ToolInfo {
        binary: self.bin,
        description: "Mock Unprobeable Binary",
        install_hint: "Cannot install",
        is_required_for_fmt: true,
        is_required_for_lint: true,
      }]
    }
    fn format(
      &self,
      _ctx: &crate::surfaces::ExecutionContext,
    ) -> crate::surfaces::SurfaceResult {
      unimplemented!()
    }
    fn lint(
      &self,
      _ctx: &crate::surfaces::ExecutionContext,
      _fix: bool,
    ) -> crate::surfaces::SurfaceResult {
      unimplemented!()
    }
    fn sync_config(
      &self,
      _ctx: &crate::surfaces::ExecutionContext,
      _check: bool,
    ) -> crate::surfaces::SurfaceResult {
      unimplemented!()
    }
    fn clone_box(&self) -> Box<dyn LanguageSurface> {
      Box::new(self.clone())
    }
  }

  let surfaces: Vec<Box<dyn LanguageSurface>> =
    vec![Box::new(UnprobeableSurface { bin: binary_name })];
  let config = FormalityConfig::default();

  let scan = scan_tools_and_build_table(&surfaces, &config);

  assert!(scan.missing.is_empty());
  assert!(scan.installed.contains(binary_name));
  assert!(!scan.outdated.contains(binary_name));
  assert!(scan.stale.is_empty());
  assert!(scan.unknown.contains(binary_name));

  let rendered = render(&scan.table, &Palette::none());
  assert!(
    rendered.contains("[UNKNOWN]"),
    "Expected [UNKNOWN] badge in table for unprobeable binary, got:\n{rendered}"
  );
  assert!(
    !rendered.contains("[READY]"),
    "Unprobeable binary must NOT render [READY] in table, got:\n{rendered}"
  );
}
