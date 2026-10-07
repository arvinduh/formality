//! Toolchain checks and installs for `fml doctor` and the stale-tool preflight.
//!
//! Owns the granular operations: which tools a set of surfaces needs, whether
//! each is on `PATH` and at a supported version, and installing one tool.
//! Virtualenv detection and `.gitignore` hygiene live in submodules. Choosing
//! what to check, in what order, and what to print is the CLI's job.

/// `.gitignore` coverage of each surface's cache and build artifacts.
pub mod gitignore;
/// Python virtual environment detection.
pub mod venv;

use std::collections;
use std::io;
use std::path;

use crate::config;
use crate::engine::version;
use crate::surfaces;
use crate::surfaces::tooling;

/// One tool a surface needs, with what probing it found.
#[derive(Debug)]
pub struct Check {
  /// The first surface (in scan order) that needs the tool.
  pub surface: &'static str,
  /// The tool itself.
  pub tool: surfaces::ToolInfo,
  /// Where the binary resolved, when it did.
  pub path: Option<path::PathBuf>,
  /// Presence and version relative to the MSTV floor and the pin.
  pub status: version::ToolStatus,
}

impl Check {
  /// Returns whether `fml doctor --install` would (re)install this tool: it
  /// is missing, or stale and its selected installer can pin the version.
  #[must_use]
  pub fn needs_install(&self) -> bool {
    match &self.status {
      version::ToolStatus::NotFound => true,
      version::ToolStatus::Stale { pinned, .. } => {
        tooling::selected_pinned_version_for(self.tool.binary).as_ref()
          == Some(pinned)
      }
      _ => false,
    }
  }
}

/// Checks every tool `surfaces` need, once per binary, in surface order.
///
/// `filter` keeps a tool by its [`surfaces::ToolInfo`]; pass `|_| true` for
/// all of them.
///
/// # Side Effects
///
/// Spawns each found tool once to probe its version.
#[must_use]
pub fn check(
  surfaces: &[Box<dyn surfaces::LanguageSurface>],
  config: &config::FormalityConfig,
  filter: impl Fn(&surfaces::ToolInfo) -> bool,
) -> Vec<Check> {
  let global = config.resolve_global();
  let mut seen = collections::HashSet::new();
  let mut checks = Vec::new();
  for surface in surfaces {
    let resolved = config.resolve_for_lang_with_global(surface.name(), &global);
    for tool in surface.tool_info(&resolved) {
      if !filter(&tool) || !seen.insert(tool.binary) {
        continue;
      }
      let (path, status) = probe(tool.binary);
      checks.push(Check {
        surface: surface.name(),
        tool,
        path,
        status,
      });
    }
  }
  checks
}

/// Whether a subprocess invocation's result indicates the tool actually ran
/// and exited successfully — as opposed to merely existing on disk. Split
/// out from [`clippy_probe_succeeds`] so the success decision is
/// unit-testable without spawning any subprocess at all.
fn command_ran_successfully(result: &io::Result<std::process::Output>) -> bool {
  matches!(result, Ok(output) if output.status.success())
}

/// Whether the `clippy` component is actually installed and functional, by
/// invoking `<driver_bin> --version` and falling back to
/// `<cargo_bin> clippy --version` — a bare `which` presence check is not
/// enough, because `clippy-driver` is a rustup shim that exists on disk
/// whenever rustup is installed, regardless of whether the `clippy` component
/// itself is (#192 [pre-recreation]). Parameterized over the binary names so
/// tests can substitute a stand-in binary for a broken shim.
fn clippy_probe_succeeds(driver_bin: &str, cargo_bin: &str) -> bool {
  command_ran_successfully(
    &tooling::create_tool_command(driver_bin)
      .arg("--version")
      .output(),
  ) || command_ran_successfully(
    &tooling::create_tool_command(cargo_bin)
      .args(["clippy", "--version"])
      .output(),
  )
}

/// Returns whether `binary` is reachable from this process right now.
///
/// The single predicate for "is this tool present", shared by [`check`] and
/// [`install`]'s post-install verification so they cannot disagree (#106).
/// Clippy is registered under three names and is probed by running it.
#[must_use]
pub fn on_path(binary: &str) -> bool {
  if matches!(binary, "clippy" | "clippy-driver" | "cargo-clippy") {
    clippy_probe_succeeds("clippy-driver", "cargo")
  } else {
    tooling::check_binary_exists(binary)
  }
}

/// Probes `binary` for its resolved path and its status against the MSTV
/// floor and the pin.
fn probe(binary: &'static str) -> (Option<path::PathBuf>, version::ToolStatus) {
  if !on_path(binary) {
    return (None, version::ToolStatus::NotFound);
  }
  let path = which::which(binary)
    .or_else(|_| which::which("clippy-driver"))
    .or_else(|_| which::which("cargo"))
    .ok();
  let raw = version::get_raw_tool_version(binary);
  let status = match raw
    .as_deref()
    .and_then(|r| version::normalize_probed_version(binary, r))
  {
    Some(current) => version::evaluate_tool_status(
      Some(current),
      raw,
      version::minimum_supported_tool_version(binary).as_ref(),
      tooling::pinned_version_for(binary).as_ref(),
    ),
    None => version::ToolStatus::UnknownVersion(raw.unwrap_or_default()),
  };
  (path, status)
}

/// What one [`install`] attempt achieved.
#[derive(Debug)]
pub enum Install {
  /// Installed, and at the pinned version where a pin exists.
  Installed,
  /// Installed, but it reports a version other than its pin.
  Mismatch {
    /// The version the pin expects.
    expected: version::Version,
    /// The version the binary reports, if it could be probed.
    actual: Option<version::Version>,
  },
  /// The installer exited 0, but the binary is still not on `PATH`.
  NotOnPath {
    /// The installer program that ran.
    installer: String,
  },
  /// The installer ran and failed.
  Failed {
    /// The installer program that ran.
    installer: String,
    /// Its exit code, when it had one.
    code: Option<i32>,
  },
  /// The installer could not be spawned.
  Spawn(io::Error),
  /// No installer in the tool's chain is available here.
  NoInstaller,
}

/// Decides what an installer that exited 0 actually achieved, from whether
/// the binary now resolves and, for a pinned tool, what version it reports.
///
/// `PATH` comes first: a tool that cannot be invoked is absent, not merely at
/// the wrong version (#106).
fn classify(
  on_path: bool,
  installer: String,
  expected: Option<version::Version>,
  actual: Option<version::Version>,
) -> Install {
  if !on_path {
    return Install::NotOnPath { installer };
  }
  match expected {
    Some(expected) if actual.as_ref() != Some(&expected) => {
      Install::Mismatch { expected, actual }
    }
    _ => Install::Installed,
  }
}

/// Installs `tool` with the first available installer in its chain.
///
/// Bootstraps `cargo-binstall` first when the tool's chain prefers it and it
/// is missing, so a prebuilt binary wins over a source compile.
///
/// # Side Effects
///
/// Runs the installer with inherited stdio, evicts the tool from the binary
/// cache, and may extend this process's `PATH`.
#[must_use]
pub fn install(tool: &surfaces::ToolInfo) -> Install {
  if !tooling::has_cargo_binstall()
    && tooling::tool_would_benefit_from_cargo_binstall_bootstrap(tool.binary)
    && !tooling::ensure_cargo_binstall()
  {
    log::warn!("could not bootstrap cargo-binstall for {}", tool.binary);
  }
  let Some((program, args)) = tool.get_auto_install_cmd() else {
    return Install::NoInstaller;
  };
  log::info!(
    "installing {} via {program} {}",
    tool.binary,
    args.join(" ")
  );
  let status = match tooling::create_tool_command(&program).args(&args).status()
  {
    Ok(status) => status,
    Err(err) => return Install::Spawn(err),
  };
  if !status.success() {
    return Install::Failed {
      installer: program,
      code: status.code(),
    };
  }
  // The scan memoized the miss; a fresh lookup must see the new binary,
  // including in a directory the installer only just added to `PATH`.
  tooling::forget_binary(tool.binary);
  tooling::refresh_path_after_install(&program);
  let found = on_path(tool.binary);
  let expected = found
    .then(|| tooling::pinned_version_for(tool.binary))
    .flatten();
  let actual = expected
    .as_ref()
    .and_then(|_| version::probe_tool_version(tool.binary));
  classify(found, program, expected, actual)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detect_virtualenv_from_env_var() {
    let temp = tempfile::tempdir().unwrap();
    let mock_venv = temp.path().join("custom_venv");
    std::fs::create_dir_all(&mock_venv).unwrap();

    let info =
      venv::detect_virtualenv_with_env(temp.path(), Some(mock_venv.clone()));
    assert!(info.is_active);
    assert_eq!(info.venv_path, Some(mock_venv));
    assert_eq!(info.source, venv::VirtualEnvSource::EnvVar);
  }

  #[test]
  fn detect_virtualenv_from_workspace_dirs() {
    for dir_name in &[".venv", "venv", "env", ".env"] {
      let temp = tempfile::tempdir().unwrap();
      let venv_dir = temp.path().join(dir_name);
      std::fs::create_dir_all(&venv_dir).unwrap();

      let info = venv::detect_virtualenv_with_env(temp.path(), None);
      assert!(!info.is_active);
      assert_eq!(info.venv_path, Some(venv_dir));
      assert_eq!(
        info.source,
        venv::VirtualEnvSource::Workspace(dir_name.to_string())
      );
    }
  }

  #[test]
  fn detect_virtualenv_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let dot_venv = temp.path().join(".venv");
    let venv = temp.path().join("venv");
    std::fs::create_dir_all(&dot_venv).unwrap();
    std::fs::create_dir_all(&venv).unwrap();

    let info = venv::detect_virtualenv_with_env(temp.path(), None);
    assert_eq!(info.venv_path, Some(dot_venv));
    assert_eq!(
      info.source,
      venv::VirtualEnvSource::Workspace(".venv".to_string())
    );
  }

  #[test]
  fn detect_virtualenv_none() {
    let temp = tempfile::tempdir().unwrap();
    let info = venv::detect_virtualenv_with_env(temp.path(), None);
    assert!(!info.is_active);
    assert_eq!(info.venv_path, None);
    assert_eq!(info.source, venv::VirtualEnvSource::None);
  }

  #[test]
  fn find_venv_interpreter() {
    let temp = tempfile::tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let python_bin = bin_dir.join("python");
    std::fs::write(&python_bin, "#!/bin/sh\n").unwrap();

    let found = venv::find_venv_interpreter(temp.path());
    assert_eq!(found, Some(python_bin));
  }

  #[test]
  fn is_pattern_ignored() {
    let lines = vec![
      "# Comments should be ignored",
      "",
      "target/",
      "/.ruff_cache/",
      "__pycache__",
      "**/node_modules/**",
      "!not_ignored",
    ];

    assert!(gitignore::is_pattern_ignored(&lines, "target"));
    assert!(gitignore::is_pattern_ignored(&lines, ".ruff_cache"));
    assert!(gitignore::is_pattern_ignored(&lines, "__pycache__"));
    assert!(gitignore::is_pattern_ignored(&lines, "node_modules"));
    assert!(!gitignore::is_pattern_ignored(&lines, ".pytest_cache"));
    assert!(!gitignore::is_pattern_ignored(&lines, "not_ignored"));
  }

  #[test]
  fn is_pattern_ignored_pyc_alias() {
    let lines = vec!["*.pyc"];
    assert!(gitignore::is_pattern_ignored(&lines, "__pycache__"));
  }

  #[test]
  fn check_gitignore_hygiene_all_satisfied() {
    let gitignore = r"
  /target/
  .ruff_cache/
  __pycache__/
  .pytest_cache/
  node_modules/
  ";
    let report = gitignore::check_gitignore_hygiene_content(
      Some(gitignore),
      true, // has_python
      true, // has_rust
      true, // has_js
    );
    assert!(report.gitignore_exists);
    assert!(report.issues.is_empty());
  }

  #[test]
  fn check_gitignore_hygiene_missing_entries() {
    let gitignore = r"
  target/
  ";
    let report = gitignore::check_gitignore_hygiene_content(
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
  fn check_gitignore_hygiene_no_file() {
    let report = gitignore::check_gitignore_hygiene_content(
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
  fn command_ran_successfully_true_on_zero_exit() {
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
  fn command_ran_successfully_false_on_nonzero_exit() {
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
  fn command_ran_successfully_false_on_spawn_error() {
    // A binary name that should never exist on PATH — spawning it fails
    // outright, which must not be mistaken for a successful run.
    let output =
      std::process::Command::new("definitely_not_a_real_binary_xyz_192")
        .output();
    assert!(!command_ran_successfully(&output));
  }

  /// Regression test for #192 [pre-recreation]: `clippy_probe_succeeds` (what [`on_path`]
  /// gates presence on for every alias clippy can be registered under —
  /// `"clippy"`, `"clippy-driver"`, `"cargo-clippy"`) must require an actual
  /// successful invocation, not just presence on `PATH`. Parameterized over
  /// the binary names lets this substitute the real `true`/`false` binaries as
  /// deterministic stand-ins for a functional vs. a present-but-broken shim,
  /// with no `PATH` mutation needed.
  #[cfg(unix)]
  #[test]
  fn clippy_probe_succeeds_requires_functional_driver() {
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

  /// Regression test for #192 [pre-recreation]: [`on_path`] must use the
  /// functional clippy probe for every name clippy is registered under,
  /// including `"clippy-driver"`, the one production code passes.
  #[test]
  fn on_path_uses_functional_clippy_probe_for_every_alias() {
    let functional = clippy_probe_succeeds("clippy-driver", "cargo");
    for alias in ["clippy", "clippy-driver", "cargo-clippy"] {
      assert_eq!(on_path(alias), functional, "{alias}");
    }
  }

  /// #106: an installer that exits 0 while the binary never lands on `PATH`
  /// is not an install, whatever the pin says.
  #[test]
  fn classify_checks_path_before_version() {
    let pin = version::Version::new(1, 2, 3);
    assert!(matches!(
      classify(false, "npm".into(), Some(pin.clone()), Some(pin)),
      Install::NotOnPath { .. }
    ));
    assert!(matches!(
      classify(false, "npm".into(), None, None),
      Install::NotOnPath { .. }
    ));
  }

  #[test]
  fn classify_compares_a_pinned_version() {
    let pin = version::Version::new(1, 2, 3);
    assert!(matches!(
      classify(true, "brew".into(), Some(pin.clone()), Some(pin.clone())),
      Install::Installed
    ));
    assert!(matches!(
      classify(
        true,
        "brew".into(),
        Some(pin.clone()),
        Some(version::Version::new(1, 2, 0))
      ),
      Install::Mismatch { .. }
    ));
    assert!(matches!(
      classify(true, "brew".into(), Some(pin), None),
      Install::Mismatch { actual: None, .. }
    ));
  }

  #[test]
  fn classify_unpinned_present_binary_is_installed() {
    assert!(matches!(
      classify(true, "cargo".into(), None, None),
      Install::Installed
    ));
  }

  #[test]
  fn find_system_python() {
    let found = venv::find_system_python();
    let expected = which::which("python3")
      .or_else(|_| which::which("python"))
      .ok();
    assert_eq!(found, expected);
  }
}
