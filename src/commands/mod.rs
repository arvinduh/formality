//! Standalone command implementations for the Formality CLI.

/// Doctor diagnostic commands for workspace and toolchain verification.
pub mod doctor;
/// In-place autofix CLI command handler.
pub mod fix;
/// Code formatting CLI command handler.
pub mod fmt;
/// Configuration initialization CLI command handler.
pub mod init;
/// Code linting CLI command handler.
pub mod lint;
/// Language Server Protocol passthrough server implementation.
pub mod lsp;
/// Structured per-violation lint diagnostics for `fml lsp` (Fixes #159).
pub mod lsp_diagnostics;
/// Config schema-reference migration CLI command handler.
pub mod migrate;
/// JSON Schema generator CLI command handler.
pub mod schema;
/// Language surfaces inspector CLI command handler.
pub mod surfaces;
/// Native configuration synchronization CLI command handler.
pub mod sync;
/// Output table formatting helper CLI command.
pub mod table;

use std::path::{Path, PathBuf};

use colored::Colorize;

use crate::config::FormalityConfig;
use crate::engine::{Runner, RunnerAction};
use crate::errors::{ExitStatus, FormalityError, GitError, SurfaceError};
use crate::surfaces::{
  LanguageSurface, all_surfaces, detect_surfaces_smart, find_files_with_ext,
  get_surface_by_name,
};

/// Prints a warning that one or more required tools failed to auto-install,
/// so the affected language(s) may have been skipped for this `verb`.
pub fn warn_tool_install_failed(verb: &str) {
  eprintln!(
    "{} One or more required tools failed to install automatically; {verb} may be skipped for affected languages.",
    "[WARN]".yellow().bold()
  );
}

/// Dispatches a surface action across target surfaces for `fmt`, `lint`, and
/// `fix` commands after resolving git paths, target surfaces, and preflight tool
/// requirements.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_surface_action(
  root: &Path,
  config: &FormalityConfig,
  staged: bool,
  changed: bool,
  lang: Vec<String>,
  install: bool,
  paths: Vec<PathBuf>,
  action: RunnerAction,
  verb: &'static str,
) -> ExitStatus {
  let target_paths = match resolve_git_paths(root, staged, changed, paths) {
    Ok(p) => p,
    Err(e) => {
      e.print_diagnostic();
      return ExitStatus::Error;
    }
  };

  let surfaces =
    match resolve_target_surfaces(root, &lang, &target_paths, config) {
      Ok(s) => s,
      Err(e) => {
        e.print_diagnostic();
        return ExitStatus::Error;
      }
    };

  let (for_fmt, for_lint) = match action {
    RunnerAction::Format { .. } => (true, false),
    RunnerAction::Lint { .. } => (false, true),
    RunnerAction::Fix => (true, true),
    RunnerAction::Sync { .. } => (false, false),
  };

  let mut install_failed = false;
  if install {
    if !crate::commands::doctor::preflight_install(
      &surfaces, config, for_fmt, for_lint,
    ) {
      warn_tool_install_failed(verb);
      install_failed = true;
    }
  } else {
    crate::commands::doctor::preflight_warn_stale_tools(
      &surfaces, config, for_fmt, for_lint,
    );
  }

  let status = Runner::run(surfaces, root, &target_paths, action, config);
  if install_failed && status.is_clean() {
    ExitStatus::Error
  } else {
    status
  }
}

fn normalize_path(path: &Path) -> PathBuf {
  let mut components = Vec::new();
  for component in path.components() {
    match component {
      std::path::Component::CurDir => {}
      std::path::Component::ParentDir => {
        if let Some(std::path::Component::Normal(_)) = components.last() {
          components.pop();
        } else {
          components.push(component);
        }
      }
      c => components.push(c),
    }
  }
  components.iter().collect()
}

/// Resolves the target file paths for a command from its `--staged`/
/// `--changed`/explicit-path flags. `staged` and `changed` are mutually
/// exclusive; if neither is set, `explicit_paths` is returned as-is.
/// When `staged` or `changed` is set alongside `explicit_paths`, the
/// git-discovered file list is filtered to only include files matching
/// the explicit paths.
///
/// # Errors
///
/// Returns a [`FormalityError`] if both `staged` and `changed` are set, or if
/// the underlying git query fails.
pub fn resolve_git_paths(
  root: &Path,
  staged: bool,
  changed: bool,
  explicit_paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, FormalityError> {
  if staged && changed {
    return Err(FormalityError::Git(GitError::MutuallyExclusiveFlags));
  }
  if !staged && !changed {
    return Ok(explicit_paths);
  }

  let git_files = if staged {
    get_git_staged_files(root)?
  } else {
    get_git_changed_files(root)?
  };

  if explicit_paths.is_empty() {
    return Ok(git_files);
  }

  let norm_root = normalize_path(root);
  let normalized_explicit: Vec<(PathBuf, PathBuf)> = explicit_paths
    .iter()
    .map(|p| {
      let abs = if p.is_absolute() {
        normalize_path(p)
      } else {
        normalize_path(&norm_root.join(p))
      };
      let rel = normalize_path(p);
      (abs, rel)
    })
    .collect();

  let filtered = git_files
    .into_iter()
    .filter(|file| {
      let norm_file = normalize_path(file);
      let norm_rel_file =
        norm_file.strip_prefix(&norm_root).unwrap_or(&norm_file);
      normalized_explicit.iter().any(|(abs_exp, rel_exp)| {
        norm_file.starts_with(abs_exp)
          || norm_rel_file.starts_with(rel_exp)
          || norm_rel_file.starts_with(abs_exp)
      })
    })
    .collect();

  Ok(filtered)
}

fn get_git_diff_files(
  root: &Path,
  staged: bool,
  error_context: &str,
) -> Result<Vec<PathBuf>, FormalityError> {
  let mut cmd = std::process::Command::new("git");
  cmd.arg("diff").arg("--name-only");
  if staged {
    cmd.arg("--cached");
  }
  cmd.arg("--diff-filter=ACMR").current_dir(root);

  let output = cmd.output().map_err(|e| {
    FormalityError::Git(GitError::ExecutionFailed(e.to_string()))
  })?;

  if !output.status.success() {
    return Err(FormalityError::Git(GitError::CommandFailed(format!(
      "Failed to query git {error_context} files."
    ))));
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let files: Vec<PathBuf> = stdout
    .lines()
    .map(|l| root.join(l.trim()))
    .filter(|p| p.is_file())
    .collect();

  Ok(files)
}

/// Returns the list of staged git files relative to `root`.
///
/// # Errors
///
/// Returns a [`FormalityError`] if git execution fails or the git command cannot be run.
pub fn get_git_staged_files(
  root: &Path,
) -> Result<Vec<PathBuf>, FormalityError> {
  get_git_diff_files(root, true, "staged")
}

/// Returns the list of changed git files relative to `root`.
///
/// # Errors
///
/// Returns a [`FormalityError`] if git execution fails or the git command cannot be run.
pub fn get_git_changed_files(
  root: &Path,
) -> Result<Vec<PathBuf>, FormalityError> {
  get_git_diff_files(root, false, "changed")
}

/// Resolves which language surfaces a command should act on: an explicit
/// `lang_filter` wins outright, otherwise surfaces are narrowed to those with
/// matching files under `paths`, falling back to full smart detection when
/// neither is given.
///
/// # Errors
///
/// Returns a [`FormalityError`] if `lang_filter` names a surface that doesn't
/// exist.
pub fn resolve_target_surfaces(
  root: &Path,
  lang_filter: &[String],
  paths: &[PathBuf],
  config: &FormalityConfig,
) -> Result<Vec<Box<dyn LanguageSurface>>, FormalityError> {
  if !lang_filter.is_empty() {
    let mut selected = Vec::new();
    for name in lang_filter {
      if let Some(s) = get_surface_by_name(name) {
        selected.push(s);
      } else {
        return Err(FormalityError::Surface(SurfaceError::UnknownSurface(
          name.clone(),
        )));
      }
    }
    return Ok(selected);
  }

  if !paths.is_empty() {
    let mut active = Vec::new();
    let global = config.resolve_global();
    for surface in all_surfaces() {
      let lang_cfg =
        config.resolve_for_lang_with_global(surface.name(), &global);
      let matching = find_files_with_ext(
        root,
        surface.file_extensions(),
        paths,
        &lang_cfg.files,
        &lang_cfg.exclude,
      );
      if !matching.is_empty() {
        active.push(surface);
      }
    }
    return Ok(active);
  }

  Ok(detect_surfaces_smart(root, config))
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  #[test]
  fn test_normalize_path_components() {
    let p = Path::new("a/b/../c/./d");
    let norm = normalize_path(p);
    assert_eq!(norm, PathBuf::from("a/c/d"));

    let p2 = Path::new("./a/b");
    let norm2 = normalize_path(p2);
    assert_eq!(norm2, PathBuf::from("a/b"));
  }

  #[test]
  fn test_resolve_git_paths_mutual_exclusion() {
    let res = resolve_git_paths(Path::new("."), true, true, vec![]);
    assert!(matches!(
      res,
      Err(FormalityError::Git(GitError::MutuallyExclusiveFlags))
    ));
  }

  #[test]
  fn test_resolve_git_paths_no_git_flags_returns_explicit() {
    let explicit =
      vec![PathBuf::from("src/main.rs"), PathBuf::from("README.md")];
    let res = resolve_git_paths(Path::new("."), false, false, explicit.clone())
      .unwrap();
    assert_eq!(res, explicit);
  }

  #[test]
  fn test_resolve_git_paths_staged_filtering() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Initialize git repository
    let init_ok = std::process::Command::new("git")
      .arg("init")
      .current_dir(root)
      .output()
      .map(|o| o.status.success())
      .unwrap_or(false);
    if !init_ok {
      return;
    }

    // Configure user name/email for commit
    let _ = std::process::Command::new("git")
      .args(["config", "user.name", "test"])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["config", "user.email", "test@example.com"])
      .current_dir(root)
      .output();

    let src = root.join("src");
    let tests = root.join("tests");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&tests).unwrap();

    let file_a = src.join("a.rs");
    let file_b = src.join("b.rs");
    let file_c = tests.join("c.rs");
    fs::write(&file_a, "fn a() {}\n").unwrap();
    fs::write(&file_b, "fn b() {}\n").unwrap();
    fs::write(&file_c, "fn c() {}\n").unwrap();

    // Initial commit so HEAD exists
    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["commit", "-m", "initial"])
      .current_dir(root)
      .output();

    // Modify all 3 files and stage file_a and file_c
    fs::write(&file_a, "fn a_mod() {}\n").unwrap();
    fs::write(&file_b, "fn b_mod() {}\n").unwrap();
    fs::write(&file_c, "fn c_mod() {}\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "src/a.rs", "tests/c.rs"])
      .current_dir(root)
      .output();

    // 1. Staged without explicit paths returns both staged files
    let staged_all = resolve_git_paths(root, true, false, vec![]).unwrap();
    assert_eq!(staged_all.len(), 2);
    assert!(staged_all.contains(&file_a));
    assert!(staged_all.contains(&file_c));
    assert!(!staged_all.contains(&file_b));

    // 2. Staged filtered by explicit directory "src"
    let staged_src =
      resolve_git_paths(root, true, false, vec![PathBuf::from("src")]).unwrap();
    assert_eq!(staged_src.len(), 1);
    assert_eq!(staged_src[0], file_a);

    // 3. Staged filtered by explicit file "tests/c.rs"
    let staged_c =
      resolve_git_paths(root, true, false, vec![PathBuf::from("tests/c.rs")])
        .unwrap();
    assert_eq!(staged_c.len(), 1);
    assert_eq!(staged_c[0], file_c);

    // 4. Staged filtered by non-staged explicit path "src/b.rs"
    let staged_b =
      resolve_git_paths(root, true, false, vec![PathBuf::from("src/b.rs")])
        .unwrap();
    assert!(staged_b.is_empty());

    // 5. Changed (unstaged) without explicit paths returns modified unstaged file_b
    let changed_all = resolve_git_paths(root, false, true, vec![]).unwrap();
    assert_eq!(changed_all.len(), 1);
    assert_eq!(changed_all[0], file_b);
  }
}
