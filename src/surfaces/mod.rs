//! Language surfaces: the `LanguageSurface` trait, the fleet of per-language
//! implementations, and the shared machinery (registry, glob matching, tool
//! discovery, config sync) they're all built on.

pub mod cpp;
pub mod editorconfig;
pub mod glob;
pub mod go;
pub mod java;
pub mod javascript;
pub mod json;
pub mod kotlin;
pub mod markdown;
pub mod native;
pub mod python;
pub mod registry;
pub mod rust;
pub mod sync;
pub mod toml;
pub mod tooling;
pub mod typst;
pub mod yaml;

pub use native::{
  AUTO_GENERATED_HEADER, AUTO_GENERATED_JSON_COMMENT, EDITORCONFIG_FILE_NAME,
  NativeConfig, generate_editorconfig, generate_editorconfig_from_config,
  render_native_config, serialize_json_pretty, serialize_toml_with_header,
  serialize_yaml_with_header, sync_editorconfig, sync_native_config,
};

pub use crate::config::facets::{DeclaresFacets, Facet, FacetSupport};
use crate::config::{ResolvedGlobalConfig, ResolvedLangConfig};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use glob::{find_files_with_ext, is_excluded, simple_glob_match};
pub use registry::{
  SurfaceRegistry, all_surfaces, detect_surfaces, detect_surfaces_smart,
  get_surface_by_name, resolve_canonical_name,
};
pub use sync::{diff_check_via_tempcopy, is_auto_generated, sync_file_helper};
pub use tooling::{
  check_binary_exists, create_tool_command, has_cargo_binstall,
  tool_missing_result,
};

/// Execution context shared with every [`LanguageSurface`] invocation for a
/// single `fml` command.
///
/// `paths` and `global_config` are wrapped in [`Arc`] because the runner
/// builds one `ExecutionContext` per surface and dispatches them in
/// parallel (`rayon::par_iter`): with plain owned fields, every surface
/// would deep-clone the *entire* candidate path list and global config on
/// every invocation, even though all surfaces see the same values. `Arc`
/// makes that a cheap refcount bump instead of an O(paths.len()) copy.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
  pub root: PathBuf,
  pub paths: Arc<Vec<PathBuf>>,
  pub global_config: Arc<ResolvedGlobalConfig>,
  pub lang_config: ResolvedLangConfig,
  pub check_only: bool,
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
  pub binary: &'static str,
  pub description: &'static str,
  pub install_hint: &'static str,
  pub is_required_for_fmt: bool,
  pub is_required_for_lint: bool,
}

impl ToolInfo {
  /// Returns the (program, args) for the first available installer in this
  /// tool's preference chain: prebuilt binary package managers first,
  /// falling back to `cargo install ... --locked` source compilation where
  /// the tool ships as a crate.
  pub fn get_auto_install_cmd(&self) -> Option<(String, Vec<String>)> {
    tooling::install_chain_for(self.binary)?
      .iter()
      .find(|method| method.is_available())
      .map(tooling::InstallMethod::command)
  }
}

#[derive(Debug, Clone)]
pub enum SurfaceStatus {
  Passed,
  ViolationsFound {
    message: String,
    diff: Option<String>,
  },
  ToolMissing {
    binary: String,
    install_hint: String,
  },
  ExecutionError {
    message: String,
  },
  Skipped {
    reason: String,
  },
  ConfigSynced {
    file: String,
    created: bool,
  },
  ConfigDrifted {
    file: String,
    diff: String,
  },
  /// Existing native config lacks the auto-generation header — it was written
  /// by hand. Overwriting silently would destroy intentional customization.
  ManualConfig {
    file: String,
    suggestion: String,
  },
}

#[derive(Debug, Clone)]
pub struct SurfaceResult {
  pub surface_name: &'static str,
  pub status: SurfaceStatus,
  pub duration: Duration,
}

impl SurfaceResult {
  #[must_use]
  pub fn is_success(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::Passed
        | SurfaceStatus::Skipped { .. }
        | SurfaceStatus::ConfigSynced { .. }
    )
  }

  #[must_use]
  pub fn is_violation(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::ViolationsFound { .. }
        | SurfaceStatus::ConfigDrifted { .. }
        | SurfaceStatus::ManualConfig { .. }
    )
  }

  #[must_use]
  pub fn is_error(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::ToolMissing { .. } | SurfaceStatus::ExecutionError { .. }
    )
  }
}

pub trait LanguageSurface: DeclaresFacets + Send + Sync {
  fn name(&self) -> &'static str;
  fn display_name(&self) -> &'static str {
    self.name()
  }
  fn aliases(&self) -> &[&'static str] {
    &[]
  }
  fn file_extensions(&self) -> &[&'static str] {
    &[]
  }
  fn detect(&self, root: &Path) -> bool;
  fn tool_info(&self, config: &ResolvedLangConfig) -> Vec<ToolInfo>;
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult;
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult;
  fn supports_lint_fix(&self) -> bool {
    false
  }
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult;
  fn clone_box(&self) -> Box<dyn LanguageSurface>;
}

impl Clone for Box<dyn LanguageSurface> {
  fn clone(&self) -> Self {
    self.clone_box()
  }
}

#[cfg(test)]
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
    // SurfaceResult::is_success / is_violation / is_error are the tri-state
    // classification every downstream consumer (the runner's exit-code
    // logic, doctor summaries, table rendering) relies on. Each of the 7
    // SurfaceStatus variants had never been checked against these
    // predicates directly — only indirectly, via a handful of individual
    // surfaces' own integration tests.
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
    // Clone for Box<dyn LanguageSurface> delegates to clone_box() on every
    // concrete surface; verify the round trip actually produces an
    // independent, equally-named clone rather than e.g. aliasing or
    // panicking, across a representative sample of surfaces.
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
    let dummy_ctx = ExecutionContext {
      root: PathBuf::from("."),
      paths: Arc::new(Vec::new()),
      global_config: Arc::new(ResolvedGlobalConfig::default()),
      lang_config: ResolvedLangConfig::new("dummy"),
      check_only: false,
    };

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
