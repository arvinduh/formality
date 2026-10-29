//! `--quiet` prints nothing on a clean run and only the problems otherwise,
//! the contract agent hooks rely on.

use std::path;
use std::process;

/// Runs `fml fmt --check --quiet` in `root` over `paths` with an empty `PATH`
/// and home, so no formatter is found, and returns the process output.
fn quiet_check(root: &path::Path, paths: &[&path::Path]) -> process::Output {
  let empty = root.join("empty");
  std::fs::create_dir_all(&empty).unwrap();
  process::Command::new(env!("CARGO_BIN_EXE_fml"))
    .args(["fmt", "--check", "--quiet", "--root"])
    .arg(root)
    .args(paths)
    .env("NO_COLOR", "1")
    .env("PATH", &empty)
    .env("HOME", &empty)
    .env("USERPROFILE", &empty)
    .env("LOCALAPPDATA", &empty)
    .output()
    .expect("failed to run fml fmt")
}

#[test]
fn quiet_prints_nothing_when_there_is_nothing_to_report() {
  let dir = tempfile::tempdir().expect("tempdir");
  let empty = dir.path().join("src");
  std::fs::create_dir(&empty).unwrap();

  let output = quiet_check(dir.path(), &[&empty]);
  assert_eq!(output.status.code(), Some(0));
  assert!(output.stdout.is_empty(), "{output:?}");
  assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn quiet_reports_only_the_missing_tool() {
  let dir = tempfile::tempdir().expect("tempdir");
  std::fs::write(
    dir.path().join("formality.toml"),
    "[global]\nlanguages = [\"json\"]\n",
  )
  .unwrap();
  std::fs::write(dir.path().join("data.json"), "{\"a\":1}\n").unwrap();

  let output = quiet_check(dir.path(), &[]);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(1));
  assert!(output.stdout.is_empty(), "{output:?}");
  assert!(stderr.contains("prettier not found"), "{stderr}");
  assert_eq!(stderr.lines().count(), 1, "{stderr}");
}
