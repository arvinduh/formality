//! Cross-language `.editorconfig` generation and synchronization.
//!
//! Aggregates canonical formatting facets into a unified `.editorconfig`. Individual
//! surface facet declarations are owned by each language surface in [`crate::surfaces`].

use std::collections;
use std::fmt::Write;
use std::path;
use std::time;

use crate::config;
use crate::config::facets;
use crate::surfaces;
use crate::surfaces::LanguageSurface;

/// Default filename for `.editorconfig` files.
pub const EDITORCONFIG_FILE_NAME: &str = ".editorconfig";

/// Canonical ordering of supported surfaces when writing `.editorconfig` blocks.
pub const CANONICAL_FLEET_ORDER: &[&str] = &[
  "rust",
  "python",
  "cpp",
  "java",
  "go",
  "yaml",
  "json",
  "toml",
  "markdown",
  "typst",
  "javascript",
  "kotlin",
];

/// Returns the standard `EditorConfig` section glob for a known or custom surface.
pub fn glob_for_surface(surface: &dyn LanguageSurface) -> String {
  match surface.name() {
    "rust" => "[*.rs]".to_string(),
    "python" => "[*.py]".to_string(),
    "cpp" => "[*.{c,cc,cpp,cxx,h,hh,hpp,hxx}]".to_string(),
    "java" => "[*.java]".to_string(),
    "go" => "[*.go]".to_string(),
    "yaml" => "[*.{yaml,yml}]".to_string(),
    "json" => "[*.json]".to_string(),
    "toml" => "[*.toml]".to_string(),
    "markdown" => "[*.md]".to_string(),
    "typst" => "[*.typ]".to_string(),
    "javascript" => "[*.{js,jsx,ts,tsx,mjs,cjs,mts,cts}]".to_string(),
    "kotlin" => "[*.{kt,kts}]".to_string(),
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

/// Synthesizes `.editorconfig` from a full `FormalityConfig`, honoring per-language
/// overrides in addition to global defaults and layout facet capabilities.
#[must_use]
pub fn generate_editorconfig_from_config(
  config: &config::FormalityConfig,
  surfaces: &[Box<dyn LanguageSurface>],
) -> String {
  let global = config.resolve_global();
  generate_editorconfig_internal(&global, surfaces, |surface| {
    let lang_cfg = config.resolve_for_lang_with_global(surface.name(), &global);
    (
      lang_cfg.use_tabs,
      lang_cfg.indent_size,
      lang_cfg.line_length,
    )
  })
}

fn generate_editorconfig_internal<F>(
  global: &config::ResolvedGlobalConfig,
  surfaces: &[Box<dyn LanguageSurface>],
  surface_layout: F,
) -> String
where
  F: Fn(&dyn LanguageSurface) -> (bool, usize, usize),
{
  let mut out = String::new();
  out.push_str(surfaces::AUTO_GENERATED_HEADER);
  out.push_str("root = true\n\n");

  let global_indent_style = if global.use_tabs { "tab" } else { "space" };
  let global_indent_size = global.indent_size;
  let global_max_line_length = Some(global.line_length);

  // Global [*] section
  out.push_str("[*]\n");
  let _ = writeln!(out, "charset = {}", global.charset.to_ascii_lowercase());
  let _ = writeln!(
    out,
    "end_of_line = {}",
    global.end_of_line.to_ascii_lowercase()
  );
  let _ = writeln!(
    out,
    "insert_final_newline = {}",
    global.insert_final_newline
  );
  let _ = writeln!(
    out,
    "trim_trailing_whitespace = {}",
    global.trim_trailing_whitespace
  );
  let _ = writeln!(out, "indent_style = {global_indent_style}");
  let _ = writeln!(out, "indent_size = {global_indent_size}");
  let _ = writeln!(out, "max_line_length = {}", global.line_length);

  // Collect ordered distinct surfaces
  let mut seen = collections::HashSet::new();
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
    let (use_tabs, indent_size, line_length) = surface_layout(surface.as_ref());

    let indent_style = match surface.facet_support(facets::Facet::IndentTabs) {
      facets::FacetSupport::Fixed("spaces" | "space") => "space",
      facets::FacetSupport::Fixed("tabs" | "tab") => "tab",
      _ => {
        if use_tabs {
          "tab"
        } else {
          "space"
        }
      }
    };

    let indent_size = match surface.facet_support(facets::Facet::IndentWidth) {
      facets::FacetSupport::Fixed(v) => v.parse().unwrap_or(indent_size),
      _ => indent_size,
    };
    let max_line_length = match surface.facet_support(facets::Facet::LineLength)
    {
      facets::FacetSupport::Unsupported => None,
      facets::FacetSupport::Fixed(v) => v.parse().ok().or(Some(line_length)),
      facets::FacetSupport::Configurable => Some(line_length),
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
    let _ = writeln!(out, "indent_style = {indent_style}");
    let _ = writeln!(out, "indent_size = {indent_size}");
    if let Some(mll) = max_line_length {
      let _ = writeln!(out, "max_line_length = {mll}");
    }
  }

  out
}

/// Helper function to sync `.editorconfig` at the repository root.
#[must_use]
pub fn sync_editorconfig(
  root: &path::Path,
  config: &config::FormalityConfig,
  surfaces: &[Box<dyn LanguageSurface>],
  check: bool,
) -> surfaces::SurfaceResult {
  let start = time::Instant::now();
  let target = root.join(EDITORCONFIG_FILE_NAME);
  let content = generate_editorconfig_from_config(config, surfaces);
  surfaces::sync_file_helper(
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

  #[test]
  fn test_generate_editorconfig_defaults() {
    let config = config::FormalityConfig::with_defaults();
    let surfaces = surfaces::all_surfaces();
    let ec = generate_editorconfig_from_config(&config, &surfaces);

    assert!(ec.starts_with(surfaces::AUTO_GENERATED_HEADER));
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
      Box::new(surfaces::rust::RustSurface),
      Box::new(surfaces::toml::TomlSurface),
      Box::new(surfaces::markdown::MarkdownSurface),
    ];
    let ec_matching =
      generate_editorconfig_from_config(&config, &matching_surfaces);
    assert!(ec_matching.contains("[*]"));
    assert!(!ec_matching.contains("[*.rs]"));
    assert!(!ec_matching.contains("[*.toml]"));
    assert!(!ec_matching.contains("[*.md]"));
  }

  #[test]
  fn test_generate_editorconfig_fixed_tabs_and_unsupported_line_length() {
    let toml_str =
      "[global]\nuse_tabs = true\nindent_size = 4\nline_length = 100\n";
    let config = config::FormalityConfig::parse_str(
      toml_str,
      path::Path::new("formality.toml"),
    )
    .unwrap();

    let surfaces = surfaces::all_surfaces();
    let ec = generate_editorconfig_from_config(&config, &surfaces);

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
    let config = config::FormalityConfig::parse_str(
      toml_str,
      path::Path::new("formality.toml"),
    )
    .unwrap();
    let surfaces = surfaces::all_surfaces();
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
