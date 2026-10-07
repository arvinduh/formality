//! Target resolution for paths, scopes, and language surfaces.
//!
//! Owns discovering staged or modified uncommitted files via the `git` CLI,
//! path normalization, candidate file scope resolution, and language surface
//! filtering.

use std::io;
use std::path;

use thiserror;

use crate::config;
use crate::engine::runner;
use crate::surfaces;
use crate::surfaces::registry;

/// Why targets could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// `git` could not be started.
  #[error("could not run git: {0}")]
  GitSpawn(#[from] io::Error),
  /// `git diff` exited non-zero while listing the selected files.
  #[error("git could not list {0} files")]
  Git(Changes),
  /// `--lang` named something no surface answers to.
  #[error(
    "unknown language surface '{0}'; run 'fml doctor --all' to list them"
  )]
  UnknownSurface(String),
}

/// Which uncommitted files a run is limited to.
#[derive(Debug, Clone, Copy)]
pub enum Changes {
  /// Files staged for commit.
  Staged,
  /// Modified files, staged or not.
  Changed,
}

impl std::fmt::Display for Changes {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(match self {
      Self::Staged => "staged",
      Self::Changed => "changed",
    })
  }
}

/// Normalizes path components by resolving `.` and `..` segments lexically.
#[must_use]
fn normalize_path(path: &path::Path) -> path::PathBuf {
  let mut components = Vec::new();
  for component in path.components() {
    match component {
      path::Component::CurDir => {}
      path::Component::ParentDir => {
        if let Some(path::Component::Normal(_)) = components.last() {
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
/// Returns an [`Error`] if both `staged` and `changed` are set, or if
/// the underlying git query fails.
fn resolve_git_paths(
  root: &path::Path,
  changes: Option<Changes>,
  explicit_paths: Vec<path::PathBuf>,
) -> Result<Vec<path::PathBuf>, Error> {
  let Some(changes) = changes else {
    return Ok(explicit_paths);
  };
  let git_files = git_diff_files(root, changes)?;

  if explicit_paths.is_empty() {
    return Ok(git_files);
  }

  let norm_root = normalize_path(root);
  let normalized_explicit: Vec<(path::PathBuf, path::PathBuf)> = explicit_paths
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

/// Lists the files `git diff` reports for `changes`, as paths under `root`.
fn git_diff_files(
  root: &path::Path,
  changes: Changes,
) -> Result<Vec<path::PathBuf>, Error> {
  let mut cmd = std::process::Command::new("git");
  cmd.args(["diff", "--name-only", "--diff-filter=ACMR"]);
  if let Changes::Staged = changes {
    cmd.arg("--cached");
  }
  let output = cmd.current_dir(root).output()?;
  if !output.status.success() {
    return Err(Error::Git(changes));
  }
  Ok(
    String::from_utf8_lossy(&output.stdout)
      .lines()
      .map(|l| root.join(l.trim()))
      .filter(|p| p.is_file())
      .collect(),
  )
}

/// The target files and matching surfaces resolved for a command run.
pub struct ResolvedTarget {
  /// The scope of files to act on.
  pub scope: runner::Scope,
  /// The language surfaces that match the target scope.
  pub surfaces: Vec<Box<dyn surfaces::LanguageSurface>>,
}

/// Resolves target files and matching surfaces based on git filters and path arguments.
///
/// Returns `Ok(None)` if `--staged` or `--changed` was requested but no files were found.
///
/// # Errors
///
/// Returns an [`Error`] if `--staged` and `--changed` are both set,
/// if the git command fails, or if a requested language surface does not exist.
pub fn resolve_targets(
  root: &path::Path,
  changes: Option<Changes>,
  paths: Vec<path::PathBuf>,
  lang_filter: &[String],
  config: &config::FormalityConfig,
) -> Result<Option<ResolvedTarget>, Error> {
  let target_paths = resolve_git_paths(root, changes, paths)?;

  if changes.is_some() && target_paths.is_empty() {
    return Ok(None);
  }

  let scope = runner::Scope::resolve(
    root,
    &target_paths,
    &config.resolve_global().exclude,
  );
  let surfaces = resolve_target_surfaces(root, lang_filter, &scope, config)?;

  Ok(Some(ResolvedTarget { scope, surfaces }))
}

/// Resolves surfaces across the full workspace without git filtering.
///
/// # Errors
///
/// Returns an [`Error`] if a requested language surface does not exist.
pub fn resolve_workspace_targets(
  root: &path::Path,
  lang_filter: &[String],
  config: &config::FormalityConfig,
) -> Result<ResolvedTarget, Error> {
  let scope =
    runner::Scope::resolve(root, &[], &config.resolve_global().exclude);
  let surfaces = resolve_target_surfaces(root, lang_filter, &scope, config)?;
  Ok(ResolvedTarget { scope, surfaces })
}

/// Resolves which language surfaces a command should act on: an explicit
/// `lang_filter` wins outright, otherwise surfaces are narrowed to those with
/// matching files under explicit paths, falling back to full smart detection
/// over the workspace scope's candidate files.
///
/// # Errors
///
/// Returns an [`Error`] if `lang_filter` names a surface that doesn't
/// exist.
pub fn resolve_target_surfaces(
  root: &path::Path,
  lang_filter: &[String],
  scope: &runner::Scope,
  config: &config::FormalityConfig,
) -> Result<Vec<Box<dyn surfaces::LanguageSurface>>, Error> {
  if !lang_filter.is_empty() {
    let mut selected = Vec::new();
    for name in lang_filter {
      if let Some(s) = registry::get_surface_by_name(name) {
        selected.push(s);
      } else {
        return Err(Error::UnknownSurface(name.clone()));
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
        registry::default_registry().detect_surfaces_in(root, config, &present),
      )
    }
  }
}

/// Every surface with at least one of its files among the explicit paths'
/// expanded `files`.
#[must_use]
fn surfaces_with_files_under(
  root: &path::Path,
  files: &[path::PathBuf],
  config: &config::FormalityConfig,
) -> Vec<Box<dyn surfaces::LanguageSurface>> {
  let global = config.resolve_global();
  registry::all_surfaces()
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
  fn normalize_path_components() {
    let p = path::Path::new("a/b/../c/./d");
    let norm = normalize_path(p);
    assert_eq!(norm, path::PathBuf::from("a/c/d"));

    let p2 = path::Path::new("./a/b");
    let norm2 = normalize_path(p2);
    assert_eq!(norm2, path::PathBuf::from("a/b"));
  }

  #[test]
  fn resolve_git_paths_no_git_flags_returns_explicit() {
    let explicit = vec![
      path::PathBuf::from("src/main.rs"),
      path::PathBuf::from("README.md"),
    ];
    let res =
      resolve_git_paths(path::Path::new("."), None, explicit.clone()).unwrap();
    assert_eq!(res, explicit);
  }

  #[test]
  fn resolve_git_paths_staged_filtering() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    // Initialize git repository
    let init_ok = std::process::Command::new("git")
      .arg("init")
      .current_dir(root)
      .output()
      .is_ok_and(|o| o.status.success());
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
    let staged_all =
      resolve_git_paths(root, Some(Changes::Staged), vec![]).unwrap();
    assert_eq!(staged_all.len(), 2);
    assert!(staged_all.contains(&file_a));
    assert!(staged_all.contains(&file_c));
    assert!(!staged_all.contains(&file_b));

    // 2. Staged filtered by explicit directory "src"
    let staged_src = resolve_git_paths(
      root,
      Some(Changes::Staged),
      vec![path::PathBuf::from("src")],
    )
    .unwrap();
    assert_eq!(staged_src, vec![file_a]);

    // 3. Staged filtered by explicit file "tests/c.rs"
    let staged_file = resolve_git_paths(
      root,
      Some(Changes::Staged),
      vec![path::PathBuf::from("tests/c.rs")],
    )
    .unwrap();
    assert_eq!(staged_file, vec![file_c]);

    // 4. Staged filtered by non-matching explicit path returns empty
    let staged_none = resolve_git_paths(
      root,
      Some(Changes::Staged),
      vec![path::PathBuf::from("nonexistent")],
    )
    .unwrap();
    assert!(staged_none.is_empty());
  }

  #[test]
  fn resolve_git_paths_changed_filtering() {
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

    let file_tracked = root.join("tracked.txt");
    fs::write(&file_tracked, "v1\n").unwrap();

    let _ = std::process::Command::new("git")
      .args(["add", "."])
      .current_dir(root)
      .output();
    let _ = std::process::Command::new("git")
      .args(["commit", "-m", "initial"])
      .current_dir(root)
      .output();

    // Modify tracked file (unstaged)
    fs::write(&file_tracked, "v2\n").unwrap();

    let changed =
      resolve_git_paths(root, Some(Changes::Changed), vec![]).unwrap();
    assert_eq!(changed, vec![file_tracked.clone()]);

    let changed_filtered = resolve_git_paths(
      root,
      Some(Changes::Changed),
      vec![path::PathBuf::from("tracked.txt")],
    )
    .unwrap();
    assert_eq!(changed_filtered, vec![file_tracked]);

    let changed_unmatched = resolve_git_paths(
      root,
      Some(Changes::Changed),
      vec![path::PathBuf::from("other.txt")],
    )
    .unwrap();
    assert!(changed_unmatched.is_empty());
  }

  #[test]
  fn staged_surface_discovery_ignores_ignored_files() {
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
      resolve_git_paths(root, Some(Changes::Staged), vec![]).unwrap();
    assert_eq!(staged_files.len(), 4);

    // Formality config with surface exclusion for Rust
    let mut config = config::FormalityConfig::default();
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
    let resolved_files =
      surfaces::test_ctx_with_paths(root, lang_cfg, staged_files)
        .matched_files(surfaces[0].file_extensions());

    // Per #214 / style guide §6: assert on the resolved file set directly
    assert_eq!(resolved_files, vec![file_active.clone()]);
    assert!(resolved_files.contains(&file_active));
    assert!(!resolved_files.contains(&file_excluded));
    assert!(!resolved_files.contains(&file_fixture));
    assert!(!resolved_files.contains(&file_ignored));
  }

  #[test]
  fn staged_file_discovery_respects_surface_exclusions() {
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
      resolve_git_paths(root, Some(Changes::Staged), vec![]).unwrap();
    assert_eq!(staged_files.len(), 2);

    let mut config = config::FormalityConfig::default();
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
    let resolved_files =
      surfaces::test_ctx_with_paths(root, lang_cfg, staged_files)
        .matched_files(surfaces[0].file_extensions());
    assert_eq!(resolved_files.len(), 1);
    assert_eq!(resolved_files[0], file_active);
  }

  #[test]
  fn explicit_directory_path_expands_candidates_once_across_surfaces() {
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "fn a() {}\n").unwrap();
    fs::write(src.join("README.md"), "# Title\n").unwrap();

    let target = resolve_targets(
      temp.path(),
      None,
      vec![src.clone()],
      &[],
      &config::FormalityConfig::default(),
    )
    .unwrap()
    .unwrap();
    let plan = runner::Plan::fmt(true, true);
    let results = runner::Runner::run(
      &target.surfaces,
      temp.path(),
      &target.scope,
      &plan,
      &config::FormalityConfig::default(),
    );

    // Selection and both surfaces' runs share one expansion of `src`.
    let names: Vec<&str> = results.iter().map(|r| r.surface_name).collect();
    assert!(
      names.contains(&"rust") && names.contains(&"markdown"),
      "{names:?}"
    );
    assert_eq!(surfaces::glob::walk_count::of(&src), 1);
  }

  #[test]
  fn file_matched_only_by_global_exclude_does_not_activate_its_surface() {
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
      detected(&config::FormalityConfig::default()),
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
  fn root_marker_activates_its_surface_despite_global_exclude() {
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
