//! Shared test helpers for integration tests.
//!
//! Provides reusable synthetic repository builders and CLI invocation helpers
//! to reduce boilerplate across test binaries.

#![allow(dead_code, missing_docs)]

use fml::cli::{Cli, Commands};
use fml::errors::ExitStatus;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Creates a temporary directory populated with the given `(relative_path, content)` files.
/// Parent directories are created automatically for any nested file paths.
pub fn temp_repo(files: &[(&str, &str)]) -> TempDir {
  let temp = TempDir::new().expect("failed to create temporary directory");
  let root = temp.path();
  for (rel_path, content) in files {
    let dest = root.join(rel_path);
    if let Some(parent) = dest.parent() {
      fs::create_dir_all(parent).expect("failed to create parent directories");
    }
    fs::write(&dest, content).expect("failed to write fixture file");
  }
  temp
}

/// Executes a CLI command targeted at the given root directory.
pub fn run_cli(root: impl AsRef<Path>, command: Commands) -> ExitStatus {
  let args = Cli {
    config: None,
    root: Some(root.as_ref().to_path_buf()),
    command,
  };
  fml::run_with_args(args)
}

/// Executes a CLI command without specifying a root directory (global / ambient mode).
pub fn run_cli_no_root(command: Commands) -> ExitStatus {
  let args = Cli {
    config: None,
    root: None,
    command,
  };
  fml::run_with_args(args)
}

/// Initializes a git repository in `path` with a dummy committer identity.
/// Returns `true` if git was successfully initialized.
pub fn init_git_repo(path: impl AsRef<Path>) -> bool {
  let root = path.as_ref();
  let init_ok = std::process::Command::new("git")
    .arg("init")
    .current_dir(root)
    .output()
    .map(|o| o.status.success())
    .unwrap_or(false);
  if !init_ok {
    return false;
  }
  let _ = std::process::Command::new("git")
    .args(["config", "user.name", "test"])
    .current_dir(root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["config", "user.email", "test@example.com"])
    .current_dir(root)
    .output();
  true
}

/// Helper to create a `Commands::Init` command.
pub fn init_cmd(force: bool, hidden: bool) -> Commands {
  Commands::Init { force, hidden }
}

/// Helper to create a `Commands::Sync` command.
pub fn sync_cmd(check: bool, lang: &[&str]) -> Commands {
  Commands::Sync {
    check,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
  }
}

/// Helper to create a standard `Commands::Fmt` command.
pub fn fmt_cmd(check: bool, lang: &[&str]) -> Commands {
  Commands::Fmt {
    check,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    install: false,
    paths: vec![],
  }
}

/// Helper to create a standard `Commands::Fix` command.
pub fn fix_cmd(check: bool, lang: &[&str]) -> Commands {
  Commands::Fix {
    check,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    install: false,
    paths: vec![],
  }
}

/// Helper to create a standard `Commands::Lint` command.
pub fn lint_cmd(fix: bool, lang: &[&str]) -> Commands {
  Commands::Lint {
    fix,
    check: false,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    install: false,
    paths: vec![],
  }
}
