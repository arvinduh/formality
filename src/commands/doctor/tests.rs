use super::*;
use crate::ui::table;
use std::path;

#[test]
fn test_detect_virtualenv_from_env_var() {
  let temp = tempfile::tempdir().unwrap();
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
    let temp = tempfile::tempdir().unwrap();
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
  let temp = tempfile::tempdir().unwrap();
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
  let temp = tempfile::tempdir().unwrap();
  let info = detect_virtualenv_with_env(temp.path(), None);
  assert!(!info.is_active);
  assert_eq!(info.venv_path, None);
  assert_eq!(info.source, VirtualEnvSource::None);
}

#[test]
fn test_find_venv_interpreter() {
  let temp = tempfile::tempdir().unwrap();
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

/// Regression test for #192 [pre-recreation]: `clippy_probe_succeeds` (what `lookup_tool_info`
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
  // 1) — this is the shim-present-but-component-missing case from #192 [pre-recreation], and
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

/// Regression test for #192 [pre-recreation]: `lookup_tool_info` must dispatch to the
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

#[test]
fn test_stale_unpinnable_explanation() {
  let expl = stale_unpinnable_explanation(
    "prettier",
    &version::Version::new(3, 8, 1),
    &version::Version::new(3, 9, 6),
  );
  assert!(expl.contains("prettier is stale (v3.8.1 != pinned v3.9.6)"));
  assert!(expl.contains("can't pin to v3.9.6"));
  assert!(expl.contains("or accept this drift"));
}

#[test]
fn test_pinned_version_for_golangci_lint() {
  assert_eq!(
    surfaces::pinned_version_for("golangci-lint"),
    Some(version::Version::new(2, 13, 2))
  );
}

#[test]
fn test_scan_tools_and_build_table_surfaces_unprobeable_status_not_ready() {
  #[derive(Clone)]
  struct UnprobeableSurface {
    bin: &'static str,
  }

  impl config::facets::DeclaresFacets for UnprobeableSurface {
    fn facet_support(
      &self,
      _facet: config::facets::Facet,
    ) -> config::facets::FacetSupport {
      config::facets::FacetSupport::Unsupported
    }
  }

  impl LanguageSurface for UnprobeableSurface {
    fn name(&self) -> &'static str {
      "mock_unprobeable"
    }
    fn detect(
      &self,
      _: &path::Path,
      _: &surfaces::glob::PresentExtensions,
    ) -> bool {
      true
    }
    fn tool_info(
      &self,
      _resolved: &config::ResolvedLangConfig,
    ) -> Vec<surfaces::ToolInfo> {
      vec![surfaces::ToolInfo {
        binary: self.bin,
        description: "Mock Unprobeable Binary",
        install_hint: Some("Cannot install"),
        is_required_for_fmt: true,
        is_required_for_lint: true,
      }]
    }
    fn format(
      &self,
      _ctx: &surfaces::ExecutionContext,
    ) -> surfaces::SurfaceResult {
      unimplemented!()
    }
    fn lint(
      &self,
      _ctx: &surfaces::ExecutionContext,
      _fix: bool,
    ) -> surfaces::SurfaceResult {
      unimplemented!()
    }
    fn sync_config(
      &self,
      _ctx: &surfaces::ExecutionContext,
      _check: bool,
    ) -> surfaces::SurfaceResult {
      unimplemented!()
    }
    fn clone_box(&self) -> Box<dyn LanguageSurface> {
      Box::new(self.clone())
    }
  }

  // Use a system binary that exists on PATH but does not produce semver on `--version`
  let binary_name: &'static str = if cfg!(windows) { "where" } else { "test" };
  if which::which(binary_name).is_err() {
    return;
  }

  let surfaces: Vec<Box<dyn LanguageSurface>> =
    vec![Box::new(UnprobeableSurface { bin: binary_name })];
  let config = config::FormalityConfig::default();

  let present = std::cell::LazyCell::new(|| {
    surfaces::glob::PresentExtensions::from_paths(&[])
  });
  let scan = scan_tools_and_build_table(
    path::Path::new("."),
    &surfaces,
    &collections::HashSet::new(),
    &present,
    &config,
  );

  assert!(scan.missing.is_empty());
  assert!(scan.installed.contains(binary_name));
  assert!(!scan.outdated.contains(binary_name));
  assert!(scan.stale.is_empty());
  assert!(scan.unknown.contains(binary_name));

  let rendered = table::render(&scan.table, &table::Palette::none());
  assert!(
    rendered.contains("[UNKNOWN]"),
    "Expected [UNKNOWN] badge in table for unprobeable binary, got:\n{rendered}"
  );
  assert!(
    !rendered.contains("[READY]"),
    "Unprobeable binary must NOT render [READY] in table, got:\n{rendered}"
  );
}

#[test]
fn test_format_stale_tool_warning() {
  let warning = format_stale_tool_warning(
    "prettier",
    &version::Version::new(3, 8, 1),
    &version::Version::new(3, 9, 6),
  );
  assert_eq!(
    warning,
    "tool 'prettier' is stale (v3.8.1 != pinned v3.9.6); run 'fml doctor --install' to update"
  );
}

#[test]
fn test_install_missing_tools_framed_fails_for_tool_without_installer() {
  let missing_tool = surfaces::ToolInfo {
    binary: "__missing_dummy_binary_test__",
    description: "Dummy Missing Tool Test",
    install_hint: Some("Run npm install -g dummy"),
    is_required_for_fmt: true,
    is_required_for_lint: true,
  };

  let report =
    install_missing_tools_framed(&[missing_tool], table::Frame::capped());
  assert!(
    !report.all_ok,
    "Should report failure when tool cannot be auto-installed"
  );
}

#[test]
fn test_preflight_warn_stale_tools_empty_surfaces() {
  let config = config::FormalityConfig::default();
  // Must execute cleanly without panicking
  preflight_warn_stale_tools(&[], &config, true, true);
  preflight_warn_stale_tools(&[], &config, true, false);
  preflight_warn_stale_tools(&[], &config, false, true);
}

#[test]
fn test_preflight_warn_stale_tools_with_surfaces() {
  let config = config::FormalityConfig::default();
  let surfaces = surfaces::all_surfaces();
  // Runs against real/registered surfaces cleanly without panicking
  preflight_warn_stale_tools(&surfaces, &config, true, false);
  preflight_warn_stale_tools(&surfaces, &config, false, true);
  preflight_warn_stale_tools(&surfaces, &config, true, true);
}

/// Builds a [`ToolTally`] in the shape a pre-install scan would leave it —
/// the starting point every #106 reconciliation test works from.
fn pre_install_tally(
  installed: &[&'static str],
  stale: &[&'static str],
  missing: &[&'static str],
  unknown: usize,
) -> ToolTally {
  ToolTally {
    installed: installed.iter().copied().collect(),
    outdated: 0,
    stale: stale.iter().copied().collect(),
    unknown,
    missing: missing.iter().copied().collect(),
  }
}

/// One Install Summary row, as [`install_missing_tools_framed`] would have
/// pushed it. Only `binary` and `outcome` matter to the tally.
fn install_row(
  binary: &'static str,
  outcome: InstallOutcome,
) -> InstallSummaryRow {
  InstallSummaryRow {
    binary,
    installer: "npm".to_string(),
    outcome,
    detail: String::new(),
  }
}

/// Throwaway [`ToolInfo`] for the tally tests — only `binary` is read.
fn doctor_tool_info(binary: &'static str) -> surfaces::ToolInfo {
  surfaces::ToolInfo {
    binary,
    description: "",
    install_hint: None,
    is_required_for_fmt: true,
    is_required_for_lint: false,
  }
}

/// #106's exact repro: 8 tools, one of them (`prettier`) genuinely missing,
/// installed successfully. The footer must read `8 installed (1 unknown), 0
/// missing` — agreeing with the Install Summary table printed directly above
/// it — not replay the pre-install scan's `7 installed (1 unknown), 1
/// missing`.
///
/// This tests the *ordering*, not just the arithmetic: it asserts on the same
/// rendered footer string the old code produced, and pins that the install
/// run is folded in before that string is built. The pre-install assertion
/// below is the old, contradictory output, kept verbatim so a regression back
/// to snapshot-rendering fails here loudly instead of silently printing a
/// plausible-looking number.
#[test]
fn test_tool_tally_footer_reflects_post_install_state() {
  let mut tally = pre_install_tally(
    &[
      "rustfmt",
      "clippy-driver",
      "ruff",
      "taplo",
      "yamlfmt",
      "gofmt",
      "markdownlint-cli2",
    ],
    &[],
    &["prettier"],
    1,
  );

  // What the pre-install snapshot says — the bug, verbatim.
  let before = table::strip_ansi_escapes(&tally.render(false));
  assert_eq!(before.trim(), "7 installed (1 unknown), 1 missing");

  tally.apply_install_run(&InstallRunReport {
    all_ok: true,
    rows: vec![install_row("prettier", InstallOutcome::Ok)],
  });

  let after = table::strip_ansi_escapes(&tally.render(false));
  assert_eq!(
    after.trim(),
    "8 installed (1 unknown), 0 missing",
    "the footer must count a tool the Install Summary marked [OK] as \
     installed, not missing"
  );
}

/// #106 was fundamentally an *ordering* bug — the footer was rendered from a
/// binding taken before `--install` ran, so it could not reflect anything the
/// install changed. The semantic tests around this one pin what
/// reconciliation does; this one pins that `run_doctor` actually performs it
/// before rendering, which is the half that regressed.
///
/// Tier-2 source scan, the mechanism `docs/style-guide.md` §2/§3 establish
/// for a rule a type signature can't carry. It fails against the old
/// ordering in both directions: the pre-fix `run_doctor` has no
/// `apply_install_run` call at all (the `expect` below fires), and a
/// re-introduced stale read trips the `scan.`-free assertion.
#[test]
fn test_run_doctor_folds_install_into_tally_before_rendering_footer() {
  let source = include_str!("mod.rs");
  let start = source
    .find("pub fn run_doctor(")
    .expect("run_doctor must exist");

  // Blank out `//` comment lines before scanning. Without this the check is
  // vacuous: `run_doctor`'s own prose names the very call this looks for, so
  // a commented-out call would satisfy the scan as well as a real one --
  // confirmed by commenting the real call out and watching an earlier draft
  // of this test still pass.
  let stripped: String = source[start..]
    .lines()
    .map(|line| {
      if line.trim_start().starts_with("//") {
        ""
      } else {
        line
      }
    })
    .collect::<Vec<_>>()
    .join("\n");

  // Bound the scan to `run_doctor`'s own body -- the first column-0 `}`,
  // which for rustfmt-formatted source is the function's closing brace.
  // Scanning to EOF instead would let an `apply`/`render` pair anywhere later
  // in the file (a helper, a future function) satisfy this test while
  // `run_doctor` itself had regressed, and would drag unrelated `scan.` /
  // `from_scan` text into the "between" window.
  let end = stripped
    .find("\n}\n")
    .map_or(stripped.len(), |offset| offset + 2);
  let body = &stripped[..end];
  assert!(
    body.contains("ToolTally::from_scan("),
    "the scanned window must actually be run_doctor's body -- it no longer \
     contains the tally construction, so this guard is measuring the wrong \
     code"
  );

  let apply = body
    .find("tally.apply_install_run(")
    .expect("run_doctor must fold the install run into the tally (#106)");
  let render = body
    .find("tally.render(")
    .expect("run_doctor must render the footer from the tally (#106)");

  assert!(
    apply < render,
    "the install run must be folded into the tally before the footer is \
     rendered, or the footer replays the pre-install scan (#106)"
  );

  // ...and the footer must not be built from the pre-install snapshot at
  // all: no scan bucket may be read between the install and the render.
  let between = &body[apply..render];
  assert!(
    !between.contains("scan."),
    "the footer must be rendered from the reconciled tally, not the \
     pre-install scan (#106); found a `scan.` read at: {between}"
  );

  // The `scan.` check alone has a hole big enough to reinstate the bug
  // verbatim: `let tally = ToolTally::from_scan(&scan);` re-bound here would
  // throw away everything `apply_install_run` just folded in, yet the
  // substring `&scan)` *contains* `scan.`-free text and, worse, the
  // `from_scan` call reads the snapshot without ever writing the literal
  // `scan.` -- so the assertion above passes. Confirmed by inserting exactly
  // that line and watching the guard stay green. Name the constructor
  // explicitly (#213: a guard that greps for a leak must also grep for every
  // wrapper that performs the same leak).
  assert!(
    !between.contains("from_scan"),
    "the tally must not be rebuilt from the pre-install scan between the \
     install run and the render -- that discards the reconciliation and \
     restores #106 exactly; found a `from_scan` call at: {between}"
  );
}

/// The CI-runner shape from #106: three missing tools, all three install.
#[test]
fn test_tool_tally_footer_after_multiple_successful_installs() {
  let mut tally = pre_install_tally(
    &["rustfmt", "clippy-driver", "ruff", "yamlfmt", "gofmt"],
    &[],
    &["prettier", "markdownlint-cli2", "taplo"],
    0,
  );

  tally.apply_install_run(&InstallRunReport {
    all_ok: true,
    rows: vec![
      install_row("prettier", InstallOutcome::Ok),
      install_row("markdownlint-cli2", InstallOutcome::Ok),
      install_row("taplo", InstallOutcome::Ok),
    ],
  });

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "8 installed, 0 missing"
  );
}

/// A tool that could not be installed stays counted as missing — the tally
/// follows each per-tool outcome, it does not blanket-assume every attempted
/// tool succeeded.
#[test]
fn test_tool_tally_failed_install_stays_missing() {
  let mut tally =
    pre_install_tally(&["rustfmt"], &[], &["prettier", "taplo"], 0);

  tally.apply_install_run(&InstallRunReport {
    all_ok: false,
    rows: vec![
      install_row("prettier", InstallOutcome::Ok),
      install_row("taplo", InstallOutcome::Fail),
    ],
  });

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "2 installed, 1 missing"
  );
  assert!(tally.missing.contains("taplo"));
}

/// Same for a tool with no installer chain at all: `[MISS]` in the Install
/// Summary means still missing in the tally.
#[test]
fn test_tool_tally_uninstallable_tool_stays_missing() {
  let mut tally = pre_install_tally(&["rustfmt"], &[], &["clang-format"], 0);

  tally.apply_install_run(&InstallRunReport {
    all_ok: false,
    rows: vec![install_row("clang-format", InstallOutcome::NoInstaller)],
  });

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "1 installed, 1 missing"
  );
}

/// A `[STALE]` tool that reinstalls cleanly stops being counted as stale
/// without inflating the installed count — it was already on `PATH`, so it
/// was already counted as installed.
#[test]
fn test_tool_tally_reinstalled_stale_tool_drops_from_stale() {
  let mut tally = pre_install_tally(&["rustfmt", "taplo"], &["taplo"], &[], 0);
  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "2 installed (1 stale), 0 missing"
  );

  tally.apply_install_run(&InstallRunReport {
    all_ok: true,
    rows: vec![install_row("taplo", InstallOutcome::Ok)],
  });

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "2 installed, 0 missing"
  );
}

/// The convergence guard's `[WARN]` case: the binary landed on `PATH` but
/// reports a version that doesn't match the pin. It stops being missing (it
/// is genuinely there now) and is counted stale (present at the wrong
/// version), rather than being passed off as a clean install.
#[test]
fn test_tool_tally_version_mismatched_install_counts_as_stale() {
  let mut tally = pre_install_tally(&["rustfmt"], &[], &["typstyle"], 0);

  tally.apply_install_run(&InstallRunReport {
    all_ok: false,
    rows: vec![install_row("typstyle", InstallOutcome::Warn)],
  });

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "2 installed (1 stale), 0 missing"
  );
}

/// The case this whole gate exists for: `npm i -g prettier` exits 0, npm's
/// global bin directory is not on this process's `PATH`, and
/// `refresh_path_after_install` is a deliberate no-op for npm. The exit code
/// alone would say `[OK]`; the tool is nonetheless uninvokable, and the very
/// next `fml fmt` would report it missing. Unpinned (`expected: None`) is the
/// common path — per `surfaces::tooling::ToolChain`, most tools opt out of
/// the pin — and is exactly where the old code had no post-install
/// verification at all.
#[test]
fn test_classify_install_outcome_unpinned_absent_binary_is_not_ok() {
  assert_eq!(
    classify_install_outcome(false, None, None),
    InstallOutcome::NotOnPath,
    "an installer exiting 0 without putting the binary on PATH must not be \
     reported as installed (#106)"
  );
}

/// A tool with no install step that can run on the OS (`gofmt` anywhere, it
/// has no chain; `checkstyle` on Windows, its chain is `brew` + `apt`) must
/// not read like a chain whose package managers are all missing.
#[test]
fn test_miss_headline_distinguishes_no_install_path_from_no_installer() {
  let headline =
    |binary, os| table::strip_ansi_escapes(&miss_headline(binary, os));
  let no_installer =
    |binary| format!("No automatic package manager found for {binary}.");
  let no_path =
    |binary| format!("fml has no install path for {binary} on this OS.");
  for os in ["linux", "macos", "windows"] {
    assert_eq!(headline("gofmt", os), no_path("gofmt"), "{os}");
    assert_eq!(headline("prettier", os), no_installer("prettier"), "{os}");
  }
  assert_eq!(headline("checkstyle", "linux"), no_installer("checkstyle"));
  assert_eq!(headline("checkstyle", "macos"), no_installer("checkstyle"));
  assert_eq!(headline("checkstyle", "windows"), no_path("checkstyle"));
}

/// `PATH` resolution is checked first and unconditionally: a tool that cannot
/// be invoked is not "present at the wrong version", it is absent. Even a
/// probed version matching the pin exactly cannot promote it — that
/// combination is unreachable in production, and pinning it here is what
/// stops a future reordering of the two checks from turning an absent tool
/// into an `[OK]` row.
#[test]
fn test_classify_install_outcome_path_check_precedes_version_check() {
  let pinned = version::Version::new(1, 2, 3);
  assert_eq!(
    classify_install_outcome(false, Some(&pinned), Some(&pinned)),
    InstallOutcome::NotOnPath
  );
  assert_eq!(
    classify_install_outcome(false, Some(&pinned), None),
    InstallOutcome::NotOnPath
  );
}

/// The ordinary success path: on `PATH`, no pin to satisfy.
#[test]
fn test_classify_install_outcome_unpinned_present_binary_is_ok() {
  assert_eq!(
    classify_install_outcome(true, None, None),
    InstallOutcome::Ok
  );
}

/// A pinned tool that lands on `PATH` reporting the pinned version is a clean
/// install; one reporting anything else (or nothing parseable) trips the
/// convergence guard.
#[test]
fn test_classify_install_outcome_pinned_compares_versions_when_present() {
  let pinned = version::Version::new(0, 9, 0);
  let other = version::Version::new(0, 8, 0);

  assert_eq!(
    classify_install_outcome(true, Some(&pinned), Some(&pinned)),
    InstallOutcome::Ok
  );
  assert_eq!(
    classify_install_outcome(true, Some(&pinned), Some(&other)),
    InstallOutcome::Warn
  );
  assert_eq!(
    classify_install_outcome(true, Some(&pinned), None),
    InstallOutcome::Warn
  );
}

/// The tally half of the same case: an `[FAIL]`/`NotOnPath` row must leave the
/// tool counted as missing, so the closing footer agrees with the Install
/// Summary table instead of claiming `8 installed, 0 missing` for a tool the
/// next command cannot run.
#[test]
fn test_tool_tally_not_on_path_install_stays_missing() {
  let mut tally = pre_install_tally(&["rustfmt"], &[], &["prettier"], 0);

  tally.apply_install_run(&InstallRunReport {
    all_ok: false,
    rows: vec![install_row("prettier", InstallOutcome::NotOnPath)],
  });

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "1 installed, 1 missing",
    "a tool whose installer exited 0 without putting it on PATH must stay \
     counted as missing (#106)"
  );
  assert!(tally.missing.contains("prettier"));
  assert!(!tally.installed.contains("prettier"));
}

/// `tool_is_on_path` is not a second, looser notion of "present" living
/// alongside the scan's — it *is* the scan's. If these ever disagree the
/// doctor table and the Install Summary can contradict each other again,
/// which is the class of bug #106 is.
#[test]
fn test_tool_is_on_path_matches_the_scan_predicate() {
  // A name no package manager will ever have installed.
  const ABSENT: &str = "fml-nonexistent-binary-for-issue-106";
  assert!(!tool_is_on_path(ABSENT));
  assert!(!lookup_tool_info(ABSENT).is_installed);

  // `cargo` is running this very test, so it is unambiguously on `PATH`.
  assert!(tool_is_on_path("cargo"));
  assert!(lookup_tool_info("cargo").is_installed);
}

/// Tier-2 source scan, same mechanism as the ordering guard above. The gate
/// has to live *at the install site*, before any `InstallSummaryRow` is
/// pushed, so the Install Summary table and the closing tally are a single
/// honest source of truth rather than two places that could classify the same
/// install differently. Doing it downstream in `apply_install_run` would
/// leave the printed table claiming `[OK]` for a tool the footer counted
/// missing.
#[test]
fn test_install_site_resolves_path_before_classifying_the_outcome() {
  let source = include_str!("mod.rs");
  let start = source
    .find("fn install_missing_tools_framed(")
    .expect("install_missing_tools_framed must exist");

  // Comment lines are blanked for the same reason as the ordering guard: the
  // install site's own prose names both calls this test looks for, so an
  // unstripped scan would pass on the comments alone.
  let stripped: String = source[start..]
    .lines()
    .map(|line| {
      if line.trim_start().starts_with("//") {
        ""
      } else {
        line
      }
    })
    .collect::<Vec<_>>()
    .join("\n");
  let end = stripped
    .find("\n}\n")
    .map_or(stripped.len(), |offset| offset + 2);
  let body = &stripped[..end];

  let probe = body.find("tool_is_on_path(").expect(
    "the install site must resolve the binary on PATH before deciding what \
     the install accomplished -- an exit code of 0 is not evidence the tool \
     is usable (#106)",
  );
  let classify = body.find("classify_install_outcome(").expect(
    "the install site must classify the outcome through the shared \
             predicate (#106)",
  );
  assert!(
    probe < classify,
    "the PATH resolution must happen before the outcome is classified"
  );

  let pushes = body
    .find("summary_rows.push(")
    .expect("the install site must record a summary row per tool");
  assert!(
    classify < pushes,
    "no Install Summary row may be pushed for a successful install command \
     before the outcome has been classified -- that is how an unverified \
     `[OK]` gets into the table and the tally (#106)"
  );
}

/// Applying the same run twice must not double-count — the buckets are name
/// sets precisely so reconciliation is idempotent.
#[test]
fn test_tool_tally_apply_install_run_is_idempotent() {
  let mut tally = pre_install_tally(&["rustfmt"], &[], &["prettier"], 0);
  let report = InstallRunReport {
    all_ok: true,
    rows: vec![install_row("prettier", InstallOutcome::Ok)],
  };

  tally.apply_install_run(&report);
  let once = table::strip_ansi_escapes(&tally.render(false));
  tally.apply_install_run(&report);

  assert_eq!(table::strip_ansi_escapes(&tally.render(false)), once);
}

/// An empty install run (nothing left to install) leaves the tally untouched,
/// so a second `fml doctor --install` still reports the scan's own numbers.
#[test]
fn test_tool_tally_empty_install_run_leaves_tally_untouched() {
  let mut tally = pre_install_tally(&["rustfmt", "ruff"], &[], &[], 1);
  let before = table::strip_ansi_escapes(&tally.render(false));

  tally.apply_install_run(&InstallRunReport {
    all_ok: true,
    rows: Vec::new(),
  });

  assert_eq!(table::strip_ansi_escapes(&tally.render(false)), before);
}

/// `ToolTally::from_scan` carries every scan bucket across.
#[test]
fn test_tool_tally_from_scan_carries_every_bucket() {
  let scan = DoctorScanResult {
    table: table::Table::new(vec![table::Column::new(table::Cell::text(""))]),
    missing: vec![doctor_tool_info("prettier")],
    installed: collections::HashSet::from(["rustfmt", "taplo"]),
    outdated: collections::HashSet::from(["rustfmt"]),
    stale: vec![doctor_tool_info("taplo")],
    unknown: collections::HashSet::from(["taplo"]),
  };

  let tally = ToolTally::from_scan(&scan);

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(false)).trim(),
    "2 installed (1 outdated) (1 stale) (1 unknown), 1 missing"
  );
}

/// The install hint only renders when asked for.
#[test]
fn test_tool_tally_render_install_hint() {
  let tally = pre_install_tally(&["rustfmt"], &[], &["prettier"], 0);

  assert_eq!(
    table::strip_ansi_escapes(&tally.render(true)).trim(),
    "1 installed, 1 missing\n  (run 'fml doctor --install' to install missing/stale tools)"
  );
}

#[test]
fn test_doctor_table_shows_detected_vs_undetected_status_for_surfaces() {
  let temp = tempfile::tempdir().unwrap();
  let cargo_toml = temp.path().join("Cargo.toml");
  std::fs::write(
    &cargo_toml,
    "[package]\nname = \"doc_test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
  )
  .unwrap();
  let src_dir = temp.path().join("src");
  std::fs::create_dir_all(&src_dir).unwrap();
  std::fs::write(src_dir.join("main.rs"), "fn main() {}\n").unwrap();

  let config = config::FormalityConfig::default();
  let surfaces = surfaces::all_surfaces();
  let present = std::cell::LazyCell::new(|| {
    surfaces::glob::PresentExtensions::scan(temp.path(), &[])
  });
  let detected: collections::HashSet<&'static str> =
    surfaces::default_registry()
      .detect_surfaces_in(temp.path(), &config, &present)
      .iter()
      .map(|s| s.name())
      .collect();
  let scan = scan_tools_and_build_table(
    temp.path(),
    &surfaces,
    &detected,
    &present,
    &config,
  );

  let rendered = table::strip_ansi_escapes(&table::render(
    &scan.table,
    &table::Palette::none(),
  ));

  // In this workspace, rust is detected; other languages (e.g. python, go) are not.
  assert!(
    rendered.contains("rust"),
    "table should contain rust surface, got:\n{rendered}"
  );
  assert!(
    rendered.contains("detected"),
    "table should contain 'detected' status, got:\n{rendered}"
  );
  assert!(
    rendered.contains("undetected"),
    "table should contain 'undetected' status, got:\n{rendered}"
  );

  // Filter for primary row lines (which start with '[') so wrapped cell continuations are ignored
  let row_lines: Vec<&str> = rendered
    .lines()
    .filter(|l| l.trim_start().starts_with('['))
    .collect();

  let rust_rows: Vec<&str> = row_lines
    .iter()
    .copied()
    .filter(|line| line.contains("rust"))
    .collect();
  assert!(!rust_rows.is_empty(), "expected at least one rust row");
  for line in &rust_rows {
    assert!(
      line.contains("detected") && !line.contains("undetected"),
      "rust row should be marked detected, got: {line}"
    );
  }

  let undetected_rows: Vec<&str> = row_lines
    .iter()
    .copied()
    .filter(|line| line.contains("python"))
    .collect();
  assert!(
    !undetected_rows.is_empty(),
    "expected at least one undetected row"
  );
  for line in &undetected_rows {
    assert!(
      line.contains("undetected"),
      "undetected surface line should be marked undetected, got: {line}"
    );
  }
}

#[test]
fn test_find_system_python() {
  let found = find_system_python();
  let expected = which::which("python3")
    .or_else(|_| which::which("python"))
    .ok();
  assert_eq!(found, expected);
}

#[test]
fn test_format_version_details_distro_suffix_renders_both_forms() {
  // Compatible: both normalized version and raw banner version rendered
  let ready_info = format_version_details(
    &version::Version::new(14, 0, 0),
    None,
    Some("clang-tidy version 14.0.0-1ubuntu1"),
  );
  assert_eq!(ready_info, " (v14.0.0 — reported 14.0.0-1ubuntu1)");

  // Outdated branch: both normalized comparison and reported version rendered
  let outdated_info = format_version_details(
    &version::Version::new(13, 0, 0),
    Some("< MSTV v14.0.0"),
    Some("clang-tidy version 13.0.0-1ubuntu1"),
  );
  assert_eq!(
    outdated_info,
    " (v13.0.0 < MSTV v14.0.0 — reported 13.0.0-1ubuntu1)"
  );

  // Stale branch: both normalized comparison and reported version rendered
  let stale_info = format_version_details(
    &version::Version::new(13, 0, 0),
    Some("!= pinned v14.0.0"),
    Some("clang-tidy version 13.0.0-1ubuntu1"),
  );
  assert_eq!(
    stale_info,
    " (v13.0.0 != pinned v14.0.0 — reported 13.0.0-1ubuntu1)"
  );
}

#[test]
fn test_format_version_details_identical_when_versions_match() {
  // Compatible: matches -> identical output without "reported"
  let ready_info = format_version_details(
    &version::Version::new(14, 0, 0),
    None,
    Some("clang-tidy version 14.0.0"),
  );
  assert_eq!(ready_info, " (v14.0.0)");

  // Compatible with leading 'v' in banner token
  let ready_v_info = format_version_details(
    &version::Version::new(14, 0, 0),
    None,
    Some("clang-tidy version v14.0.0"),
  );
  assert_eq!(ready_v_info, " (v14.0.0)");

  // Outdated: matches -> identical output without "reported"
  let outdated_info = format_version_details(
    &version::Version::new(13, 0, 0),
    Some("< MSTV v14.0.0"),
    Some("clang-tidy version 13.0.0"),
  );
  assert_eq!(outdated_info, " (v13.0.0 < MSTV v14.0.0)");

  // Stale: matches -> identical output without "reported"
  let stale_info = format_version_details(
    &version::Version::new(13, 0, 0),
    Some("!= pinned v14.0.0"),
    Some("clang-tidy version 13.0.0"),
  );
  assert_eq!(stale_info, " (v13.0.0 != pinned v14.0.0)");

  // None raw banner -> identical output
  let no_banner =
    format_version_details(&version::Version::new(14, 0, 0), None, None);
  assert_eq!(no_banner, " (v14.0.0)");
}

#[test]
fn test_doctor_table_layout_budget_with_reported_distro_version() {
  let mut doctor_table = table::Table::new(vec![
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(10)),
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(20)),
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(10)),
    table::Column::new(table::Cell::text("")).width(table::WidthPolicy::Auto),
  ])
  .layout(
    table::Layout::compact()
      .indent(2)
      .padding(0, 1)
      .max_width(80),
  );

  let v_info_reported = format_version_details(
    &version::Version::new(14, 0, 0),
    None,
    Some("clang-tidy version 14.0.0-1ubuntu1"),
  );
  let v_info_matched = format_version_details(
    &version::Version::new(1, 8, 0),
    None,
    Some("rustfmt 1.8.0"),
  );

  doctor_table.add_row(table::Row::new(vec![
    table::Cell::styled("[READY]", table::Style::Ok),
    table::Cell::styled("clang-tidy", table::Style::Tool),
    table::Cell::styled("cpp", table::Style::Dim),
    table::Cell::new(vec![
      table::Span::styled("/usr/bin/clang-tidy", table::Style::Dim),
      table::Span::styled(v_info_reported, table::Style::Info),
    ]),
  ]));

  doctor_table.add_row(table::Row::new(vec![
    table::Cell::styled("[READY]", table::Style::Ok),
    table::Cell::styled("rustfmt", table::Style::Tool),
    table::Cell::styled("rust", table::Style::Dim),
    table::Cell::new(vec![
      table::Span::styled("/home/user/.cargo/bin/rustfmt", table::Style::Dim),
      table::Span::styled(v_info_matched, table::Style::Info),
    ]),
  ]));

  let rendered = table::render(&doctor_table, &table::Palette::none());
  assert!(rendered.contains("[READY]"));
  assert!(rendered.contains("clang-tidy"));
  assert!(rendered.contains("v14.0.0"));
  assert!(rendered.contains("reported 14.0.0-1ubuntu1"));
  assert!(rendered.contains("v1.8.0"));
  assert!(!rendered.contains("v1.8.0 — reported"));

  // Check 80-col budget enforcement: wrapping respects the width cap
  for line in rendered.lines() {
    assert!(
      line.chars().count() <= 80,
      "Doctor table line exceeded 80-col budget ({len} chars): {line}",
      len = line.chars().count()
    );
  }
}

#[test]
fn test_run_doctor_walks_the_workspace_at_most_once() {
  let temp = tempfile::tempdir().unwrap();
  std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();

  let _ =
    run_doctor(temp.path(), false, false, &config::FormalityConfig::empty());

  // Detection and the table's detected column share one walk.
  assert_eq!(surfaces::glob::walk_count::of(temp.path()), 1);
}

#[test]
fn test_doctor_detects_the_surfaces_fmt_runs_under_global_exclude() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path();
  std::fs::write(root.join("pyproject.toml"), "[project]\n").unwrap();
  std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
  std::fs::create_dir(root.join("ci")).unwrap();
  std::fs::write(root.join("ci/build.yaml"), "a: 1\n").unwrap();
  let config = config::FormalityConfig::parse_str(
    "[global]\nexclude = [\"ci\", \"pyproject.toml\"]\n",
    path::Path::new("formality.toml"),
  )
  .unwrap();

  let _ = run_doctor(root, false, false, &config);
  let doctor = LAST_DETECTED.take();
  let scope =
    crate::engine::Scope::resolve(root, &[], &config.resolve_global().exclude);
  let fmt: Vec<&str> =
    crate::commands::resolve_target_surfaces(root, &[], &scope, &config)
      .unwrap()
      .iter()
      .map(|s| s.name())
      .collect();

  // The only `.yaml` file is excluded; the excluded root marker still
  // activates python in both.
  assert!(!doctor.contains(&"yaml"), "{doctor:?}");
  assert_eq!(doctor, fmt);
}
