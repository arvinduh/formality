//! Language surfaces interface and shared fleet infrastructure.
//!
//! Defines the `LanguageSurface` trait and shared registry/lookup machinery.
//! Concrete language implementations live in `lang`, while execution
//! orchestration is owned by `crate::engine`.

/// File discovery and path filtering shared by surfaces and the engine.
pub mod glob;
/// Language surface drivers for all supported ecosystems.
pub(crate) mod lang;
/// Surface registry and auto-detection engine.
pub mod registry;
/// Config file sync helpers.
pub mod sync;
/// Tool lookup, process spawning, and installer chains.
pub mod tooling;

use std::path;
use std::sync as std_sync;
use std::time;

use crate::config;

/// Execution context shared with every [`trait@LanguageSurface`] invocation for a
/// single `fml` command.
///
/// `root`, `paths`, `global_config`, and `candidate_files` are wrapped in [`std::sync::Arc`] because the
/// runner builds one `ExecutionContext` per surface and dispatches them in
/// parallel (`rayon::par_iter`), and all surfaces see the same values for
/// these fields. For `paths`, `global_config`, and `candidate_files` this avoids a real
/// per-surface cost: without `Arc`, every one of the (currently) 12 surfaces
/// would deep-clone the candidate path list and the global config
/// on every invocation, in place of a cheap refcount bump. `root` is wrapped
/// for consistency with those shared fields, not for a comparable
/// saving — it's one short `path::PathBuf`, so the copy avoided there is small.
/// `Arc<path::PathBuf>` (not `Arc<Path>`) matches the `Arc<Vec<path::PathBuf>>` /
/// `Arc<ResolvedGlobalConfig>` shape already used above: every field here is
/// `Arc` wrapping the type's natural owned form, not the `Arc<[T]>`/
/// `sync::Arc<str>`-style unsized-coercion pattern, so `root` follows the same
/// convention rather than special-casing to `sync::Arc<Path>`.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
  /// Target workspace root directory path.
  pub root: std_sync::Arc<path::PathBuf>,
  /// Target path arguments.
  pub paths: std_sync::Arc<Vec<path::PathBuf>>,
  /// Resolved global configuration settings.
  pub global_config: std_sync::Arc<config::ResolvedGlobalConfig>,
  /// Resolved per-language configuration settings for this surface.
  pub lang_config: config::ResolvedLangConfig,
  /// Whether to perform check-only mode without mutating files.
  pub check_only: bool,
  /// The run's candidate files, found once for every surface: the workspace
  /// walk when `paths` is empty, otherwise `paths` expanded.
  pub candidate_files: std_sync::Arc<Vec<path::PathBuf>>,
}

impl ExecutionContext {
  /// Discovers target files for the surface matching extensions, honoring scoped paths, files, and excludes.
  #[must_use]
  pub fn matched_files(&self, extensions: &[&str]) -> Vec<path::PathBuf> {
    if self.paths.is_empty() {
      let includes: Vec<String> = self
        .lang_config
        .files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
      glob::filter_candidates_with_ext(
        &self.candidate_files,
        extensions,
        &includes,
        &self.lang_config.exclude,
      )
    } else {
      // Explicit paths, already expanded: select from them without walking
      // a directory argument again.
      let filter = glob::FileFilter::new(
        self.root.as_path(),
        extensions,
        &self.lang_config.exclude,
      );
      self
        .candidate_files
        .iter()
        .filter(|file| filter.matches(file))
        .cloned()
        .collect()
    }
  }

  /// Returns the files to pass to a directory-walking CLI tool.
  /// If paths, `lang_config` files, or `lang_config` excludes are specified,
  /// returns the filtered files; otherwise returns an empty Vec so the tool
  /// can scan the whole directory.
  #[must_use]
  fn files_to_pass(&self, files: Vec<path::PathBuf>) -> Vec<path::PathBuf> {
    if !self.paths.is_empty()
      || !self.lang_config.files.is_empty()
      || !self.lang_config.exclude.is_empty()
    {
      files
    } else {
      Vec::new()
    }
  }
}

/// Returns a `Passed` result for surface `name` when `files` is empty, so a
/// surface with nothing to act on skips spawning its tool.
#[must_use]
fn passed_if_empty(
  files: &[path::PathBuf],
  name: &'static str,
  start: time::Instant,
) -> Option<SurfaceResult> {
  if files.is_empty() {
    Some(SurfaceResult::new(name, start, SurfaceStatus::Passed))
  } else {
    None
  }
}

/// Runs `surface`'s detection on `root` alone, scanning `root` for it.
#[cfg(test)]
fn detect_in(surface: &dyn LanguageSurface, root: &path::Path) -> bool {
  surface.detect(root, &glob::PresentExtensions::scan(root, &[]))
}

/// Builds a minimal `ExecutionContext` for testing language surfaces.
#[cfg(test)]
#[must_use]
fn test_ctx(
  root: impl AsRef<path::Path>,
  lang_config: config::ResolvedLangConfig,
) -> ExecutionContext {
  let root_ref = root.as_ref();
  ExecutionContext {
    root: std_sync::Arc::new(root_ref.to_path_buf()),
    paths: std_sync::Arc::new(Vec::new()),
    global_config: std_sync::Arc::new(config::ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: std_sync::Arc::new(glob::walk_candidate_files(
      root_ref,
      &[],
    )),
  }
}

/// Builds a minimal `ExecutionContext` scoped to explicit `paths` for testing.
#[cfg(test)]
#[must_use]
pub fn test_ctx_with_paths(
  root: impl AsRef<path::Path>,
  lang_config: config::ResolvedLangConfig,
  paths: Vec<path::PathBuf>,
) -> ExecutionContext {
  let root_ref = root.as_ref();
  let candidate_files = glob::expand_targets(root_ref, &paths);
  ExecutionContext {
    root: std_sync::Arc::new(root_ref.to_path_buf()),
    paths: std_sync::Arc::new(paths),
    global_config: std_sync::Arc::new(config::ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: std_sync::Arc::new(candidate_files),
  }
}

/// Metadata describing a binary executable tool required by a surface.
#[derive(Debug)]
pub struct ToolInfo {
  /// Executable binary name.
  pub binary: &'static str,
  /// Human-readable tool description.
  pub description: &'static str,
  /// Installation instructions override. `None` (the common case, and the
  /// default for every tool with a real install-preference chain) derives
  /// the hint from `binary`'s registered chain via
  /// [`Self::effective_install_hint`], so the printed text can never drift
  /// out of sync with the chain the way hand-written prose did (Fixes
  /// #264). `Some(..)` is reserved for two narrow cases, both of which
  /// must be exactly one named `const` referenced from every call site for
  /// that tool (never a repeated string literal — that is the exact #264
  /// drift shape, just moved one level up):
  /// - `binary` has no install chain at all (it ships inside a toolchain
  ///   rather than through a package manager — e.g. `cargo`, `gofmt`).
  /// - `binary` has a chain, but the chain has a real coverage gap a
  ///   package-manager command can't express (no entry at all for some
  ///   platform, or a manual-download fallback) — e.g.
  ///   `google-java-format`/`checkstyle`, whose chains have no Windows
  ///   entry. Reach for this only when the gap is real; a chain that
  ///   already covers every platform (e.g. `ktlint`'s) should stay `None`
  ///   even if its old hand-written hint said something extra.
  pub install_hint: Option<&'static str>,
  /// Whether this tool is required for formatting.
  pub is_required_for_fmt: bool,
  /// Whether this tool is required for linting.
  pub is_required_for_lint: bool,
}

impl ToolInfo {
  /// Returns the first available installer in this tool's preference chain.
  #[must_use]
  fn selected_install_method(&self) -> Option<tooling::InstallMethod> {
    tooling::selected_install_method_for(self.binary)
  }

  /// The install hint to actually print: `install_hint` when this tool
  /// declared an override, or the chain-derived text from
  /// [`tooling::install_hint_for`] otherwise. This is the one place that
  /// picks between the two, so every call site (`tool_missing_guard`'s
  /// `None` sites, `fml doctor`'s printed tables) reads the exact same
  /// text for the exact same tool.
  #[must_use]
  pub fn effective_install_hint(&self) -> String {
    self
      .install_hint
      .map_or_else(|| tooling::install_hint_for(self.binary), str::to_string)
  }

  /// Returns the (program, args) for the first available installer in this
  /// tool's preference chain: prebuilt binary package managers first,
  /// falling back to `cargo install ... --locked` source compilation where
  /// the tool ships as a crate.
  #[must_use]
  pub fn get_auto_install_cmd(&self) -> Option<(String, Vec<String>)> {
    self
      .selected_install_method()
      .map(|method| method.command())
  }
}

/// One native config file written by a surface's `fml sync` pass.
#[derive(Debug, Clone)]
pub struct SyncedConfigFile {
  /// Name of the config file, relative to the workspace root.
  pub file: String,
  /// Whether the file did not exist before this run.
  pub created: bool,
}

impl SyncedConfigFile {
  /// Builds a record for a file that was newly created (`created == true`)
  /// or overwritten in place (`created == false`).
  #[must_use]
  pub fn new(file: impl Into<String>, created: bool) -> Self {
    Self {
      file: file.into(),
      created,
    }
  }
}

/// Outcome status resulting from running a tool operation on a surface.
#[derive(Debug, Clone)]
pub enum SurfaceStatus {
  /// All checks passed cleanly with no violations.
  Passed,
  /// Tool completed but rule violations or formatting drift were found.
  ViolationsFound {
    /// Summary message of violations.
    message: String,
    /// Rendered diff string if available.
    diff: Option<String>,
  },
  /// Required tool binary was not found on system PATH.
  ToolMissing {
    /// Missing binary name.
    binary: String,
    /// Installation hint instruction.
    install_hint: String,
  },
  /// Tool execution failed with non-zero exit code or error output.
  ExecutionError {
    /// Error message detailing the failure.
    message: String,
  },
  /// Surface execution was skipped.
  Skipped {
    /// Reason string for skipping execution.
    reason: String,
  },
  /// Native tool configuration was updated or created in sync.
  ///
  /// Carries **every** file the surface wrote, not just the last one. A
  /// surface that syncs more than one native config (markdown writes
  /// `.markdownlint.json` as well as `.prettierrc.json`; cpp writes
  /// `.clang-format` as well as `.clang-tidy`) used to discard all but the
  /// final result, so a file could appear on disk having never been named in
  /// the output — #130. Fold per-file results together with
  /// [`sync::merge_sync_results`].
  ConfigSynced {
    /// Every native config file this surface created or updated, in the
    /// order it wrote them. Never empty.
    files: Vec<SyncedConfigFile>,
  },
  /// Native tool configuration is out of sync with canonical formality settings.
  ConfigDrifted {
    /// Config filename.
    file: String,
    /// Rendered diff showing configuration drift.
    diff: String,
  },
  /// Existing native config lacks the auto-generation header — it was written
  /// by hand. Overwriting silently would destroy intentional customization.
  ManualConfig {
    /// Config filename.
    file: String,
    /// User suggestion hint message.
    suggestion: String,
  },
}

/// How severe a [`SurfaceStatus`] is, ordered least to most severe.
///
/// The one encoding of severity: [`SurfaceResult::is_success`] and the
/// runner's tally, exit code and pass-merging precedence all derive from
/// [`SurfaceStatus::severity`] rather than restating which status is worse.
#[derive(Debug, PartialEq, PartialOrd)]
pub enum Severity {
  /// Nothing ran.
  Skipped,
  /// Ran clean, or wrote the config it was asked to.
  Passed,
  /// A required tool binary is not installed.
  ToolMissing,
  /// Rule violations, formatting drift or native-config drift.
  Violation,
  /// The tool itself failed.
  Error,
}

impl SurfaceStatus {
  /// Classifies this status by [`Severity`].
  ///
  /// The match is exhaustive with no wildcard arm, so a new status does not
  /// compile until it is classified here.
  #[must_use]
  pub fn severity(&self) -> Severity {
    match self {
      Self::Skipped { .. } => Severity::Skipped,
      Self::Passed | Self::ConfigSynced { .. } => Severity::Passed,
      Self::ToolMissing { .. } => Severity::ToolMissing,
      Self::ViolationsFound { .. }
      | Self::ConfigDrifted { .. }
      | Self::ManualConfig { .. } => Severity::Violation,
      Self::ExecutionError { .. } => Severity::Error,
    }
  }

  /// Every config file named by a [`SurfaceStatus::ConfigSynced`], in write
  /// order; empty for every other status.
  #[must_use]
  fn synced_files(&self) -> &[SyncedConfigFile] {
    match self {
      Self::ConfigSynced { files } => files,
      _ => &[],
    }
  }

  /// The names of the config files this status reports as *newly created*.
  #[cfg(test)]
  #[must_use]
  fn created_file_names(&self) -> Vec<&str> {
    self
      .synced_files()
      .iter()
      .filter(|f| f.created)
      .map(|f| f.file.as_str())
      .collect()
  }

  /// The names of every config file this status reports, created or updated.
  #[cfg(test)]
  #[must_use]
  fn synced_file_names(&self) -> Vec<&str> {
    self
      .synced_files()
      .iter()
      .map(|f| f.file.as_str())
      .collect()
  }
}

/// Result returned from a surface action (format, lint, sync).
#[derive(Debug)]
pub struct SurfaceResult {
  /// Name of the language surface.
  pub surface_name: &'static str,
  /// Status of the execution.
  pub status: SurfaceStatus,
  /// Execution duration.
  pub duration: time::Duration,
}

impl SurfaceResult {
  /// A result for `surface_name` with `status`, timed from `start`.
  #[must_use]
  pub fn new(
    surface_name: &'static str,
    start: time::Instant,
    status: SurfaceStatus,
  ) -> Self {
    Self {
      surface_name,
      status,
      duration: start.elapsed(),
    }
  }

  /// A [`SurfaceStatus::ExecutionError`] result carrying `message`.
  #[must_use]
  pub fn error(
    surface_name: &'static str,
    start: time::Instant,
    message: impl Into<String>,
  ) -> Self {
    Self::new(
      surface_name,
      start,
      SurfaceStatus::ExecutionError {
        message: message.into(),
      },
    )
  }

  /// Returns `true` if the status is a skip or a clean pass.
  #[must_use]
  pub fn is_success(&self) -> bool {
    matches!(self.status.severity(), Severity::Skipped | Severity::Passed)
  }
}

/// Core abstraction for language surface tools and configuration sync.
pub trait LanguageSurface:
  config::facets::DeclaresFacets + Send + Sync
{
  /// Canonical surface identifier name (e.g. `"rust"`, `"python"`).
  fn name(&self) -> &'static str;
  /// Alternative alias names recognized for this surface.
  fn aliases(&self) -> &[&'static str] {
    &[]
  }
  /// Supported file extensions for auto-matching.
  fn file_extensions(&self) -> &[&'static str] {
    &[]
  }
  /// Root-level filenames (manifests, tool configs) that mark this surface
  /// as active even when no source file exists yet.
  fn marker_files(&self) -> &[&'static str] {
    &[]
  }
  /// Detects whether this language surface is active in workspace `root`,
  /// given `present`, the extensions of `root`'s candidate files.
  ///
  /// The default is active when any `marker_files()` entry is a regular file
  /// directly under `root` (a directory of that name does not count), or
  /// when `present` holds one of `file_extensions()`. `present` comes from
  /// one walk shared by every surface, so an override must not walk again.
  fn detect(
    &self,
    root: &path::Path,
    present: &glob::PresentExtensions,
  ) -> bool {
    self.marker_files().iter().any(|m| root.join(m).is_file())
      || self.file_extensions().iter().any(|e| present.contains(e))
  }
  /// Returns information about required tools for this surface.
  fn tool_info(&self, config: &config::ResolvedLangConfig) -> Vec<ToolInfo>;
  /// Keys `[lang.<name>.extra_args]` accepts, one per tool invocation this
  /// surface makes. Each is the binary name `fml doctor` and the install
  /// chains use, or `<binary>-<subcommand>` where one binary runs two passes
  /// (python's `ruff-check`/`ruff-format`). Any other key is a config error.
  fn extra_args_tools(&self) -> &'static [&'static str] {
    &[]
  }
  /// Formats source files using underlying tools.
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult;
  /// Lints source files using underlying tools.
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult;
  /// Indicates whether this surface supports automatic lint fixing.
  fn supports_lint_fix(&self) -> bool {
    false
  }
  /// Synchronizes native tool configuration file.
  ///
  /// A surface must **not** sync `.prettierrc.json` here even if it formats
  /// via prettier — that file is shared by several surfaces and is written
  /// once by `sync::prettier::sync_shared_prettier_config`, outside the runner's
  /// parallel fan-out. Declare [`LanguageSurface::uses_prettier`] instead.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult;
  /// Whether this surface formats via `prettier` and therefore shares the
  /// single root `.prettierrc.json` with every other prettier surface.
  ///
  /// Declaring this — rather than each surface syncing the file itself — is
  /// what gives that file exactly one writer (#130). Three surfaces used to
  /// sync `.prettierrc.json` from their own `sync_config`, racing on one
  /// path under `surfaces.par_iter()`, which made the report
  /// nondeterministic and risked a sharing violation on Windows.
  fn uses_prettier(&self) -> bool {
    false
  }
  /// Clones the surface into a boxed trait object.
  fn clone_box(&self) -> Box<dyn LanguageSurface>;
}

impl Clone for Box<dyn LanguageSurface> {
  fn clone(&self) -> Self {
    self.clone_box()
  }
}

/// Errors related to language surfaces or native configuration rendering.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// Serialization of native surface configuration failed.
  #[error("Failed to serialize {surface} config: {message}")]
  SerializationFailed {
    /// Surface name.
    surface: String,
    /// Detailed failure message.
    message: String,
  },
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn surface_supports_lint_fix() {
    assert!(lang::rust::RustSurface.supports_lint_fix());
    assert!(lang::python::PythonSurface.supports_lint_fix());
    assert!(lang::cpp::CppSurface.supports_lint_fix());
    assert!(!lang::java::JavaSurface.supports_lint_fix());
    assert!(lang::go::GoSurface.supports_lint_fix());
    assert!(!lang::yaml::YamlSurface.supports_lint_fix());
    assert!(!lang::toml::TomlSurface.supports_lint_fix());
    assert!(lang::markdown::MarkdownSurface.supports_lint_fix());
    assert!(!lang::json::JsonSurface.supports_lint_fix());
    assert!(!lang::typst::TypstSurface.supports_lint_fix());
    assert!(lang::javascript::JavaScriptSurface.supports_lint_fix());
    assert!(lang::kotlin::KotlinSurface.supports_lint_fix());
  }

  #[test]
  fn default_detect_markers_are_root_regular_files_only() {
    let surface = lang::rust::RustSurface;
    let dir_marker = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(dir_marker.path().join("Cargo.toml")).unwrap();
    assert!(!detect_in(&surface, dir_marker.path()));

    let nested_marker = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(nested_marker.path().join("sub")).unwrap();
    std::fs::write(nested_marker.path().join("sub/Cargo.toml"), "").unwrap();
    assert!(!detect_in(&surface, nested_marker.path()));

    let root_marker = tempfile::TempDir::new().unwrap();
    std::fs::write(root_marker.path().join("Cargo.toml"), "").unwrap();
    assert!(detect_in(&surface, root_marker.path()));
  }

  #[test]
  fn default_detect_finds_nested_extension_outside_ignored_dirs() {
    let surface = lang::typst::TypstSurface;
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join("target/a")).unwrap();
    std::fs::write(temp.path().join("target/a/doc.typ"), "").unwrap();
    assert!(!detect_in(&surface, temp.path()));

    std::fs::create_dir_all(temp.path().join("docs/a")).unwrap();
    std::fs::write(temp.path().join("docs/a/doc.typ"), "").unwrap();
    assert!(detect_in(&surface, temp.path()));
  }

  #[test]
  fn is_success_holds_for_skips_and_clean_passes_only() {
    fn result_for(status: SurfaceStatus) -> SurfaceResult {
      SurfaceResult {
        surface_name: "test",
        status,
        duration: time::Duration::from_millis(0),
      }
    }

    let passed = result_for(SurfaceStatus::Passed);
    assert!(passed.is_success());

    let skipped = result_for(SurfaceStatus::Skipped {
      reason: "n/a".to_string(),
    });
    assert!(skipped.is_success());

    let synced = result_for(SurfaceStatus::ConfigSynced {
      files: vec![SyncedConfigFile::new("x", true)],
    });
    assert!(synced.is_success());

    let violations = result_for(SurfaceStatus::ViolationsFound {
      message: "bad".to_string(),
      diff: None,
    });
    assert!(!violations.is_success());

    let drifted = result_for(SurfaceStatus::ConfigDrifted {
      file: "x".to_string(),
      diff: "d".to_string(),
    });
    assert!(!drifted.is_success());

    let manual = result_for(SurfaceStatus::ManualConfig {
      file: "x".to_string(),
      suggestion: "s".to_string(),
    });
    assert!(!manual.is_success());

    let missing = result_for(SurfaceStatus::ToolMissing {
      binary: "x".to_string(),
      install_hint: "h".to_string(),
    });
    assert!(!missing.is_success());

    let exec_err = result_for(SurfaceStatus::ExecutionError {
      message: "boom".to_string(),
    });
    assert!(!exec_err.is_success());
  }

  #[test]
  fn box_dyn_language_surface_clone_preserves_identity() {
    let originals: Vec<Box<dyn LanguageSurface>> = vec![
      Box::new(lang::rust::RustSurface),
      Box::new(lang::python::PythonSurface),
      Box::new(lang::kotlin::KotlinSurface),
    ];

    for original in &originals {
      let cloned = original.clone();
      assert_eq!(cloned.name(), original.name());
      assert_eq!(cloned.file_extensions(), original.file_extensions());
    }
  }

  #[test]
  fn unsupported_lint_fix_returns_skipped() {
    let dummy_ctx = test_ctx(
      path::Path::new("."),
      config::ResolvedLangConfig::new("dummy"),
    );

    let unsupported_surfaces: Vec<Box<dyn LanguageSurface>> = vec![
      Box::new(lang::yaml::YamlSurface),
      Box::new(lang::toml::TomlSurface),
      Box::new(lang::json::JsonSurface),
      Box::new(lang::typst::TypstSurface),
      Box::new(lang::java::JavaSurface),
    ];

    for surface in unsupported_surfaces {
      let res = surface.lint(&dummy_ctx, true);
      match res.status {
        SurfaceStatus::Skipped { reason } => {
          assert_eq!(
            reason,
            "Tool does not support autofix; run fml fmt instead",
            "Mismatch for surface {}",
            surface.name()
          );
        }
        other => panic!(
          "Surface {} did not return Skipped on lint with fix=true: {:?}",
          surface.name(),
          other
        ),
      }
    }
  }
}
