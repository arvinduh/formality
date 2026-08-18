use crate::config::facets::{Facet, FacetSupport};
use crate::config::{FormalityConfig, ResolvedGlobalConfig};
use crate::surfaces::{
  AUTO_GENERATED_HEADER, LanguageSurface, SurfaceResult, sync_file_helper,
};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

pub const EDITORCONFIG_FILE_NAME: &str = ".editorconfig";

const CANONICAL_FLEET_ORDER: &[&str] = &[
  "rust",
  "python",
  "cpp",
  "go",
  "yaml",
  "json",
  "toml",
  "markdown",
  "typst",
  "javascript",
];

/// Returns the standard EditorConfig section glob for a known or custom surface.
pub fn glob_for_surface(surface: &dyn LanguageSurface) -> String {
  match surface.name() {
    "rust" => "[*.rs]".to_string(),
    "python" => "[*.py]".to_string(),
    "cpp" => "[*.{c,cc,cpp,cxx,h,hh,hpp,hxx}]".to_string(),
    "go" => "[*.go]".to_string(),
    "yaml" => "[*.{yaml,yml}]".to_string(),
    "json" => "[*.json]".to_string(),
    "toml" => "[*.toml]".to_string(),
    "markdown" => "[*.md]".to_string(),
    "typst" => "[*.typ]".to_string(),
    "javascript" => "[*.{js,jsx,ts,tsx,mjs,cjs,mts,cts}]".to_string(),
    _ => {
      let exts = surface.file_extensions();
      if exts.len() == 1 {
        format!("[*.{}]", exts[0])
      } else if exts.is_empty() {
        format!("[*.{}]", surface.name())
      } else {
        format!("[*.{{{}}}]", exts.join(","))
      }
    }
  }
}

/// Synthesizes a portable root `.editorconfig` file combining `ResolvedGlobalConfig`
/// and the provided language surfaces' `LayoutFacet` settings.
pub fn generate_editorconfig(
  global: &ResolvedGlobalConfig,
  surfaces: &[Box<dyn LanguageSurface>],
) -> String {
  let mut out = String::new();
  out.push_str(AUTO_GENERATED_HEADER);
  out.push_str("root = true\n\n");

  let global_indent_style = if global.use_tabs { "tab" } else { "space" };
  let global_indent_size = global.indent_size;
  let global_max_line_length = Some(global.line_length);

  // Global [*] section
  out.push_str("[*]\n");
  out.push_str(&format!(
    "charset = {}\n",
    global.charset.to_ascii_lowercase()
  ));
  out.push_str(&format!(
    "end_of_line = {}\n",
    global.end_of_line.to_ascii_lowercase()
  ));
  out.push_str(&format!(
    "insert_final_newline = {}\n",
    global.insert_final_newline
  ));
  out.push_str(&format!(
    "trim_trailing_whitespace = {}\n",
    global.trim_trailing_whitespace
  ));
  out.push_str(&format!("indent_style = {}\n", global_indent_style));
  out.push_str(&format!("indent_size = {}\n", global_indent_size));
  out.push_str(&format!("max_line_length = {}\n", global.line_length));

  // Collect ordered distinct surfaces
  let mut seen = HashSet::new();
  let mut ordered_surfaces: Vec<&Box<dyn LanguageSurface>> = Vec::new();

  for &canonical_name in CANONICAL_FLEET_ORDER {
    if let Some(s) = surfaces.iter().find(|s| s.name() == canonical_name)
      && seen.insert(s.name())
    {
      ordered_surfaces.push(s);
    }
  }

  for s in surfaces {
    if seen.insert(s.name()) {
      ordered_surfaces.push(s);
    }
  }

  for surface in ordered_surfaces {
    let glob = glob_for_surface(surface.as_ref());
    let indent_style = match surface.facet_support(Facet::IndentTabs) {
      FacetSupport::Fixed("spaces") | FacetSupport::Fixed("space") => "space",
      FacetSupport::Fixed("tabs") | FacetSupport::Fixed("tab") => "tab",
      _ => global_indent_style,
    };

    let indent_size = global.indent_size;
    let max_line_length =
      if surface.facet_support(Facet::LineLength).is_unsupported() {
        None
      } else {
        Some(global.line_length)
      };

    let diverges = indent_style != global_indent_style
      || indent_size != global_indent_size
      || max_line_length != global_max_line_length;

    if !diverges {
      continue;
    }

    out.push('\n');
    out.push_str(&glob);
    out.push('\n');
    out.push_str(&format!("indent_style = {}\n", indent_style));
    out.push_str(&format!("indent_size = {}\n", indent_size));
    if let Some(mll) = max_line_length {
      out.push_str(&format!("max_line_length = {}\n", mll));
    }
  }

  out
}

/// Synthesizes `.editorconfig` from a full `FormalityConfig`, honoring per-language
/// overrides in addition to global defaults and layout facet capabilities.
pub fn generate_editorconfig_from_config(
  config: &FormalityConfig,
  surfaces: &[Box<dyn LanguageSurface>],
) -> String {
  let global = config.resolve_global();
  let mut out = String::new();
  out.push_str(AUTO_GENERATED_HEADER);
  out.push_str("root = true\n\n");

  let global_indent_style = if global.use_tabs { "tab" } else { "space" };
  let global_indent_size = global.indent_size;
  let global_max_line_length = Some(global.line_length);

  // Global [*] section
  out.push_str("[*]\n");
  out.push_str(&format!(
    "charset = {}\n",
    global.charset.to_ascii_lowercase()
  ));
  out.push_str(&format!(
    "end_of_line = {}\n",
    global.end_of_line.to_ascii_lowercase()
  ));
  out.push_str(&format!(
    "insert_final_newline = {}\n",
    global.insert_final_newline
  ));
  out.push_str(&format!(
    "trim_trailing_whitespace = {}\n",
    global.trim_trailing_whitespace
  ));
  out.push_str(&format!("indent_style = {}\n", global_indent_style));
  out.push_str(&format!("indent_size = {}\n", global_indent_size));
  out.push_str(&format!("max_line_length = {}\n", global.line_length));

  // Collect ordered distinct surfaces
  let mut seen = HashSet::new();
  let mut ordered_surfaces: Vec<&Box<dyn LanguageSurface>> = Vec::new();

  for &canonical_name in CANONICAL_FLEET_ORDER {
    if let Some(s) = surfaces.iter().find(|s| s.name() == canonical_name)
      && seen.insert(s.name())
    {
      ordered_surfaces.push(s);
    }
  }

  for s in surfaces {
    if seen.insert(s.name()) {
      ordered_surfaces.push(s);
    }
  }

  for surface in ordered_surfaces {
    let glob = glob_for_surface(surface.as_ref());
    let lang_cfg = config.resolve_for_lang(surface.name());

    let indent_style = match surface.facet_support(Facet::IndentTabs) {
      FacetSupport::Fixed("spaces") | FacetSupport::Fixed("space") => "space",
      FacetSupport::Fixed("tabs") | FacetSupport::Fixed("tab") => "tab",
      _ => {
        if lang_cfg.use_tabs {
          "tab"
        } else {
          "space"
        }
      }
    };

    let indent_size = lang_cfg.indent_size;
    let max_line_length =
      if surface.facet_support(Facet::LineLength).is_unsupported() {
        None
      } else {
        Some(lang_cfg.line_length)
      };

    let diverges = indent_style != global_indent_style
      || indent_size != global_indent_size
      || max_line_length != global_max_line_length;

    if !diverges {
      continue;
    }

    out.push('\n');
    out.push_str(&glob);
    out.push('\n');
    out.push_str(&format!("indent_style = {}\n", indent_style));
    out.push_str(&format!("indent_size = {}\n", indent_size));
    if let Some(mll) = max_line_length {
      out.push_str(&format!("max_line_length = {}\n", mll));
    }
  }

  out
}

/// Helper function to sync `.editorconfig` at the repository root.
pub fn sync_editorconfig(
  root: &Path,
  config: &FormalityConfig,
  surfaces: &[Box<dyn LanguageSurface>],
  check: bool,
) -> SurfaceResult {
  let start = Instant::now();
  let target = root.join(EDITORCONFIG_FILE_NAME);
  let content = generate_editorconfig_from_config(config, surfaces);
  sync_file_helper(
    &target,
    EDITORCONFIG_FILE_NAME,
    &content,
    check,
    start,
    "editorconfig",
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::surfaces::all_surfaces;

  #[test]
  fn test_generate_editorconfig_defaults() {
    let global = ResolvedGlobalConfig::default();
    let surfaces = all_surfaces();
    let ec = generate_editorconfig(&global, &surfaces);

    assert!(ec.starts_with(AUTO_GENERATED_HEADER));
    assert!(ec.contains("root = true"));
    assert!(ec.contains("[*]"));
    assert!(ec.contains("charset = utf-8"));
    assert!(ec.contains("end_of_line = lf"));
    assert!(ec.contains("insert_final_newline = true"));
    assert!(ec.contains("trim_trailing_whitespace = true"));
    assert!(ec.contains("indent_style = space"));
    assert!(ec.contains("indent_size = 2"));
    assert!(ec.contains("max_line_length = 80"));

    // Surfaces matching [*] baseline are omitted
    assert!(!ec.contains("[*.rs]"));
    assert!(!ec.contains("[*.py]"));
    assert!(!ec.contains("[*.{c,cc,cpp,cxx,h,hh,hpp,hxx}]"));
    assert!(!ec.contains("[*.{yaml,yml}]"));
    assert!(!ec.contains("[*.toml]"));
    assert!(!ec.contains("[*.md]"));
    assert!(!ec.contains("[*.typ]"));

    // JSON diverges due to unsupported line length
    assert!(ec.contains("[*.json]"));
    assert!(ec.contains("[*.json]\nindent_style = space\nindent_size = 2\n"));
    assert!(!ec.contains(
      "[*.json]\nindent_style = space\nindent_size = 2\nmax_line_length"
    ));

    // When all provided surfaces match [*], only [*] is emitted
    let matching_surfaces: Vec<Box<dyn LanguageSurface>> = vec![
      Box::new(crate::surfaces::rust::RustSurface),
      Box::new(crate::surfaces::toml::TomlSurface),
      Box::new(crate::surfaces::markdown::MarkdownSurface),
    ];
    let ec_matching = generate_editorconfig(&global, &matching_surfaces);
    assert!(ec_matching.contains("[*]"));
    assert!(!ec_matching.contains("[*.rs]"));
    assert!(!ec_matching.contains("[*.toml]"));
    assert!(!ec_matching.contains("[*.md]"));
  }

  #[test]
  fn test_generate_editorconfig_fixed_tabs_and_unsupported_line_length() {
    let global = ResolvedGlobalConfig {
      use_tabs: true,
      indent_size: 4,
      line_length: 100,
      ..ResolvedGlobalConfig::default()
    };

    let surfaces = all_surfaces();
    let ec = generate_editorconfig(&global, &surfaces);

    // Global has tab
    assert!(ec.contains("[*]\ncharset = utf-8\nend_of_line = lf\ninsert_final_newline = true\ntrim_trailing_whitespace = true\nindent_style = tab\nindent_size = 4\nmax_line_length = 100"));

    // Rust is fixed to spaces (diverges from tab)
    assert!(ec.contains(
      "[*.rs]\nindent_style = space\nindent_size = 4\nmax_line_length = 100"
    ));

    // Python is configurable -> tab (matches [*], omitted)
    assert!(!ec.contains("[*.py]"));

    // C++ is configurable -> tab (matches [*], omitted)
    assert!(!ec.contains("[*.{c,cc,cpp,cxx,h,hh,hpp,hxx}]"));

    // JSON is configurable for tabs, but unsupported for max_line_length (diverges from 100)
    assert!(ec.contains("[*.json]\nindent_style = tab\nindent_size = 4\n"));
    assert!(!ec.contains(
      "[*.json]\nindent_style = tab\nindent_size = 4\nmax_line_length"
    ));

    // YAML is fixed to spaces (diverges from tab)
    assert!(ec.contains("[*.{yaml,yml}]\nindent_style = space\nindent_size = 4\nmax_line_length = 100"));

    // TOML is configurable -> tab (matches [*], omitted)
    assert!(!ec.contains("[*.toml]"));

    // Markdown is configurable -> tab (matches [*], omitted)
    assert!(!ec.contains("[*.md]"));

    // Typst is fixed to spaces (diverges from tab)
    assert!(ec.contains(
      "[*.typ]\nindent_style = space\nindent_size = 4\nmax_line_length = 100"
    ));
  }

  #[test]
  fn test_generate_editorconfig_from_config_overrides() {
    let toml_str = r#"
[global]
indent_size = 2
line_length = 80
end_of_line = "crlf"
charset = "utf-8"

[lang.rust]
indent_size = 4
line_length = 100

[lang.python]
use_tabs = true
indent_size = 4
line_length = 88
"#;
    let config =
      FormalityConfig::parse_str(toml_str, Path::new("formality.toml"))
        .unwrap();
    let surfaces = all_surfaces();
    let ec = generate_editorconfig_from_config(&config, &surfaces);

    assert!(ec.contains("end_of_line = crlf"));
    assert!(ec.contains(
      "[*.rs]\nindent_style = space\nindent_size = 4\nmax_line_length = 100"
    ));
    assert!(ec.contains(
      "[*.py]\nindent_style = tab\nindent_size = 4\nmax_line_length = 88"
    ));

    // Non-diverging surfaces matching [*] are omitted
    assert!(!ec.contains("[*.{c,cc,cpp,cxx,h,hh,hpp,hxx}]"));
    assert!(!ec.contains("[*.{yaml,yml}]"));
    assert!(!ec.contains("[*.toml]"));
    assert!(!ec.contains("[*.md]"));
    assert!(!ec.contains("[*.typ]"));

    // JSON still diverges on unsupported max_line_length
    assert!(ec.contains("[*.json]\nindent_style = space\nindent_size = 2\n"));
  }
}
