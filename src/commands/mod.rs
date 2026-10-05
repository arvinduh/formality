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
/// Language Server Protocol server: document formatting and lint diagnostics.
pub mod lsp;
/// Structured per-violation lint diagnostics for `fml lsp` (Fixes #159 [pre-recreation]).
pub mod lsp_diagnostics;
/// JSON Schema generator CLI command handler.
pub mod schema;
/// Native configuration synchronization CLI command handler.
pub mod sync;

use std::path;

use colored::Colorize;

use crate::config;
use crate::engine;
use crate::errors;
use crate::surfaces;
use crate::surfaces::LanguageSurface;

/// Dispatches a [`Plan`] across target surfaces for the `fmt`, `lint`, and
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
  plan: &engine::Plan,
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
fn dispatch_into(
  out: &mut dyn std::io::Write,
  root: &path::Path,
  config: &config::FormalityConfig,
  staged: bool,
  changed: bool,
  lang: &[String],
  paths: Vec<path::PathBuf>,
  plan: &engine::Plan,
) -> errors::ExitStatus {
  let scoped = !paths.is_empty();
  let target_paths = match resolve_git_paths(root, staged, changed, paths) {
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
fn run_resolved(
  out: &mut dyn std::io::Write,
  root: &path::Path,
  config: &config::FormalityConfig,
  lang: &[String],
  paths: &[path::PathBuf],
  plan: &engine::Plan,
) -> errors::ExitStatus {
  let scope =
    engine::Scope::resolve(root, paths, &config.resolve_global().exclude);
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
  let for_fmt = plan.includes(engine::Pass::Format);
  let for_lint = plan.includes(engine::Pass::Lint);

  doctor::preflight_warn_stale_tools(&surfaces, config, for_fmt, for_lint);

  engine::Runner::run_into(out, &surfaces, root, &scope, plan, config)
}

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
/// Returns a [`FormalityError`] if both `staged` and `changed` are set, or if
/// the underlying git query fails.
pub fn resolve_git_paths(
  root: &path::Path,
  staged: bool,
  changed: bool,
  explicit_paths: Vec<path::PathBuf>,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  if staged && changed {
    return Err(errors::FormalityError::Git(
      errors::GitError::MutuallyExclusiveFlags,
    ));
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

fn get_git_diff_files(
  root: &path::Path,
  staged: bool,
  error_context: &str,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  let mut cmd = std::process::Command::new("git");
  cmd.arg("diff").arg("--name-only");
  if staged {
    cmd.arg("--cached");
  }
  cmd.arg("--diff-filter=ACMR").current_dir(root);

  let output = cmd.output().map_err(|e| {
    errors::FormalityError::Git(errors::GitError::ExecutionFailed(
      e.to_string(),
    ))
  })?;

  if !output.status.success() {
    return Err(errors::FormalityError::Git(
      errors::GitError::CommandFailed(format!(
        "Failed to query git {error_context} files."
      )),
    ));
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let files: Vec<path::PathBuf> = stdout
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
  root: &path::Path,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  get_git_diff_files(root, true, "staged")
}

/// Returns the list of changed git files relative to `root`.
///
/// # Errors
///
/// Returns a [`FormalityError`] if git execution fails or the git command cannot be run.
pub fn get_git_changed_files(
  root: &path::Path,
) -> Result<Vec<path::PathBuf>, errors::FormalityError> {
  get_git_diff_files(root, false, "changed")
}

/// Resolves which language surfaces a command should act on: an explicit
/// `lang_filter` wins outright, otherwise surfaces are narrowed to those with
/// matching files under explicit paths, falling back to full smart detection
/// over the workspace scope's candidate files, so detection walks nothing
/// the runner does not already walk.
///
/// # Errors
///
/// Returns a [`FormalityError`] if `lang_filter` names a surface that doesn't
/// exist.
pub fn resolve_target_surfaces(
  root: &path::Path,
  lang_filter: &[String],
  scope: &engine::Scope,
  config: &config::FormalityConfig,
) -> Result<Vec<Box<dyn LanguageSurface>>, errors::FormalityError> {
  if !lang_filter.is_empty() {
    let mut selected = Vec::new();
    for name in lang_filter {
      if let Some(s) = surfaces::get_surface_by_name(name) {
        selected.push(s);
      } else {
        return Err(errors::FormalityError::Surface(
          errors::SurfaceError::UnknownSurface(name.clone()),
        ));
      }
    }
    return Ok(selected);
  }

  match scope {
    engine::Scope::Paths { files, .. } => {
      Ok(surfaces_with_files_under(root, files, config))
    }
    engine::Scope::Workspace(candidates) => {
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
  use super::*;
  use std::fs;

  #[test]
  fn test_normalize_path_components() {
    let p = path::Path::new("a/b/../c/./d");
    let norm = normalize_path(p);
    assert_eq!(norm, path::PathBuf::from("a/c/d"));

    let p2 = path::Path::new("./a/b");
    let norm2 = normalize_path(p2);
    assert_eq!(norm2, path::PathBuf::from("a/b"));
  }

  #[test]
  fn test_resolve_git_paths_mutual_exclusion() {
    let res = resolve_git_paths(path::Path::new("."), true, true, vec![]);
    assert!(matches!(
      res,
      Err(errors::FormalityError::Git(
        errors::GitError::MutuallyExclusiveFlags
      ))
    ));
  }

  #[test]
  fn test_resolve_git_paths_no_git_flags_returns_explicit() {
    let explicit = vec![
      path::PathBuf::from("src/main.rs"),
      path::PathBuf::from("README.md"),
    ];
    let res =
      resolve_git_paths(path::Path::new("."), false, false, explicit.clone())
        .unwrap();
    assert_eq!(res, explicit);
  }

  #[test]
  fn test_resolve_git_paths_staged_filtering() {
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
    let staged_all = resolve_git_paths(root, true, false, vec![]).unwrap();
    assert_eq!(staged_all.len(), 2);
    assert!(staged_all.contains(&file_a));
    assert!(staged_all.contains(&file_c));
    assert!(!staged_all.contains(&file_b));

    // 2. Staged filtered by explicit directory "src"
    let staged_src =
      resolve_git_paths(root, true, false, vec![path::PathBuf::from("src")])
        .unwrap();
    assert_eq!(staged_src.len(), 1);
    assert_eq!(staged_src[0], file_a);

    // 3. Staged filtered by explicit file "tests/c.rs"
    let staged_c = resolve_git_paths(
      root,
      true,
      false,
      vec![path::PathBuf::from("tests/c.rs")],
    )
    .unwrap();
    assert_eq!(staged_c.len(), 1);
    assert_eq!(staged_c[0], file_c);

    // 4. Staged filtered by non-staged explicit path "src/b.rs"
    let staged_b = resolve_git_paths(
      root,
      true,
      false,
      vec![path::PathBuf::from("src/b.rs")],
    )
    .unwrap();
    assert!(staged_b.is_empty());

    // 5. Changed (unstaged) without explicit paths returns modified unstaged file_b
    let changed_all = resolve_git_paths(root, false, true, vec![]).unwrap();
    assert_eq!(changed_all.len(), 1);
    assert_eq!(changed_all[0], file_b);
  }

  #[test]
  fn test_staged_file_discovery_respects_surface_exclusions() {
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
    let staged_files = resolve_git_paths(root, true, false, vec![]).unwrap();
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
      &engine::Scope::resolve(root, &staged_files, &[]),
      &config,
    )
    .unwrap();
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].name(), "rust");

    // Discover staged files for the surface (Execution context matched_files)
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
  fn test_empty_staged_selection_runs_no_surface() {
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
    fs::write(root.join("bad.py"), "x  =  1\n").unwrap();

    let mut out = Vec::new();
    let config = config::FormalityConfig::with_defaults();
    let plan = engine::Plan::fmt(true, true);
    let status =
      dispatch_into(&mut out, root, &config, true, false, &[], vec![], &plan);

    let out = String::from_utf8_lossy(&out);
    assert!(out.contains("No staged files."), "{out}");
    assert!(!out.contains("python"), "{out}");
    assert!(status.is_clean());

    let mut out = Vec::new();
    let paths = vec![path::PathBuf::from("bad.py")];
    let _ =
      dispatch_into(&mut out, root, &config, true, false, &[], paths, &plan);
    let out = String::from_utf8_lossy(&out);
    assert!(
      out.contains("No staged files under the given paths."),
      "{out}"
    );
  }

  #[test]
  fn test_default_run_walks_the_workspace_once() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();

    let mut out = Vec::new();
    let _ = run_resolved(
      &mut out,
      root,
      &config::FormalityConfig::with_defaults(),
      &[],
      &[],
      &engine::Plan::fmt(true, true),
    );

    // Detection found rust and the runner formatted it, both from one walk.
    assert!(String::from_utf8_lossy(&out).contains("rust"));
    assert_eq!(surfaces::glob::walk_count::of(root), 1);
  }

  #[test]
  fn test_explicit_directory_is_walked_once_for_all_surfaces() {
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(src.join("notes.md"), "# Notes\n").unwrap();

    let scope =
      engine::Scope::resolve(temp.path(), std::slice::from_ref(&src), &[]);
    let surfaces = resolve_target_surfaces(
      temp.path(),
      &[],
      &scope,
      &config::FormalityConfig::empty(),
    )
    .unwrap();

    let names: Vec<&str> = surfaces.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["rust", "markdown"]);
    assert_eq!(surfaces::glob::walk_count::of(&src), 1);
  }

  #[test]
  fn test_explicit_directory_run_walks_it_once() {
    let temp = tempfile::TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(src.join("notes.md"), "# Notes\n").unwrap();

    let mut out = Vec::new();
    let _ = run_resolved(
      &mut out,
      temp.path(),
      &config::FormalityConfig::empty(),
      &[],
      std::slice::from_ref(&src),
      &engine::Plan::fmt(true, true),
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
        engine::Scope::resolve(root, &[], &config.resolve_global().exclude);
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
      engine::Scope::resolve(root, &[], &config.resolve_global().exclude);
    let surfaces = resolve_target_surfaces(root, &[], &scope, &config).unwrap();
    let names: Vec<&str> = surfaces.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["python"]);
  }
}
