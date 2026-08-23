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
use crate::errors::{FormalityError, GitError, SurfaceError};
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

/// Resolves the target file paths for a command from its `--staged`/
/// `--changed`/explicit-path flags. `staged` and `changed` are mutually
/// exclusive; if neither is set, `explicit_paths` is returned as-is.
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
  if staged {
    return get_git_staged_files(root);
  }
  if changed {
    return get_git_changed_files(root);
  }
  Ok(explicit_paths)
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
    for surface in all_surfaces() {
      let lang_cfg = config.resolve_for_lang(surface.name());
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
