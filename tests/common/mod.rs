//! Shared test helpers for integration tests.
//!
//! Provides reusable synthetic repository builders and CLI invocation helpers
//! to reduce boilerplate across test binaries.

#![expect(
  dead_code,
  reason = "shared helpers are not all used by every integration test binary"
)]

pub mod lsp;

use std::fs;
use std::path;
use std::sync;
use tempfile;

use fml::commands;
use fml::config;
use fml::errors;

/// Test representation of CLI commands for integration test dispatch.
#[derive(Debug, Clone)]
pub enum Command {
  /// Format source files.
  Fmt {
    /// Report what would be reformatted without writing.
    check: bool,
    /// Only act on staged files.
    staged: bool,
    /// Only act on changed files.
    changed: bool,
    /// Filter by language.
    lang: Vec<String>,
    /// Allow missing tools.
    allow_missing: bool,
    /// Target paths.
    paths: Vec<path::PathBuf>,
  },
  /// Lint source files.
  Lint {
    /// Check flag (rejected or ignored in lint).
    check: bool,
    /// Only act on staged files.
    staged: bool,
    /// Only act on changed files.
    changed: bool,
    /// Filter by language.
    lang: Vec<String>,
    /// Allow missing tools.
    allow_missing: bool,
    /// Target paths.
    paths: Vec<path::PathBuf>,
  },
  /// Apply lint fixes, then reformat.
  Fix {
    /// Report whether fixes would change anything.
    check: bool,
    /// Only act on staged files.
    staged: bool,
    /// Only act on changed files.
    changed: bool,
    /// Filter by language.
    lang: Vec<String>,
    /// Allow missing tools.
    allow_missing: bool,
    /// Target paths.
    paths: Vec<path::PathBuf>,
  },
  /// Sync native tool configs.
  Sync {
    /// Check whether configs are in sync without writing.
    check: bool,
    /// Filter by language.
    lang: Vec<String>,
  },
  /// Diagnose installed toolchains.
  Doctor {
    /// Inspect all supported surfaces.
    all: bool,
    /// Automatically install missing toolchains.
    install: bool,
  },
  /// Scaffold a new formality.toml.
  Init {
    /// Overwrite existing config.
    force: bool,
    /// Create hidden config (.formality.toml).
    hidden: bool,
  },
  /// Write JSON schema to stdout or file.
  Schema {
    /// Optional output file path.
    output: Option<path::PathBuf>,
  },
}

/// Dispatches a test command directly through library APIs.
fn dispatch_command(root: &path::Path, command: Command) -> errors::ExitStatus {
  let project_config_path = config::find_project_config(root);
  let (config, _) = match config::FormalityConfig::load_layered_with_path(
    project_config_path.as_deref(),
  ) {
    Ok(res) => res,
    Err(e) => {
      errors::FormalityError::from(e).print_diagnostic();
      return errors::ExitStatus::Error;
    }
  };

  match command {
    Command::Fmt {
      check,
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => commands::fmt::run_fmt(
      root,
      &config,
      check,
      staged,
      changed,
      &lang,
      paths,
      allow_missing,
    ),
    Command::Lint {
      staged,
      changed,
      lang,
      allow_missing,
      paths,
      ..
    } => commands::lint::run_lint(
      root,
      &config,
      staged,
      changed,
      &lang,
      paths,
      allow_missing,
    ),
    Command::Fix {
      check,
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => commands::fix::run_fix(
      root,
      &config,
      check,
      staged,
      changed,
      &lang,
      paths,
      allow_missing,
    ),
    Command::Sync { check, lang } => {
      commands::sync::run_sync(root, &config, check, &lang)
    }
    Command::Doctor { all, install } => {
      commands::doctor::run_doctor(root, all, install, &config)
    }
    Command::Init { force, hidden } => {
      commands::init::run_init(root, &config, force, hidden)
    }
    Command::Schema { output } => commands::schema::run_schema(output),
  }
}

/// Orders in-process fml runs against overrides of fml's process-wide binary
/// cache, which every test thread in this binary shares.
///
/// [`run_cli`] and [`run_cli_no_root`] hold a read guard for the whole run; a
/// test that overrides the cache holds the write guard from before its first
/// override until after its last eviction. No run can therefore observe, or
/// race a cold lookup against, an override it did not install. Read guards
/// never poison, and every acquisition recovers a poisoned write guard, so a
/// failing test cannot fail others through this lock.
static BINARY_CACHE_LOCK: sync::RwLock<()> = sync::RwLock::new(());

/// Write access to fml's binary cache, for simulating missing tools.
///
/// `Drop` evicts every hidden binary before the write guard releases, even
/// while unwinding, so the next run resolves them afresh.
pub struct BinaryOverride {
  hidden: Vec<&'static str>,
  _exclusive: sync::RwLockWriteGuard<'static, ()>,
}

impl BinaryOverride {
  /// Waits for every in-process run in this test binary to finish, then
  /// blocks new ones until the returned value drops.
  pub fn lock() -> Self {
    let exclusive = BINARY_CACHE_LOCK
      .write()
      .unwrap_or_else(sync::PoisonError::into_inner);
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
  pub fn run_cli(
    &self,
    root: &path::Path,
    command: Command,
  ) -> errors::ExitStatus {
    dispatch_command(root, command)
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
pub fn temp_repo(files: &[(&str, &str)]) -> tempfile::TempDir {
  let temp =
    tempfile::TempDir::new().expect("failed to create temporary directory");
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
pub fn run_cli(
  root: impl AsRef<path::Path>,
  command: Command,
) -> errors::ExitStatus {
  let _shared = BINARY_CACHE_LOCK
    .read()
    .unwrap_or_else(sync::PoisonError::into_inner);
  dispatch_command(root.as_ref(), command)
}

/// Executes a CLI command without specifying a root directory (global / ambient mode).
pub fn run_cli_no_root(command: Command) -> errors::ExitStatus {
  let _shared = BINARY_CACHE_LOCK
    .read()
    .unwrap_or_else(sync::PoisonError::into_inner);
  let cwd =
    std::env::current_dir().unwrap_or_else(|_| path::PathBuf::from("."));
  dispatch_command(&cwd, command)
}

/// Initializes a git repository in `path` with a dummy committer identity.
/// Returns `true` if git was successfully initialized.
pub fn init_git_repo(path: impl AsRef<path::Path>) -> bool {
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

/// Helper to create a `Command::Init` command.
pub fn init_cmd(force: bool, hidden: bool) -> Command {
  Command::Init { force, hidden }
}

/// Helper to create a `Command::Sync` command.
pub fn sync_cmd(check: bool, lang: &[&str]) -> Command {
  Command::Sync {
    check,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
  }
}

/// Helper to create a standard `Command::Fmt` command.
pub fn fmt_cmd(check: bool, lang: &[&str]) -> Command {
  Command::Fmt {
    check,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    allow_missing: false,
    paths: vec![],
  }
}

/// Helper to create a standard `Command::Fix` command.
pub fn fix_cmd(check: bool, lang: &[&str]) -> Command {
  Command::Fix {
    check,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    allow_missing: false,
    paths: vec![],
  }
}

/// Helper to create a standard `Command::Lint` command.
pub fn lint_cmd(lang: &[&str]) -> Command {
  Command::Lint {
    check: false,
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    allow_missing: false,
    paths: vec![],
  }
}

/// Helper to create a `Command::Doctor` command.
pub fn doctor_cmd(all: bool, install: bool) -> Command {
  Command::Doctor { all, install }
}

/// Helper to create a `Command::Schema` command.
pub fn schema_cmd(output: Option<path::PathBuf>) -> Command {
  Command::Schema { output }
}
