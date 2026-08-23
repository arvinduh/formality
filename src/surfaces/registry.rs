//! The [`SurfaceRegistry`]: discovery, lookup, and detection of the fleet of
//! registered [`LanguageSurface`] implementations.

use super::{
  LanguageSurface, cpp, go, java, javascript, json, kotlin, markdown, python,
  rust, toml, typst, yaml,
};
use crate::config::FormalityConfig;
use std::path::Path;

/// Registry for managing, querying, and discovering language surfaces.
#[derive(Clone)]
pub struct SurfaceRegistry {
  surfaces: Vec<Box<dyn LanguageSurface>>,
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

  /// Creates a registry pre-populated with the default fleet of 12 language surfaces.
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  /// Registers a concrete boxed surface instance in the registry.
  pub fn register(&mut self, surface: Box<dyn LanguageSurface>) {
    self.surfaces.push(surface);
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
      .find(|s| {
        s.name().eq_ignore_ascii_case(query)
          || s.aliases().iter().any(|a| a.eq_ignore_ascii_case(query))
      })
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
      .find(|s| {
        s.name().eq_ignore_ascii_case(query)
          || s.aliases().iter().any(|a| a.eq_ignore_ascii_case(query))
      })
      .map(|s| s.name())
  }

  /// Returns the canonical names of all registered surfaces.
  #[must_use]
  pub fn supported_languages(&self) -> Vec<&'static str> {
    self.surfaces.iter().map(|s| s.name()).collect()
  }

  /// Returns the number of registered surfaces.
  #[must_use]
  pub fn len(&self) -> usize {
    self.surfaces.len()
  }

  /// Returns whether the registry is empty.
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.surfaces.is_empty()
  }

  /// Detects active surfaces within `root` based on filesystem heuristics.
  #[must_use]
  pub fn detect_surfaces(&self, root: &Path) -> Vec<Box<dyn LanguageSurface>> {
    self
      .surfaces
      .iter()
      .filter(|s| s.detect(root))
      .cloned()
      .collect()
  }

  /// Performs smart detection respecting configuration allowlists and ignore rules.
  #[must_use]
  pub fn detect_surfaces_smart(
    &self,
    root: &Path,
    config: &FormalityConfig,
  ) -> Vec<Box<dyn LanguageSurface>> {
    let global = config.resolve_global();

    let is_ignored = |name: &str, aliases: &[&'static str]| -> bool {
      if let Some(ref ignores) = global.ignore_languages {
        ignores.iter().any(|ig| {
          ig.eq_ignore_ascii_case(name)
            || aliases.iter().any(|a| a.eq_ignore_ascii_case(ig))
        })
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
          let resolved = config.resolve_for_lang(s.name());
          if resolved.enabled {
            selected.push(s);
          }
        }
      }
      return selected;
    }

    // 2. Otherwise auto-detect all project surfaces minus ignore_languages
    self
      .surfaces
      .iter()
      .filter(|surface| {
        if is_ignored(surface.name(), surface.aliases()) {
          return false;
        }
        let resolved = config.resolve_for_lang(surface.name());
        if !resolved.enabled {
          return false;
        }
        surface.detect(root)
      })
      .cloned()
      .collect()
  }
}

#[must_use]
pub fn all_surfaces() -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().all_surfaces()
}

#[must_use]
pub fn detect_surfaces(root: &Path) -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().detect_surfaces(root)
}

#[must_use]
pub fn detect_surfaces_smart(
  root: &Path,
  config: &FormalityConfig,
) -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().detect_surfaces_smart(root, config)
}

#[must_use]
pub fn get_surface_by_name(name: &str) -> Option<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().get_surface_by_name(name)
}

#[must_use]
pub fn resolve_canonical_name(name_or_alias: &str) -> Option<&'static str> {
  SurfaceRegistry::default().resolve_canonical_name(name_or_alias)
}

#[cfg(test)]
mod tests {
  use super::*;

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

      // Verify resolve_canonical_name
      assert_eq!(
        resolve_canonical_name(query),
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
    assert!(resolve_canonical_name("unknown").is_none());
  }

  #[test]
  fn test_custom_surface_registry() {
    let mut reg = SurfaceRegistry::empty();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert_eq!(reg.all_surfaces().len(), 0);

    reg.register_surface::<crate::surfaces::rust::RustSurface>();
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
    assert!(reg.get_surface_by_name("rs").is_some());
    assert!(reg.get_surface_by_name("python").is_none());

    reg.register(Box::new(crate::surfaces::python::PythonSurface));
    assert_eq!(reg.len(), 2);
    assert!(reg.get_surface_by_name("py").is_some());

    assert_eq!(reg.supported_languages(), vec!["rust", "python"]);
  }

  #[test]
  fn test_detect_surfaces_finds_active_languages_only() {
    // detect_surfaces() had no direct test at all — only the smart-detection
    // variant's building blocks were exercised transitively via other tests.
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("script.py"), "print(1)").unwrap();

    let reg = SurfaceRegistry::default();
    let detected = reg.detect_surfaces(root);
    let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

    assert!(names.contains(&"rust"));
    assert!(names.contains(&"python"));
    // A language with no matching files/markers in the workspace must not be
    // detected as active.
    assert!(!names.contains(&"kotlin"));
  }

  #[test]
  fn test_detect_surfaces_smart_explicit_allowlist_minus_ignore() {
    use crate::config::FormalityConfig;

    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    // Files present for all three, but only rust/python are in the
    // allowlist and go is separately ignore_languages'd.
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("script.py"), "print(1)").unwrap();
    std::fs::write(root.join("main.go"), "package main").unwrap();

    let toml = r#"
      [global]
      languages = ["rust", "python", "go"]
      ignore_languages = ["go"]
    "#;
    let config =
      FormalityConfig::parse_str(toml, std::path::Path::new("test.toml"))
        .unwrap();

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

    // rust is in the allowlist but explicitly disabled via [lang.rust].
    let toml = r#"
      [global]
      languages = ["rust"]

      [lang.rust]
      enabled = false
    "#;
    let config =
      FormalityConfig::parse_str(toml, std::path::Path::new("test.toml"))
        .unwrap();

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

    // No explicit `languages` allowlist: auto-detect, but ignore python and
    // disable toml.
    let toml = r#"
      [global]
      ignore_languages = ["python"]

      [lang.toml]
      enabled = false
    "#;
    let config =
      FormalityConfig::parse_str(toml, std::path::Path::new("test.toml"))
        .unwrap();

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

    // No duplicate entries in CANONICAL_FLEET_ORDER
    assert_eq!(
      fleet_order.len(),
      fleet_set.len(),
      "CANONICAL_FLEET_ORDER contains duplicate surface names"
    );

    // Every surface in SurfaceRegistry::default() is present in CANONICAL_FLEET_ORDER
    for &name in &registered_names {
      assert!(
        fleet_set.contains(name),
        "Surface '{name}' from SurfaceRegistry::default() is missing from CANONICAL_FLEET_ORDER"
      );
    }

    // Every surface in CANONICAL_FLEET_ORDER corresponds to a registered surface
    for &name in fleet_order {
      assert!(
        registered_names.contains(name),
        "CANONICAL_FLEET_ORDER contains '{name}' which is not in SurfaceRegistry::default()"
      );
    }

    assert_eq!(
      fleet_order.len(),
      reg.len(),
      "CANONICAL_FLEET_ORDER length ({}) does not match SurfaceRegistry::default() count ({})",
      fleet_order.len(),
      reg.len()
    );
  }

  #[test]
  fn test_child_lsp_registry_covers_all_surfaces_or_exemptions() {
    use std::collections::HashSet;

    let reg = SurfaceRegistry::default();
    let registered_names: HashSet<&str> =
      reg.surfaces().iter().map(|s| s.name()).collect();

    // Intentional exemptions from child LSP passthrough server:
    // - "markdown": diagnostics only via fml lint (markdownlint / biome); no child LSP server spawned.
    // - "java": heavyweight Eclipse JDT LS not bundled; formatting and linting handled directly via google-java-format and Checkstyle.
    // - "kotlin": standalone Kotlin language server not integrated; formatting and linting handled directly via ktlint.
    let documented_exemptions: &[(&str, &str)] = &[
      (
        "markdown",
        "Markdown uses diagnostics-only via formality lint; no dedicated child LSP binary.",
      ),
      (
        "java",
        "Java uses google-java-format and Checkstyle directly without spawning JDT LS.",
      ),
      (
        "kotlin",
        "Kotlin uses ktlint directly for formatting and linting without a standalone language server.",
      ),
    ];

    let exemption_names: HashSet<&str> = documented_exemptions
      .iter()
      .map(|(name, _)| *name)
      .collect();

    // Ensure all documented exemptions are valid registered surfaces
    for &(exempt_name, _) in documented_exemptions {
      assert!(
        registered_names.contains(exempt_name),
        "Documented LSP exemption '{exempt_name}' is not a registered surface in SurfaceRegistry::default()"
      );
      assert!(
        crate::commands::lsp::child_lsp_for_surface(exempt_name).is_none(),
        "Exempt surface '{exempt_name}' unexpectedly has a registered child LSP"
      );
    }

    let child_lsp_surfaces: Vec<&str> =
      crate::commands::lsp::CHILD_LSP_REGISTRY
        .iter()
        .map(|c| c.surface)
        .collect();

    let child_lsp_set: HashSet<&str> =
      child_lsp_surfaces.iter().copied().collect();

    // No duplicate entries in CHILD_LSP_REGISTRY
    assert_eq!(
      child_lsp_surfaces.len(),
      child_lsp_set.len(),
      "CHILD_LSP_REGISTRY contains duplicate surface names"
    );

    // Every entry in CHILD_LSP_REGISTRY is a recognized surface
    for &surface_name in &child_lsp_surfaces {
      assert!(
        registered_names.contains(surface_name),
        "CHILD_LSP_REGISTRY contains '{surface_name}' which is not in SurfaceRegistry::default()"
      );
    }

    // Ensure no overlap between active child LSPs and exemptions
    for &surface_name in &child_lsp_surfaces {
      assert!(
        !exemption_names.contains(surface_name),
        "Surface '{surface_name}' is present in both CHILD_LSP_REGISTRY and documented exemptions"
      );
    }

    // Every registered surface must either have a child LSP or be explicitly exempt
    for &name in &registered_names {
      let has_child_lsp = child_lsp_set.contains(name);
      let is_exempt = exemption_names.contains(name);

      assert!(
        has_child_lsp || is_exempt,
        "Surface '{name}' has no child LSP in CHILD_LSP_REGISTRY and no documented exemption"
      );

      if has_child_lsp {
        let lsp = crate::commands::lsp::child_lsp_for_surface(name);
        assert!(
          lsp.is_some(),
          "child_lsp_for_surface('{name}') returned None for surface in CHILD_LSP_REGISTRY"
        );
        assert_eq!(
          lsp.unwrap().surface,
          name,
          "child_lsp_for_surface('{name}') returned mismatched surface"
        );
        assert!(
          !lsp.unwrap().binary.is_empty(),
          "child LSP binary name must not be empty for surface '{name}'"
        );
      }
    }

    // Exhaustive partition: CHILD_LSP_REGISTRY + exemptions == all registered surfaces
    assert_eq!(
      child_lsp_surfaces.len() + documented_exemptions.len(),
      reg.len(),
      "Sum of CHILD_LSP_REGISTRY ({}) and documented exemptions ({}) must equal total registered surfaces ({})",
      child_lsp_surfaces.len(),
      documented_exemptions.len(),
      reg.len()
    );
  }
}
