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
  pub const fn empty() -> Self {
    Self {
      surfaces: Vec::new(),
    }
  }

  /// Creates a registry pre-populated with the default fleet of 12 language surfaces.
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
  pub fn surfaces(&self) -> &[Box<dyn LanguageSurface>] {
    &self.surfaces
  }

  /// Returns cloned boxed instances of all registered language surfaces.
  pub fn all_surfaces(&self) -> Vec<Box<dyn LanguageSurface>> {
    self.surfaces.clone()
  }

  /// Looks up a surface by canonical name or alias (case-insensitive, trimmed).
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
  pub fn supported_languages(&self) -> Vec<&'static str> {
    self.surfaces.iter().map(|s| s.name()).collect()
  }

  /// Returns the number of registered surfaces.
  pub fn len(&self) -> usize {
    self.surfaces.len()
  }

  /// Returns whether the registry is empty.
  pub fn is_empty(&self) -> bool {
    self.surfaces.is_empty()
  }

  /// Detects active surfaces within `root` based on filesystem heuristics.
  pub fn detect_surfaces(&self, root: &Path) -> Vec<Box<dyn LanguageSurface>> {
    self
      .surfaces
      .iter()
      .filter(|s| s.detect(root))
      .cloned()
      .collect()
  }

  /// Performs smart detection respecting configuration allowlists and ignore rules.
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

pub fn all_surfaces() -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().all_surfaces()
}

pub fn detect_surfaces(root: &Path) -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().detect_surfaces(root)
}

pub fn detect_surfaces_smart(
  root: &Path,
  config: &FormalityConfig,
) -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().detect_surfaces_smart(root, config)
}

pub fn get_surface_by_name(name: &str) -> Option<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().get_surface_by_name(name)
}

pub fn resolve_canonical_name(name_or_alias: &str) -> Option<&'static str> {
  SurfaceRegistry::default().resolve_canonical_name(name_or_alias)
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
