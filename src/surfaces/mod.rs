//! Language surfaces: the `LanguageSurface` trait, the fleet of per-language
//! implementations, and the shared machinery (registry, glob matching, tool
//! discovery, config sync) they're all built on.

pub mod cpp;
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
  AUTO_GENERATED_HEADER, EDITORCONFIG_FILE_NAME, NativeConfig,
  generate_editorconfig, generate_editorconfig_from_config,
  render_native_config, serialize_json_pretty, serialize_toml_with_header,
  serialize_yaml_with_header, sync_editorconfig,
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
#[path = "mod_tests.rs"]
mod tests;
