//! End-to-end runs of the `fml` binary in scratch workspaces.

use std::fs;
use std::path;
use std::process;

use tempfile;

/// A scratch workspace holding `files`, with a `.git` so it is the root.
fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
  let dir = tempfile::tempdir().expect("tempdir");
  fs::create_dir(dir.path().join(".git")).expect("mkdir .git");
  for (name, text) in files {
    let file = dir.path().join(name);
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
  }
  dir
}

/// Runs fml with `args` in `cwd`.
fn fml(cwd: &path::Path, args: &[&str]) -> process::Output {
  process::Command::new(env!("CARGO_BIN_EXE_fml-cli"))
    .args(args)
    .current_dir(cwd)
    .env("XDG_CONFIG_HOME", cwd.join(".no-user-config"))
    .output()
    .expect("fml runs")
}

/// What `output` printed to stdout.
fn stdout(output: &process::Output) -> String {
  String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What `output` printed to stderr.
fn stderr(output: &process::Output) -> String {
  String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_clean_run_exits_0_and_quiet_prints_nothing() {
  let dir = workspace(&[("a.toml", "a = 1\n")]);
  let output = fml(dir.path(), &["fmt", "--check"]);
  assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
  assert_eq!(stdout(&output), "0 files would be formatted\n");

  let output = fml(dir.path(), &["-q", "fmt", "--check"]);
  assert_eq!(output.status.code(), Some(0));
  assert_eq!(stdout(&output), "");
  assert_eq!(stderr(&output), "");
}

#[test]
fn violations_exit_1_and_quiet_prints_only_them() {
  let dir = workspace(&[("a.toml", "a=1\n"), ("b.toml", "b = 1\n")]);
  let output = fml(dir.path(), &["-q", "fmt", "--check"]);
  assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
  assert_eq!(stdout(&output), "would format a.toml\n");

  let output = fml(dir.path(), &["fmt"]);
  assert_eq!(output.status.code(), Some(0));
  assert_eq!(stdout(&output), "formatted a.toml\n1 file formatted\n");
  let text = fs::read_to_string(dir.path().join("a.toml")).expect("read");
  assert_eq!(text, "a = 1\n");
}

#[test]
fn paths_print_relative_to_the_current_directory() {
  let dir = workspace(&[("a.toml", "a=1\n"), ("sub/b.toml", "b=1\n")]);
  let output = fml(&dir.path().join("sub"), &["-q", "fmt", "--check"]);
  let up = path::Path::new("..").join("a.toml");
  let want = format!("would format {}\nwould format b.toml\n", up.display());
  assert_eq!(stdout(&output), want);
}

#[test]
fn a_missing_tool_exits_2_naming_its_install_command() {
  let dir = workspace(&[("a.sh", "echo a\n")]);
  let output = process::Command::new(env!("CARGO_BIN_EXE_fml-cli"))
    .args(["-q", "fmt"])
    .current_dir(dir.path())
    .env("PATH", dir.path().join("empty"))
    .output()
    .expect("fml runs");
  assert_eq!(output.status.code(), Some(2));
  let err = stderr(&output);
  assert!(err.starts_with("[ERROR] shell: shfmt not found"), "{err}");
  assert!(err.contains("go install mvdan.cc/sh/v3/cmd/shfmt"), "{err}");
}

#[test]
fn an_unknown_config_key_exits_2_with_file_key_and_line() {
  let config = "[global]\nindent_size = 2\nindent = 4\n";
  let dir = workspace(&[("formality.toml", config), ("a.toml", "a = 1\n")]);
  let output = fml(dir.path(), &["fmt"]);
  assert_eq!(output.status.code(), Some(2));
  let err = stderr(&output);
  assert!(err.contains("formality.toml"), "{err}");
  assert!(err.contains("line 3"), "{err}");
  assert!(err.contains("unknown field `indent`"), "{err}");
}

#[test]
fn a_named_path_that_does_not_exist_exits_2() {
  let dir = workspace(&[]);
  let output = fml(dir.path(), &["fmt", "missing.toml"]);
  assert_eq!(output.status.code(), Some(2));
  assert!(stderr(&output).starts_with("[ERROR] "));
}

/// Runs git with `args` in `dir`.
fn git(dir: &path::Path, args: &[&str]) {
  let status = process::Command::new("git")
    .args(["-c", "user.name=fml", "-c", "user.email=fml@example.com"])
    .args(args)
    .current_dir(dir)
    .status()
    .expect("git runs");
  assert!(status.success(), "git {args:?}");
}

#[test]
fn staged_and_changed_select_exactly_their_files() {
  let dir = tempfile::tempdir().expect("tempdir");
  let root = dir.path();
  git(root, &["init", "-q"]);
  for name in ["clean", "modified", "staged", "deleted"] {
    fs::write(root.join(format!("{name}.toml")), "a = 1\n").expect("write");
  }
  git(root, &["add", "."]);
  git(root, &["commit", "-q", "-m", "init"]);
  fs::write(root.join("modified.toml"), "a=2\n").expect("write");
  fs::write(root.join("staged.toml"), "a=2\n").expect("write");
  git(root, &["add", "staged.toml"]);
  fs::write(root.join("untracked.toml"), "a=2\n").expect("write");
  fs::remove_file(root.join("deleted.toml")).expect("rm");

  let output = fml(root, &["-q", "fmt", "--check", "--staged"]);
  assert_eq!(
    stdout(&output),
    "would format staged.toml\n",
    "{}",
    stderr(&output)
  );

  let output = fml(root, &["-q", "fmt", "--check", "--changed"]);
  let want = "would format modified.toml\nwould format staged.toml\n\
              would format untracked.toml\n";
  assert_eq!(stdout(&output), want, "{}", stderr(&output));

  let output = fml(root, &["-q", "fmt", "--check", "--changed", "staged.toml"]);
  assert_eq!(
    stdout(&output),
    "would format staged.toml\n",
    "a path filters"
  );
}
