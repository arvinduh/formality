//! Shared test helpers for integration tests.
//!
//! Provides reusable synthetic repository builders and CLI invocation helpers
//! to reduce boilerplate across test binaries.

#![allow(dead_code)]

pub mod lsp;

use fml::cli::{Cli, Commands};
use fml::errors::ExitStatus;
use std::fs;
use std::path::Path;
use std::sync::{PoisonError, RwLock, RwLockWriteGuard};
use tempfile::TempDir;

/// Orders in-process fml runs against overrides of fml's process-wide binary
/// cache, which every test thread in this binary shares.
///
/// [`run_cli`] and [`run_cli_no_root`] hold a read guard for the whole run; a
/// test that overrides the cache holds the write guard from before its first
/// override until after its last eviction. No run can therefore observe, or
/// race a cold lookup against, an override it did not install. Read guards
/// never poison, and every acquisition recovers a poisoned write guard, so a
/// failing test cannot fail others through this lock.
static BINARY_CACHE_LOCK: RwLock<()> = RwLock::new(());

/// Write access to fml's binary cache, for simulating missing tools.
///
/// `Drop` evicts every hidden binary before the write guard releases, even
/// while unwinding, so the next run resolves them afresh.
pub struct BinaryOverride {
  hidden: Vec<&'static str>,
  _exclusive: RwLockWriteGuard<'static, ()>,
}

impl BinaryOverride {
  /// Waits for every in-process run in this test binary to finish, then
  /// blocks new ones until the returned value drops.
  pub fn lock() -> Self {
    let exclusive = BINARY_CACHE_LOCK
      .write()
      .unwrap_or_else(PoisonError::into_inner);
    Self {
      hidden: Vec::new(),
      _exclusive: exclusive,
    }
  }

  /// Makes `binary` resolve as not installed until this override drops.
  pub fn hide(&mut self, binary: &'static str) {
    fml::surfaces::set_binary_path_for_test(binary, None);
    self.hidden.push(binary);
  }

  /// Runs `command` under this override; [`run_cli`] would deadlock here.
  #[expect(
    clippy::unused_self,
    reason = "`&self` proves the caller holds the binary-cache lock"
  )]
  pub fn run_cli(&self, root: &Path, command: Commands) -> ExitStatus {
    fml::run_with_args(Cli {
      config: None,
      root: Some(root.to_path_buf()),
      command,
    })
  }
}

impl Drop for BinaryOverride {
  fn drop(&mut self) {
    for binary in &self.hidden {
      fml::surfaces::forget_binary(binary);
    }
  }
}

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
  let _shared = BINARY_CACHE_LOCK
    .read()
    .unwrap_or_else(PoisonError::into_inner);
  let args = Cli {
    config: None,
    root: Some(root.as_ref().to_path_buf()),
    command,
  };
  fml::run_with_args(args)
}

/// Executes a CLI command without specifying a root directory (global / ambient mode).
pub fn run_cli_no_root(command: Commands) -> ExitStatus {
  let _shared = BINARY_CACHE_LOCK
    .read()
    .unwrap_or_else(PoisonError::into_inner);
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
    .is_ok_and(|o| o.status.success());
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
    allow_missing: false,
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
    allow_missing: false,
    paths: vec![],
  }
}

/// Helper to create a standard `Commands::Lint` command.
pub fn lint_cmd(lang: &[&str]) -> Commands {
  Commands::Lint {
    check: false,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    allow_missing: false,
    paths: vec![],
  }
}
