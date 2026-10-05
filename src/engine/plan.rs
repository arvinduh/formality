//! Pass plan dispatch, target surface resolution, and report coordination.
//!
//! Owns coordinating surface execution against a [`runner::Plan`]
//! across resolved target paths and reporting formatted summaries.

use std::path;

use colored::Colorize;

use crate::config;
use crate::engine::doctor;
use crate::engine::git;
use crate::engine::runner;
use crate::errors;
use crate::surfaces;
use crate::surfaces::LanguageSurface;

/// Dispatches a [`runner::Plan`] across target surfaces for the `fmt`, `lint`, and
/// `fix` commands after resolving git paths, target surfaces, and preflight
/// tool requirements. Provisioning missing tools is `fml doctor --install`'s
/// job now, not these commands' — see #282; this dispatch only warns about
/// stale tools, never installs.
#[must_use]
pub fn dispatch_plan(
  root: &path::Path,
  config: &config::FormalityConfig,
  staged: bool,
  changed: bool,
  lang: &[String],
  paths: Vec<path::PathBuf>,
  plan: &runner::Plan,
) -> errors::ExitStatus {
  dispatch_into(
    &mut std::io::stdout(),
    root,
    config,
    staged,
    changed,
    lang,
    paths,
    plan,
  )
}

/// Runs [`dispatch_plan`], rendering into `out`.
///
/// An empty `--staged`/`--changed` selection runs no surface: an empty path
/// list means the whole workspace to every layer below this one.
#[expect(
  clippy::too_many_arguments,
  reason = "dispatch_plan's arguments plus the report sink"
)]
pub fn dispatch_into(
  out: &mut dyn std::io::Write,
  root: &path::Path,
  config: &config::FormalityConfig,
  staged: bool,
  changed: bool,
  lang: &[String],
  paths: Vec<path::PathBuf>,
  plan: &runner::Plan,
) -> errors::ExitStatus {
  let scoped = !paths.is_empty();
  let target_paths = match git::resolve_git_paths(root, staged, changed, paths)
  {
    Ok(p) => p,
    Err(e) => {
      e.print_diagnostic();
      return errors::ExitStatus::Error;
    }
  };

  if (staged || changed) && target_paths.is_empty() {
    let flag = if staged { "staged" } else { "changed" };
    let under = if scoped { " under the given paths" } else { "" };
    let _ = writeln!(out, "{}", format!("No {flag} files{under}.").yellow());
    return errors::ExitStatus::Clean;
  }

  run_resolved(out, root, config, lang, &target_paths, plan)
}

/// Runs `plan` against already-resolved `paths`: resolves target surfaces,
/// warns about stale tools, then runs the passes and renders the report into
/// `out`.
///
/// `out` is stdout for the CLI and stderr for `fml lsp`, whose stdout carries
/// the JSON-RPC transport; one stray byte there breaks a strict client.
pub fn run_resolved(
  out: &mut dyn std::io::Write,
  root: &path::Path,
  config: &config::FormalityConfig,
  lang: &[String],
  paths: &[path::PathBuf],
  plan: &runner::Plan,
) -> errors::ExitStatus {
  let scope =
    runner::Scope::resolve(root, paths, &config.resolve_global().exclude);
  let surfaces = match resolve_target_surfaces(root, lang, &scope, config) {
    Ok(s) => s,
    Err(e) => {
      e.print_diagnostic();
      return errors::ExitStatus::Error;
    }
  };

  // Which tools to preflight follows directly from the plan's passes: a
  // plan that formats needs the formatters, a plan that lints needs the
  // linters, and `fix` needs both.
  let for_fmt = plan.includes(runner::Pass::Format);
  let for_lint = plan.includes(runner::Pass::Lint);

  doctor::preflight_warn_stale_tools(&surfaces, config, for_fmt, for_lint);

  runner::Runner::run_into(out, &surfaces, root, &scope, plan, config)
}

/// Resolves which language surfaces a command should act on: an explicit
/// `lang_filter` wins outright, otherwise surfaces are narrowed to those with
/// matching files under explicit paths, falling back to full smart detection
/// over the workspace scope's candidate files, so detection walks nothing
/// the runner does not already walk.
///
/// # Errors
///
/// Returns a [`errors::FormalityError`] if `lang_filter` names a surface that doesn't
/// exist.
pub fn resolve_target_surfaces(
  root: &path::Path,
  lang_filter: &[String],
  scope: &runner::Scope,
  config: &config::FormalityConfig,
) -> Result<Vec<Box<dyn LanguageSurface>>, errors::FormalityError> {
  if !lang_filter.is_empty() {
    let mut selected = Vec::new();
    for name in lang_filter {
      if let Some(s) = surfaces::get_surface_by_name(name) {
        selected.push(s);
      } else {
        return Err(errors::FormalityError::Surface(
          surfaces::Error::UnknownSurface(name.clone()),
        ));
      }
    }
    return Ok(selected);
  }

  match scope {
    runner::Scope::Paths { files, .. } => {
      Ok(surfaces_with_files_under(root, files, config))
    }
    runner::Scope::Workspace(candidates) => {
      let present = std::cell::LazyCell::new(|| {
        surfaces::glob::PresentExtensions::from_paths(candidates)
      });
      Ok(
        surfaces::default_registry().detect_surfaces_in(root, config, &present),
      )
    }
  }
}

/// Every surface with at least one of its files among the explicit paths'
/// expanded `files`.
fn surfaces_with_files_under(
  root: &path::Path,
  files: &[path::PathBuf],
  config: &config::FormalityConfig,
) -> Vec<Box<dyn LanguageSurface>> {
  let global = config.resolve_global();
  surfaces::all_surfaces()
    .into_iter()
    .filter(|surface| {
      let lang_cfg =
        config.resolve_for_lang_with_global(surface.name(), &global);
      let filter = surfaces::glob::FileFilter::new(
        root,
        surface.file_extensions(),
        &lang_cfg.exclude,
      );
      files.iter().any(|file| filter.matches(file))
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path;

  use super::*;

  #[test]
  fn test_staged_surface_discovery_ignores_ignored_files() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let init_ok = std::process::Command::new("git")
      .arg("init")
      .current_dir(root)
      .output()
      .is_ok_and(|o| o.status.success());
    if !init_ok {
      return;
    }

    let _ = std::process::Command::new("git")
      .args(["config", "user.name", "test"])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["config", "user.email", "test@example.com"])
      .current_dir(root)
      .output();

    let src = root.join("src");
    let fixtures = root.join("fixtures");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&fixtures).unwrap();

    let file_active = src.join("main.rs");
    let file_excluded = src.join("generated.rs");
    let file_fixture = fixtures.join("mock.rs");
    let file_ignored = root.join("ignored.rs");

    fs::write(&file_active, "fn main() {}\n").unwrap();
    fs::write(&file_excluded, "fn generated() {}\n").unwrap();
    fs::write(&file_fixture, "fn mock() {}\n").unwrap();
    fs::write(&file_ignored, "fn ignored() {}\n").unwrap();
    fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();

    // Initial commit so HEAD exists
    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["commit", "-m", "initial"])
      .current_dir(root)
      .output();

    // Modify files and stage them
    fs::write(&file_active, "fn main() { /* mod */ }\n").unwrap();
    fs::write(&file_excluded, "fn generated() { /* mod */ }\n").unwrap();
    fs::write(&file_fixture, "fn mock() { /* mod */ }\n").unwrap();
    fs::write(&file_ignored, "fn ignored() { /* mod */ }\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "src/main.rs", "src/generated.rs", "fixtures/mock.rs"])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["add", "-f", "ignored.rs"])
      .current_dir(root)
      .output();

    // Verify git reports all 4 files staged
    let staged_files =
      git::resolve_git_paths(root, true, false, vec![]).unwrap();
    assert_eq!(staged_files.len(), 4);

    // Formality config with surface exclusion for Rust
    let mut config = config::FormalityConfig::empty();
    let rust_lang = config::LangConfig {
      exclude: Some(vec![path::PathBuf::from("src/generated.rs")]),
      ..Default::default()
    };
    config.lang.insert("rust".to_string(), rust_lang);

    // Target surface discovery with staged paths
    let surfaces = resolve_target_surfaces(
      root,
      &[],
      &runner::Scope::resolve(root, &staged_files, &[]),
      &config,
    )
    .unwrap();
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].name(), "rust");

    // Discover staged files for the surface
    let global = config.resolve_global();
    let lang_cfg = config.resolve_for_lang_with_global("rust", &global);
    let resolved_files = surfaces::glob::find_files_with_ext(
      root,
      surfaces[0].file_extensions(),
      &staged_files,
      &lang_cfg.files,
      &lang_cfg.exclude,
    );

    // Per #214 / style guide §6: assert on the resolved file set directly
    assert_eq!(resolved_files, vec![file_active.clone()]);
    assert!(resolved_files.contains(&file_active));
    assert!(!resolved_files.contains(&file_excluded));
    assert!(!resolved_files.contains(&file_fixture));
    assert!(!resolved_files.contains(&file_ignored));
  }

  #[test]
  fn test_staged_file_discovery_respects_surface_exclusions() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let init_ok = std::process::Command::new("git")
      .arg("init")
      .current_dir(root)
      .output()
      .is_ok_and(|o| o.status.success());
    if !init_ok {
      return;
    }

    let _ = std::process::Command::new("git")
      .args(["config", "user.name", "test"])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["config", "user.email", "test@example.com"])
      .current_dir(root)
      .output();

    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    let file_active = src.join("main.rs");
    let file_excluded = src.join("ignored.rs");
    fs::write(&file_active, "fn main() {}\n").unwrap();
    fs::write(&file_excluded, "fn ignored() {}\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["commit", "-m", "initial"])
      .current_dir(root)
      .output();

    // Modify and stage both files
    fs::write(&file_active, "fn main() { /* mod */ }\n").unwrap();
    fs::write(&file_excluded, "fn ignored() { /* mod */ }\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();

    let staged_files =
      git::resolve_git_paths(root, true, false, vec![]).unwrap();
    assert_eq!(staged_files.len(), 2);

    let mut config = config::FormalityConfig::empty();
    let rust_lang = config::LangConfig {
      exclude: Some(vec![path::PathBuf::from("src/ignored.rs")]),
      ..Default::default()
    };
    config.lang.insert("rust".to_string(), rust_lang);

    let surfaces = resolve_target_surfaces(
      root,
      &[],
      &runner::Scope::resolve(root, &staged_files, &[]),
      &config,
    )
    .unwrap();
    let global = config.resolve_global();
    let lang_cfg = config.resolve_for_lang_with_global("rust", &global);
    let resolved_files = surfaces::glob::find_files_with_ext(
      root,
      surfaces[0].file_extensions(),
      &staged_files,
      &lang_cfg.files,
      &lang_cfg.exclude,
    );
    assert_eq!(resolved_files.len(), 1);
    assert_eq!(resolved_files[0], file_active);
  }

  #[test]
  fn test_explicit_directory_path_expands_candidates_once_across_surfaces() {
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "fn a() {}\n").unwrap();
    fs::write(src.join("README.md"), "# Title\n").unwrap();

    let mut out = Vec::new();
    let _ = run_resolved(
      &mut out,
      temp.path(),
      &config::FormalityConfig::empty(),
      &[],
      std::slice::from_ref(&src),
      &runner::Plan::fmt(true, true),
    );

    // Selection and both surfaces' runs share one expansion of `src`.
    let out = String::from_utf8_lossy(&out);
    assert!(out.contains("rust") && out.contains("markdown"), "{out}");
    assert_eq!(surfaces::glob::walk_count::of(&src), 1);
  }

  #[test]
  fn test_file_matched_only_by_global_exclude_does_not_activate_its_surface() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    fs::create_dir(root.join("gen")).unwrap();
    fs::write(root.join("gen/tool.py"), "x = 1\n").unwrap();
    let detected = |config: &config::FormalityConfig| -> Vec<&'static str> {
      let scope =
        runner::Scope::resolve(root, &[], &config.resolve_global().exclude);
      resolve_target_surfaces(root, &[], &scope, config)
        .unwrap()
        .iter()
        .map(|s| s.name())
        .collect()
    };

    assert_eq!(
      detected(&config::FormalityConfig::empty()),
      ["rust", "python"]
    );
    let excluding = config::FormalityConfig::parse_str(
      "[global]\nexclude = [\"gen\"]\n",
      path::Path::new("formality.toml"),
    )
    .unwrap();
    assert_eq!(detected(&excluding), ["rust"]);
  }

  #[test]
  fn test_root_marker_activates_its_surface_despite_global_exclude() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("pyproject.toml"), "[project]\n").unwrap();
    let config = config::FormalityConfig::parse_str(
      "[global]\nexclude = [\"pyproject.toml\"]\n",
      path::Path::new("formality.toml"),
    )
    .unwrap();

    // Marker files are checked at the root, outside the candidate list.
    let scope =
      runner::Scope::resolve(root, &[], &config.resolve_global().exclude);
    let surfaces = resolve_target_surfaces(root, &[], &scope, &config).unwrap();
    let names: Vec<&str> = surfaces.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["python"]);
  }
}
