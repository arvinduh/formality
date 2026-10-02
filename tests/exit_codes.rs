//! End-to-end check that the `fml` binary exits 0, 1 and 2 for clean,
//! violations and error, as `main` maps them from `ExitStatus` (Issue #496).
//!
//! Drives `fml sync --check`, which shells out to no external tool, so the
//! test is hermetic whatever formatters are installed.

use std::path::Path;
use std::process::Command;

/// Runs `fml sync` against `root` with `extra` args and returns its exit code.
fn sync_exit_code(root: &Path, extra: &[&str]) -> Option<i32> {
  Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("sync")
    .arg("--root")
    .arg(root)
    .args(extra)
    .env("NO_COLOR", "1")
    .output()
    .expect("failed to run fml sync")
    .status
    .code()
}

#[test]
fn test_binary_exit_codes_for_clean_violations_and_error() {
  let ok = tempfile::tempdir().expect("tempdir");
  std::fs::write(
    ok.path().join("formality.toml"),
    "[global]\nlanguages = [\"json\"]\n",
  )
  .unwrap();
  std::fs::write(ok.path().join("data.json"), "{ \"a\": 1 }\n").unwrap();

  // Native configs not yet generated: drift is a violation.
  assert_eq!(sync_exit_code(ok.path(), &["--check"]), Some(1));
  assert_eq!(sync_exit_code(ok.path(), &[]), Some(0));
  assert_eq!(sync_exit_code(ok.path(), &["--check"]), Some(0));

  let bad = tempfile::tempdir().expect("tempdir");
  std::fs::write(bad.path().join("formality.toml"), "[global\n").unwrap();
  assert_eq!(sync_exit_code(bad.path(), &["--check"]), Some(2));
}
