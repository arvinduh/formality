//! Shared helpers: synthetic repositories and running a pass through the
//! library the way `fml` does.

use std::fs;
use std::path;
use std::process;

use tempfile;

use fml::config;
use fml::engine::runner;
use fml::engine::target;

/// A pass to run, mirroring the `fml` subcommand of the same name.
#[derive(Debug, Clone)]
pub enum Command {
  /// `fml fmt`.
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
  /// `fml lint`.
  Lint {
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
  /// `fml fix`.
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
  /// `fml sync`.
  Sync {
    /// Check without writing.
    check: bool,
    /// Filter by language.
    lang: Vec<String>,
  },
}

/// Runs `command` against `root` through the library, returning the exit
/// status `fml` would.
pub fn run_cli(
  root: impl AsRef<path::Path>,
  command: &Command,
) -> runner::ExitStatus {
  let root = path::absolute(root.as_ref()).unwrap();
  let project = config::resolve::find_project_config(&root);
  let Ok((config, _)) =
    config::FormalityConfig::load_layered_with_path(project.as_deref())
  else {
    return runner::ExitStatus::Error;
  };
  let (selection, plan) = match command.clone() {
    Command::Fmt {
      check,
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => (
      (staged, changed, lang, paths),
      runner::Plan::fmt(check, allow_missing),
    ),
    Command::Lint {
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => (
      (staged, changed, lang, paths),
      runner::Plan::lint(allow_missing),
    ),
    Command::Fix {
      check,
      staged,
      changed,
      lang,
      allow_missing,
      paths,
    } => (
      (staged, changed, lang, paths),
      runner::Plan::fix(check, allow_missing),
    ),
    Command::Sync { check, lang } => {
      ((false, false, lang, Vec::new()), runner::Plan::sync(check))
    }
  };
  let (staged, changed, lang, paths) = selection;
  let git_filter = match (staged, changed) {
    (true, _) => Some(target::Changes::Staged),
    (_, true) => Some(target::Changes::Changed),
    _ => None,
  };
  let target =
    match target::resolve_targets(&root, git_filter, paths, &lang, &config) {
      Ok(Some(target)) => target,
      Ok(None) => return runner::ExitStatus::Clean,
      Err(_) => return runner::ExitStatus::Error,
    };
  let results =
    runner::Runner::run(&target.surfaces, &root, &target.scope, &plan, &config);
  runner::compute_exit_status(&results, plan.allow_missing)
}

/// Creates a temporary directory holding `(relative_path, content)` files,
/// creating parent directories as needed.
pub fn temp_repo(files: &[(&str, &str)]) -> tempfile::TempDir {
  let temp = tempfile::TempDir::new().expect("create temp dir");
  for (rel_path, content) in files {
    let dest = temp.path().join(rel_path);
    if let Some(parent) = dest.parent() {
      fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&dest, content).expect("write fixture file");
  }
  temp
}

/// Initializes a git repository in `path` with a dummy committer identity.
/// Returns `false` when git is unavailable.
pub fn init_git_repo(path: impl AsRef<path::Path>) -> bool {
  let root = path.as_ref();
  let git = |args: &[&str]| {
    process::Command::new("git")
      .args(args)
      .current_dir(root)
      .output()
      .is_ok_and(|o| o.status.success())
  };
  git(&["init"])
    && git(&["config", "user.name", "test"])
    && git(&["config", "user.email", "test@example.com"])
}

/// `fml sync`, optionally `--check`, over `lang`.
pub fn sync_cmd(check: bool, lang: &[&str]) -> Command {
  Command::Sync {
    check,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
  }
}

/// `fml fmt`, optionally `--check`, over `lang`.
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

/// `fml fix`, optionally `--check`, over `lang`.
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

/// `fml lint` over `lang`.
pub fn lint_cmd(lang: &[&str]) -> Command {
  Command::Lint {
    staged: false,
    changed: false,
    lang: lang.iter().map(|s| (*s).to_string()).collect(),
    allow_missing: false,
    paths: vec![],
  }
}
