//! The [`SurfaceRegistry`]: discovery, lookup, and detection of the fleet of
//! registered [`LanguageSurface`] implementations.

use super::{
  LanguageSurface, cpp, glob, go, java, javascript, json, kotlin, markdown,
  python, rust, toml, typst, yaml,
};
use crate::config::FormalityConfig;
use std::path::Path;

/// Registry for managing, querying, and discovering language surfaces.
#[derive(Clone)]
pub struct SurfaceRegistry {
  surfaces: Vec<Box<dyn LanguageSurface>>,
}

/// Whether `query` matches a surface's canonical `name` or any of its
/// `aliases`, case-insensitively (ASCII only). This is the single
/// "name-or-alias, case-insensitively" comparison shared by
/// [`SurfaceRegistry::get_surface_by_name`],
/// [`SurfaceRegistry::resolve_canonical_name`],
/// [`SurfaceRegistry::detect_surfaces_smart`]'s ignore-list check, and
/// `commands::doctor`'s unconfigured-language check — it used to be written
/// out independently at each of those call sites. `pub(crate)` so the doctor
/// command (outside this module) can reuse it too, per issue #276's "one
/// name/alias predicate" goal.
pub(crate) fn matches_name_or_alias(
  name: &str,
  aliases: &[&str],
  query: &str,
) -> bool {
  name.eq_ignore_ascii_case(query)
    || aliases.iter().any(|a| a.eq_ignore_ascii_case(query))
}

impl Default for SurfaceRegistry {
  fn default() -> Self {
    let mut reg = Self::empty();
    reg.register_surface::<rust::RustSurface>();
    reg.register_surface::<python::PythonSurface>();
    reg.register_surface::<cpp::CppSurface>();
    reg.register_surface::<java::JavaSurface>();
    reg.register_surface::<go::GoSurface>();
    reg.register_surface::<markdown::MarkdownSurface>();
    reg.register_surface::<yaml::YamlSurface>();
    reg.register_surface::<json::JsonSurface>();
    reg.register_surface::<toml::TomlSurface>();
    reg.register_surface::<typst::TypstSurface>();
    reg.register_surface::<javascript::JavaScriptSurface>();
    reg.register_surface::<kotlin::KotlinSurface>();
    reg
  }
}

impl SurfaceRegistry {
  /// Creates an empty registry with no registered surfaces.
  #[must_use]
  pub const fn empty() -> Self {
    Self {
      surfaces: Vec::new(),
    }
  }

  /// Registers a surface type that implements `LanguageSurface` and `Default`.
  pub fn register_surface<S: LanguageSurface + Default + 'static>(&mut self) {
    self.surfaces.push(Box::new(S::default()));
  }

  /// Returns a slice of references to all registered surfaces.
  #[must_use]
  pub fn surfaces(&self) -> &[Box<dyn LanguageSurface>] {
    &self.surfaces
  }

  /// Returns cloned boxed instances of all registered language surfaces.
  #[must_use]
  pub fn all_surfaces(&self) -> Vec<Box<dyn LanguageSurface>> {
    self.surfaces.clone()
  }

  /// Looks up a surface by canonical name or alias (case-insensitive, trimmed).
  #[must_use]
  pub fn get_surface_by_name(
    &self,
    name: &str,
  ) -> Option<Box<dyn LanguageSurface>> {
    let query = name.trim();
    self
      .surfaces
      .iter()
      .find(|s| matches_name_or_alias(s.name(), s.aliases(), query))
      .cloned()
  }

  /// Resolves an alias or surface name to its canonical surface name (e.g. "rs" -> "rust").
  #[must_use]
  pub fn resolve_canonical_name(
    &self,
    name_or_alias: &str,
  ) -> Option<&'static str> {
    let query = name_or_alias.trim();
    self
      .surfaces
      .iter()
      .find(|s| matches_name_or_alias(s.name(), s.aliases(), query))
      .map(|s| s.name())
  }

  /// Performs smart detection respecting configuration allowlists and ignore rules.
  #[must_use]
  pub fn detect_surfaces_smart(
    &self,
    root: &Path,
    config: &FormalityConfig,
  ) -> Vec<Box<dyn LanguageSurface>> {
    let present =
      std::cell::LazyCell::new(|| glob::PresentExtensions::scan(root, &[]));
    self.detect_surfaces_in(root, config, &present)
  }

  /// Detects like [`Self::detect_surfaces_smart`], reading extensions from
  /// the caller's `present`, which is forced only when auto-detection runs
  /// (no explicit `languages` list), so a caller that already walked the
  /// tree, or shares one walk between several checks, walks no further.
  #[must_use]
  pub fn detect_surfaces_in<F: FnOnce() -> glob::PresentExtensions>(
    &self,
    root: &Path,
    config: &FormalityConfig,
    present: &std::cell::LazyCell<glob::PresentExtensions, F>,
  ) -> Vec<Box<dyn LanguageSurface>> {
    let global = config.resolve_global();

    let is_ignored = |name: &str, aliases: &[&'static str]| -> bool {
      if let Some(ref ignores) = global.ignore_languages {
        ignores
          .iter()
          .any(|ig| matches_name_or_alias(name, aliases, ig))
      } else {
        false
      }
    };

    // 1. If explicit `languages` allowlist is defined, use that minus ignore_languages
    if let Some(ref explicit_langs) = global.languages {
      let mut selected = Vec::new();
      for lang_name in explicit_langs {
        if let Some(s) = self.get_surface_by_name(lang_name)
          && !is_ignored(s.name(), s.aliases())
        {
          let resolved = config.resolve_for_lang_with_global(s.name(), &global);
          if resolved.enabled {
            selected.push(s);
          }
        }
      }
      return selected;
    }

    // 2. Otherwise auto-detect all project surfaces minus ignore_languages,
    // every surface reading the same single walk of `root`.
    let present: &glob::PresentExtensions = present;
    self
      .surfaces
      .iter()
      .filter(|surface| {
        if is_ignored(surface.name(), surface.aliases()) {
          return false;
        }
        if !surface.detect(root, present) {
          return false;
        }
        let resolved =
          config.resolve_for_lang_with_global(surface.name(), &global);
        resolved.enabled
      })
      .cloned()
      .collect()
  }
}

static DEFAULT_REGISTRY: std::sync::LazyLock<SurfaceRegistry> =
  std::sync::LazyLock::new(SurfaceRegistry::default);

/// Returns a reference to the global default [`SurfaceRegistry`].
#[must_use]
pub fn default_registry() -> &'static SurfaceRegistry {
  &DEFAULT_REGISTRY
}

/// Returns a vector containing boxed instances of all supported language surfaces.
#[must_use]
pub fn all_surfaces() -> Vec<Box<dyn LanguageSurface>> {
  DEFAULT_REGISTRY.all_surfaces()
}

/// Detects and returns active language surfaces in `root` respecting `config` settings.
#[must_use]
pub fn detect_surfaces_smart(
  root: &Path,
  config: &FormalityConfig,
) -> Vec<Box<dyn LanguageSurface>> {
  DEFAULT_REGISTRY.detect_surfaces_smart(root, config)
}

/// Finds and returns a boxed [`LanguageSurface`] matching `name` or an alias if found.
#[must_use]
pub fn get_surface_by_name(name: &str) -> Option<Box<dyn LanguageSurface>> {
  DEFAULT_REGISTRY.get_surface_by_name(name)
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The trait's default declares no keys, which makes every
  /// `[lang.<name>.extra_args]` key a config error: a surface that forgets
  /// to override it would silently become unconfigurable.
  #[test]
  fn test_every_surface_declares_extra_args_tools() {
    for surface in all_surfaces() {
      assert!(
        !surface.extra_args_tools().is_empty(),
        "{} declares no extra_args tools",
        surface.name()
      );
    }
  }

  #[test]
  fn test_all_fleet_surfaces_present() {
    let surfaces = all_surfaces();
    assert_eq!(surfaces.len(), 12);

    let names: Vec<&str> = surfaces.iter().map(|s| s.name()).collect();
    let expected = [
      "rust",
      "python",
      "cpp",
      "java",
      "go",
      "markdown",
      "yaml",
      "json",
      "toml",
      "typst",
      "javascript",
      "kotlin",
    ];
    for exp in expected {
      assert!(
        names.contains(&exp),
        "Surface '{exp}' missing from all_surfaces()"
      );
    }
  }

  #[test]
  fn test_get_surface_by_name_canonical_and_aliases() {
    let test_cases = [
      ("rust", "rust"),
      ("rs", "rust"),
      ("python", "python"),
      ("py", "python"),
      ("cpp", "cpp"),
      ("c", "cpp"),
      ("c++", "cpp"),
      ("cxx", "cpp"),
      ("java", "java"),
      ("jav", "java"),
      ("go", "go"),
      ("golang", "go"),
      ("markdown", "markdown"),
      ("md", "markdown"),
      ("yaml", "yaml"),
      ("yml", "yaml"),
      ("json", "json"),
      ("toml", "toml"),
      ("typst", "typst"),
      ("typ", "typst"),
      ("javascript", "javascript"),
      ("js", "javascript"),
      ("ts", "javascript"),
      ("typescript", "javascript"),
      ("jsx", "javascript"),
      ("tsx", "javascript"),
      ("kotlin", "kotlin"),
      ("kt", "kotlin"),
    ];

    for (query, canonical) in test_cases {
      let surface = get_surface_by_name(query);
      assert!(
        surface.is_some(),
        "Failed to resolve surface for query '{query}'"
      );
      assert_eq!(
        surface.unwrap().name(),
        canonical,
        "Query '{query}' resolved to unexpected surface name"
      );

      assert_eq!(
        default_registry().resolve_canonical_name(query),
        Some(canonical),
        "resolve_canonical_name failed for '{query}'"
      );
    }
  }

  #[test]
  fn test_get_surface_by_name_case_insensitive() {
    let variations = [
      ("RUST", "rust"),
      ("Rust", "rust"),
      ("rS", "rust"),
      ("RS", "rust"),
      ("PYTHON", "python"),
      ("Python", "python"),
      ("Py", "python"),
      ("PY", "python"),
      ("CPP", "cpp"),
      ("Cpp", "cpp"),
      ("C++", "cpp"),
      ("CXX", "cpp"),
      ("Cxx", "cpp"),
      ("C", "cpp"),
      ("JAVA", "java"),
      ("Java", "java"),
      ("JAV", "java"),
      ("MARKDOWN", "markdown"),
      ("Markdown", "markdown"),
      ("MD", "markdown"),
      ("Md", "markdown"),
      ("YAML", "yaml"),
      ("Yaml", "yaml"),
      ("YML", "yaml"),
      ("Yml", "yaml"),
      ("JSON", "json"),
      ("Json", "json"),
      ("TOML", "toml"),
      ("Toml", "toml"),
      ("TYPST", "typst"),
      ("Typst", "typst"),
      ("TYP", "typst"),
      ("Typ", "typst"),
      ("JAVASCRIPT", "javascript"),
      ("JavaScript", "javascript"),
      ("JS", "javascript"),
      ("Js", "javascript"),
      ("TS", "javascript"),
      ("Ts", "javascript"),
      ("KOTLIN", "kotlin"),
      ("Kotlin", "kotlin"),
      ("KT", "kotlin"),
      ("Kt", "kotlin"),
      ("  rust  ", "rust"),
      ("  C++  ", "cpp"),
    ];

    for (query, canonical) in variations {
      let surface = get_surface_by_name(query);
      assert!(
        surface.is_some(),
        "Case-insensitive lookup failed for '{query}'"
      );
      assert_eq!(surface.unwrap().name(), canonical);
    }
  }

  #[test]
  fn test_get_surface_by_name_nonexistent() {
    assert!(get_surface_by_name("nonexistent").is_none());
    assert!(get_surface_by_name("unknown_lang").is_none());
    assert!(get_surface_by_name("").is_none());
    assert!(
      default_registry()
        .resolve_canonical_name("unknown")
        .is_none()
    );
  }

  #[test]
  fn test_custom_surface_registry() {
    let mut reg = SurfaceRegistry::empty();
    assert!(reg.surfaces().is_empty());
    assert_eq!(reg.all_surfaces().len(), 0);

    reg.register_surface::<rust::RustSurface>();
    assert_eq!(reg.surfaces().len(), 1);
    assert!(reg.get_surface_by_name("rs").is_some());
    assert!(reg.get_surface_by_name("python").is_none());

    reg.register_surface::<python::PythonSurface>();
    assert_eq!(reg.surfaces().len(), 2);
    assert!(reg.get_surface_by_name("py").is_some());

    let names: Vec<&str> = reg.surfaces().iter().map(|s| s.name()).collect();
    assert_eq!(names, vec!["rust", "python"]);
  }

  #[test]
  fn test_detect_surfaces_finds_active_languages_only() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("script.py"), "print(1)").unwrap();

    let reg = SurfaceRegistry::default();
    let detected =
      reg.detect_surfaces_smart(root, &FormalityConfig::with_defaults());
    let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

    assert!(names.contains(&"rust"));
    assert!(names.contains(&"python"));
    assert!(!names.contains(&"kotlin"));
  }

  #[test]
  fn test_detect_surfaces_smart_explicit_allowlist_minus_ignore() {
    use crate::config::FormalityConfig;

    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("script.py"), "print(1)").unwrap();
    std::fs::write(root.join("main.go"), "package main").unwrap();

    let toml = r#"
      [global]
      languages = ["rust", "python", "go"]
      ignore_languages = ["go"]
    "#;
    let config =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

    let reg = SurfaceRegistry::default();
    let selected = reg.detect_surfaces_smart(root, &config);
    let names: Vec<&str> = selected.iter().map(|s| s.name()).collect();

    assert_eq!(names.len(), 2);
    assert!(names.contains(&"rust"));
    assert!(names.contains(&"python"));
    assert!(
      !names.contains(&"go"),
      "go should be excluded by ignore_languages even though it's in the allowlist"
    );
  }

  #[test]
  fn test_detect_surfaces_smart_explicit_allowlist_respects_disabled_lang() {
    use crate::config::FormalityConfig;

    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();

    let toml = r#"
      [global]
      languages = ["rust"]

      [lang.rust]
      enabled = false
    "#;
    let config =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

    let reg = SurfaceRegistry::default();
    let selected = reg.detect_surfaces_smart(root, &config);
    assert!(
      selected.is_empty(),
      "an explicitly disabled language must not be selected even when allowlisted"
    );
  }

  #[test]
  fn test_detect_surfaces_smart_auto_detect_respects_ignore_and_disabled() {
    use crate::config::FormalityConfig;

    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("script.py"), "print(1)").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();

    let toml = r#"
      [global]
      ignore_languages = ["python"]

      [lang.toml]
      enabled = false
    "#;
    let config =
      FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

    let reg = SurfaceRegistry::default();
    let selected = reg.detect_surfaces_smart(root, &config);
    let names: Vec<&str> = selected.iter().map(|s| s.name()).collect();

    assert!(names.contains(&"rust"));
    assert!(
      !names.contains(&"python"),
      "python should be excluded by ignore_languages"
    );
    assert!(
      !names.contains(&"toml"),
      "toml should be excluded by enabled = false even though Cargo.toml is present"
    );
  }

  #[test]
  fn test_detect_surfaces_smart_shares_one_scan_and_honours_overrides() {
    /// Whether each `detect` call's scan saw a file an earlier call wrote.
    static SEEN: std::sync::Mutex<Vec<bool>> =
      std::sync::Mutex::new(Vec::new());

    #[derive(Clone, Default)]
    struct RecordingSurface;

    impl crate::config::facets::DeclaresFacets for RecordingSurface {
      fn facet_support(
        &self,
        _: crate::config::facets::Facet,
      ) -> crate::config::facets::FacetSupport {
        crate::config::facets::FacetSupport::Unsupported
      }
    }

    impl LanguageSurface for RecordingSurface {
      fn name(&self) -> &'static str {
        "recording"
      }
      fn detect(&self, root: &Path, present: &glob::PresentExtensions) -> bool {
        SEEN.lock().unwrap().push(present.contains("probe"));
        std::fs::write(root.join("written.probe"), "").unwrap();
        true
      }
      fn tool_info(
        &self,
        _: &crate::config::ResolvedLangConfig,
      ) -> Vec<crate::surfaces::ToolInfo> {
        unimplemented!()
      }
      fn format(
        &self,
        _: &crate::surfaces::ExecutionContext,
      ) -> crate::surfaces::SurfaceResult {
        unimplemented!()
      }
      fn lint(
        &self,
        _: &crate::surfaces::ExecutionContext,
        _: bool,
      ) -> crate::surfaces::SurfaceResult {
        unimplemented!()
      }
      fn sync_config(
        &self,
        _: &crate::surfaces::ExecutionContext,
        _: bool,
      ) -> crate::surfaces::SurfaceResult {
        unimplemented!()
      }
      fn clone_box(&self) -> Box<dyn LanguageSurface> {
        Box::new(self.clone())
      }
    }

    let mut reg = SurfaceRegistry::empty();
    reg.register_surface::<RecordingSurface>();
    reg.register_surface::<RecordingSurface>();
    let temp = tempfile::TempDir::new().unwrap();
    let detected =
      reg.detect_surfaces_smart(temp.path(), &FormalityConfig::with_defaults());

    // Both overrides ran and were honoured on an empty tree.
    assert_eq!(detected.len(), 2);
    // The second surface did not see the first one's file: both read one
    // scan taken before either ran, so detection walked the tree once.
    assert_eq!(*SEEN.lock().unwrap(), [false, false]);
  }

  #[test]
  fn test_surface_file_extensions() {
    for surface in all_surfaces() {
      let exts = surface.file_extensions();
      assert!(
        !exts.is_empty(),
        "Surface '{}' has empty file extensions",
        surface.name()
      );
    }
  }

  #[test]
  fn test_canonical_fleet_order_covers_all_surfaces() {
    use std::collections::HashSet;

    let reg = SurfaceRegistry::default();
    let registered_names: HashSet<&str> =
      reg.surfaces().iter().map(|s| s.name()).collect();

    let fleet_order = crate::surfaces::editorconfig::CANONICAL_FLEET_ORDER;
    let fleet_set: HashSet<&str> = fleet_order.iter().copied().collect();

    assert_eq!(
      fleet_order.len(),
      fleet_set.len(),
      "CANONICAL_FLEET_ORDER contains duplicate surface names"
    );

    for &name in &registered_names {
      assert!(
        fleet_set.contains(name),
        "Surface '{name}' from SurfaceRegistry::default() is missing from CANONICAL_FLEET_ORDER"
      );
    }

    for &name in fleet_order {
      assert!(
        registered_names.contains(name),
        "CANONICAL_FLEET_ORDER contains '{name}' which is not in SurfaceRegistry::default()"
      );
    }

    assert_eq!(
      fleet_order.len(),
      reg.surfaces().len(),
      "CANONICAL_FLEET_ORDER length ({}) does not match SurfaceRegistry::default() count ({})",
      fleet_order.len(),
      reg.surfaces().len()
    );
  }
}
