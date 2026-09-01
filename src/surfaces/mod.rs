//! Language surfaces: the `LanguageSurface` trait, the fleet of per-language
//! implementations, and the shared machinery (registry, glob matching, tool
//! discovery, config sync) they're all built on.

/// C/C++ language surface implementation.
pub mod cpp;
/// .editorconfig generation and synchronization.
pub mod editorconfig;
/// Glob matching and file path resolution helpers.
pub mod glob;
/// Go language surface implementation.
pub mod go;
/// Java language surface implementation.
pub mod java;
/// JavaScript/TypeScript language surface implementation.
pub mod javascript;
/// JSON language surface implementation.
pub mod json;
/// Kotlin language surface implementation.
pub mod kotlin;
/// Markdown language surface implementation.
pub mod markdown;
/// Native configuration generator and serializer.
pub mod native;
/// Prettier configuration generator and inline argument helpers.
pub mod prettier;
/// Python language surface implementation.
pub mod python;
/// Surface registry and auto-detection engine.
pub mod registry;
/// Rust language surface implementation.
pub mod rust;
/// Config file sync helpers.
pub mod sync;
/// TOML language surface implementation.
pub mod toml;
/// Tool execution and command creation utilities.
pub mod tooling;
/// Typst language surface implementation.
pub mod typst;
/// YAML language surface implementation.
pub mod yaml;

pub use native::{
  AUTO_GENERATED_HEADER, AUTO_GENERATED_JSON_COMMENT, EDITORCONFIG_FILE_NAME,
  NativeConfig, generate_editorconfig, generate_editorconfig_from_config,
  render_native_config, serialize_json_pretty, serialize_toml_with_header,
  serialize_yaml_with_header, sync_editorconfig, sync_native_config,
};
pub use prettier::{
  PrettierConfig, build_prettier_inline_args, sync_prettier_config,
};

pub use crate::config::facets::{DeclaresFacets, Facet, FacetSupport};
use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use glob::{
  filter_candidates_with_ext, filter_files_for_surface, find_files_with_ext,
  is_excluded, matches_pattern, simple_glob_match, walk_candidate_files,
};
pub use registry::{
  SurfaceRegistry, all_surfaces, default_registry, detect_surfaces,
  detect_surfaces_smart, get_surface_by_name, resolve_canonical_name,
};
pub use sync::{
  diff_check_via_tempcopy, diff_check_via_tempcopy_classified,
  is_auto_generated, sync_file_helper,
};
pub use tooling::{
  ExitClass, InstallMethod, chain_wants_cargo_binstall, check_binary_exists,
  classify_all_nonzero_as_error, classify_exit_one_as_violation,
  create_tool_command, ensure_cargo_binstall, forget_binary,
  has_cargo_binstall, install_chain_for, lint_fix_unsupported,
  pinned_installer_for, pinned_version_for, refresh_go_install_path,
  refresh_path_after_install, refresh_windows_path_from_registry,
  resolve_binary_path, run_tool_command, run_tool_command_classified,
  selected_install_method_for, selected_pinned_version_for, tool_missing_guard,
  tool_missing_result, tool_would_benefit_from_cargo_binstall_bootstrap,
};

/// Execution context shared with every [`LanguageSurface`] invocation for a
/// single `fml` command.
///
/// `root`, `paths`, `global_config`, and `candidate_files` are wrapped in [`Arc`] because the
/// runner builds one `ExecutionContext` per surface and dispatches them in
/// parallel (`rayon::par_iter`), and all surfaces see the same values for
/// these fields. For `paths`, `global_config`, and `candidate_files` this avoids a real
/// per-surface cost: without `Arc`, every one of the (currently) 12 surfaces
/// would deep-clone the candidate path list and the global config
/// on every invocation, in place of a cheap refcount bump. `root` is wrapped
/// for consistency with those shared fields, not for a comparable
/// saving — it's one short `PathBuf`, so the copy avoided there is small.
/// `Arc<PathBuf>` (not `Arc<Path>`) matches the `Arc<Vec<PathBuf>>` /
/// `Arc<ResolvedGlobalConfig>` shape already used above: every field here is
/// `Arc` wrapping the type's natural owned form, not the `Arc<[T]>`/
/// `Arc<str>`-style unsized-coercion pattern, so `root` follows the same
/// convention rather than special-casing to `Arc<Path>`.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
  /// Target workspace root directory path.
  pub root: Arc<PathBuf>,
  /// Target path arguments.
  pub paths: Arc<Vec<PathBuf>>,
  /// Resolved global configuration settings.
  pub global_config: Arc<ResolvedGlobalConfig>,
  /// Resolved per-language configuration settings for this surface.
  pub lang_config: ResolvedLangConfig,
  /// Whether to perform check-only mode without mutating files.
  pub check_only: bool,
  /// Pre-discovered candidate files for the workspace, if single-walk was performed.
  pub candidate_files: Option<Arc<Vec<PathBuf>>>,
}

impl ExecutionContext {
  /// Returns whether explicit file or directory paths were targeted.
  #[must_use]
  pub fn is_scoped(&self) -> bool {
    !self.paths.is_empty()
  }

  /// Discovers target files for the surface matching extensions, honoring scoped paths, files, and excludes.
  #[must_use]
  pub fn matched_files(&self, extensions: &[&str]) -> Vec<PathBuf> {
    if !self.paths.is_empty() {
      find_files_with_ext(
        self.root.as_path(),
        extensions,
        &self.paths,
        &self.lang_config.files,
        &self.lang_config.exclude,
      )
    } else if let Some(ref candidates) = self.candidate_files {
      let includes: Vec<String> = self
        .lang_config
        .files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
      filter_candidates_with_ext(
        candidates,
        extensions,
        &includes,
        &self.lang_config.exclude,
      )
    } else {
      find_files_with_ext(
        self.root.as_path(),
        extensions,
        &self.paths,
        &self.lang_config.files,
        &self.lang_config.exclude,
      )
    }
  }

  /// Returns `Some(SurfaceResult)` with `SurfaceStatus::Passed` if `files` is empty, or `None` otherwise.
  #[must_use]
  pub fn early_out_if_empty(
    &self,
    files: &[PathBuf],
    name: &'static str,
    start: Instant,
  ) -> Option<SurfaceResult> {
    if files.is_empty() {
      Some(SurfaceResult {
        surface_name: name,
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      })
    } else {
      None
    }
  }

  /// Returns the files to pass to a directory-walking CLI tool.
  /// If paths, lang_config files, or lang_config excludes are specified, returns the filtered files;
  /// otherwise returns an empty Vec so the tool can scan the whole directory.
  #[must_use]
  pub fn files_to_pass(&self, files: Vec<PathBuf>) -> Vec<PathBuf> {
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

/// Builds a minimal `ExecutionContext` for testing language surfaces.
#[must_use]
pub fn test_ctx(
  root: impl AsRef<Path>,
  lang_config: ResolvedLangConfig,
) -> ExecutionContext {
  ExecutionContext {
    root: Arc::new(root.as_ref().to_path_buf()),
    paths: Arc::new(Vec::new()),
    global_config: Arc::new(ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: None,
  }
}

/// Metadata describing a binary executable tool required by a surface.
#[derive(Debug, Clone)]
pub struct ToolInfo {
  /// Executable binary name.
  pub binary: &'static str,
  /// Human-readable tool description.
  pub description: &'static str,
  /// Installation instructions hint.
  pub install_hint: &'static str,
  /// Whether this tool is required for formatting.
  pub is_required_for_fmt: bool,
  /// Whether this tool is required for linting.
  pub is_required_for_lint: bool,
}

impl ToolInfo {
  /// Returns the first available installer in this tool's preference chain.
  #[must_use]
  pub fn selected_install_method(&self) -> Option<InstallMethod> {
    tooling::selected_install_method_for(self.binary)
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
  ConfigSynced {
    /// Synced configuration filename.
    file: String,
    /// Whether the file was newly created.
    created: bool,
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

/// Result returned from a surface action (format, lint, sync).
#[derive(Debug, Clone)]
pub struct SurfaceResult {
  /// Name of the language surface.
  pub surface_name: &'static str,
  /// Status of the execution.
  pub status: SurfaceStatus,
  /// Execution duration.
  pub duration: Duration,
}

impl SurfaceResult {
  /// Returns `true` if the status represents a clean success or skipped operation.
  #[must_use]
  pub fn is_success(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::Passed
        | SurfaceStatus::Skipped { .. }
        | SurfaceStatus::ConfigSynced { .. }
    )
  }

  /// Returns `true` if the status represents a formatting or lint violation.
  #[must_use]
  pub fn is_violation(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::ViolationsFound { .. }
        | SurfaceStatus::ConfigDrifted { .. }
        | SurfaceStatus::ManualConfig { .. }
    )
  }

  /// Returns `true` if the status represents an execution error or missing tool.
  #[must_use]
  pub fn is_error(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::ToolMissing { .. } | SurfaceStatus::ExecutionError { .. }
    )
  }
}

/// Core abstraction for language surface tools and configuration sync.
pub trait LanguageSurface: DeclaresFacets + Send + Sync {
  /// Canonical surface identifier name (e.g. `"rust"`, `"python"`).
  fn name(&self) -> &'static str;
  /// Human-readable display name.
  fn display_name(&self) -> &'static str {
    self.name()
  }
  /// Alternative alias names recognized for this surface.
  fn aliases(&self) -> &[&'static str] {
    &[]
  }
  /// Supported file extensions for auto-matching.
  fn file_extensions(&self) -> &[&'static str] {
    &[]
  }
  /// Detects whether this language surface is active in workspace `root`.
  fn detect(&self, root: &Path) -> bool;
  /// Returns information about required tools for this surface.
  fn tool_info(&self, config: &ResolvedLangConfig) -> Vec<ToolInfo>;
  /// Formats source files using underlying tools.
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult;
  /// Lints source files using underlying tools.
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult;
  /// Indicates whether this surface supports automatic lint fixing.
  fn supports_lint_fix(&self) -> bool {
    false
  }
  /// Synchronizes native tool configuration file.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult;
  /// Clones the surface into a boxed trait object.
  fn clone_box(&self) -> Box<dyn LanguageSurface>;
}

impl Clone for Box<dyn LanguageSurface> {
  fn clone(&self) -> Self {
    self.clone_box()
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::surfaces::{
    cpp, go, java, javascript, json, kotlin, markdown, python, rust, toml,
    typst, yaml,
  };

  #[test]
  fn test_surface_supports_lint_fix() {
    assert!(rust::RustSurface.supports_lint_fix());
    assert!(python::PythonSurface.supports_lint_fix());
    assert!(cpp::CppSurface.supports_lint_fix());
    assert!(!java::JavaSurface.supports_lint_fix());
    assert!(go::GoSurface.supports_lint_fix());
    assert!(!yaml::YamlSurface.supports_lint_fix());
    assert!(!toml::TomlSurface.supports_lint_fix());
    assert!(markdown::MarkdownSurface.supports_lint_fix());
    assert!(!json::JsonSurface.supports_lint_fix());
    assert!(!typst::TypstSurface.supports_lint_fix());
    assert!(javascript::JavaScriptSurface.supports_lint_fix());
    assert!(kotlin::KotlinSurface.supports_lint_fix());
  }

  #[test]
  fn test_surface_result_predicates_cover_every_status_variant() {
    fn result_for(status: SurfaceStatus) -> SurfaceResult {
      SurfaceResult {
        surface_name: "test",
        status,
        duration: std::time::Duration::from_millis(0),
      }
    }

    let passed = result_for(SurfaceStatus::Passed);
    assert!(passed.is_success());
    assert!(!passed.is_violation());
    assert!(!passed.is_error());

    let skipped = result_for(SurfaceStatus::Skipped {
      reason: "n/a".to_string(),
    });
    assert!(skipped.is_success());
    assert!(!skipped.is_violation());
    assert!(!skipped.is_error());

    let synced = result_for(SurfaceStatus::ConfigSynced {
      file: "x".to_string(),
      created: true,
    });
    assert!(synced.is_success());
    assert!(!synced.is_violation());
    assert!(!synced.is_error());

    let violations = result_for(SurfaceStatus::ViolationsFound {
      message: "bad".to_string(),
      diff: None,
    });
    assert!(!violations.is_success());
    assert!(violations.is_violation());
    assert!(!violations.is_error());

    let drifted = result_for(SurfaceStatus::ConfigDrifted {
      file: "x".to_string(),
      diff: "d".to_string(),
    });
    assert!(!drifted.is_success());
    assert!(drifted.is_violation());
    assert!(!drifted.is_error());

    let manual = result_for(SurfaceStatus::ManualConfig {
      file: "x".to_string(),
      suggestion: "s".to_string(),
    });
    assert!(!manual.is_success());
    assert!(manual.is_violation());
    assert!(!manual.is_error());

    let missing = result_for(SurfaceStatus::ToolMissing {
      binary: "x".to_string(),
      install_hint: "h".to_string(),
    });
    assert!(!missing.is_success());
    assert!(!missing.is_violation());
    assert!(missing.is_error());

    let exec_err = result_for(SurfaceStatus::ExecutionError {
      message: "boom".to_string(),
    });
    assert!(!exec_err.is_success());
    assert!(!exec_err.is_violation());
    assert!(exec_err.is_error());
  }

  #[test]
  fn test_box_dyn_language_surface_clone_preserves_identity() {
    let originals: Vec<Box<dyn LanguageSurface>> = vec![
      Box::new(rust::RustSurface),
      Box::new(python::PythonSurface),
      Box::new(kotlin::KotlinSurface),
    ];

    for original in &originals {
      let cloned = original.clone();
      assert_eq!(cloned.name(), original.name());
      assert_eq!(cloned.file_extensions(), original.file_extensions());
    }
  }

  #[test]
  fn test_unsupported_lint_fix_returns_skipped() {
    let dummy_ctx = test_ctx(Path::new("."), ResolvedLangConfig::new("dummy"));

    let unsupported_surfaces: Vec<Box<dyn LanguageSurface>> = vec![
      Box::new(yaml::YamlSurface),
      Box::new(toml::TomlSurface),
      Box::new(json::JsonSurface),
      Box::new(typst::TypstSurface),
      Box::new(java::JavaSurface),
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
