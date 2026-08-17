use serde::{Deserialize, Serialize};

/// Canonical vocabulary of formatting & linting facets across all language surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Facet {
  /// Indentation using tab characters instead of spaces.
  IndentTabs,
  /// Number of spaces per indentation level.
  IndentWidth,
  /// Maximum line length / column limit before wrapping.
  LineLength,
  /// Quotation style for strings (single vs double quotes).
  QuoteStyle,
  /// Trailing comma style in multiline structures.
  TrailingComma,
  /// Organization and sorting of imports / includes.
  ImportSort,
  /// Wrapping behavior for prose and markdown text.
  ProseWrap,
  /// Language edition or compiler epoch (e.g., Rust 2021, 2024).
  Edition,
  /// Language standard specification version (e.g., C++17, C11).
  Standard,
}

impl Facet {
  /// Slice containing all canonical facets.
  pub const ALL: &'static [Facet] = &[
    Facet::IndentTabs,
    Facet::IndentWidth,
    Facet::LineLength,
    Facet::QuoteStyle,
    Facet::TrailingComma,
    Facet::ImportSort,
    Facet::ProseWrap,
    Facet::Edition,
    Facet::Standard,
  ];

  /// The canonical snake_case identifier for this facet.
  pub const fn name(&self) -> &'static str {
    match self {
      Facet::IndentTabs => "indent_tabs",
      Facet::IndentWidth => "indent_width",
      Facet::LineLength => "line_length",
      Facet::QuoteStyle => "quote_style",
      Facet::TrailingComma => "trailing_comma",
      Facet::ImportSort => "import_sort",
      Facet::ProseWrap => "prose_wrap",
      Facet::Edition => "edition",
      Facet::Standard => "standard",
    }
  }

  /// Human-readable description of what this facet configures.
  pub const fn description(&self) -> &'static str {
    match self {
      Facet::IndentTabs => "Indentation using tabs instead of spaces",
      Facet::IndentWidth => "Number of spaces per indentation level",
      Facet::LineLength => "Maximum line length / column limit",
      Facet::QuoteStyle => "Quotation mark style (single vs double)",
      Facet::TrailingComma => "Trailing comma style in multiline structures",
      Facet::ImportSort => "Sorting and organization of imports",
      Facet::ProseWrap => "Prose wrapping behavior for text/markdown",
      Facet::Edition => "Language edition / compiler epoch (e.g. 2021, 2024)",
      Facet::Standard => "Language standard version (e.g. c++17, c11)",
    }
  }

  /// Parses a facet from its canonical name or common aliases.
  pub fn from_name(s: &str) -> Option<Self> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
      "indent_tabs" | "indent-tabs" | "use_tabs" | "use-tabs" | "tabs" => {
        Some(Facet::IndentTabs)
      }
      "indent_width" | "indent-width" | "indent_size" | "indent-size"
      | "tab_width" | "tab-width" => Some(Facet::IndentWidth),
      "line_length" | "line-length" | "max_width" | "max-width"
      | "column_limit" | "column-limit" | "print_width" | "print-width" => {
        Some(Facet::LineLength)
      }
      "quote_style" | "quote-style" | "quotes" | "quote" => {
        Some(Facet::QuoteStyle)
      }
      "trailing_comma" | "trailing-comma" | "trailing_commas" => {
        Some(Facet::TrailingComma)
      }
      "import_sort" | "import-sort" | "sort_imports" | "isort" => {
        Some(Facet::ImportSort)
      }
      "prose_wrap" | "prose-wrap" => Some(Facet::ProseWrap),
      "edition" => Some(Facet::Edition),
      "standard" | "std" => Some(Facet::Standard),
      _ => None,
    }
  }
}

impl std::fmt::Display for Facet {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.name())
  }
}

/// Level of support a surface provides for a given facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetSupport {
  /// The user can freely configure this facet for the surface.
  Configurable,
  /// The tool/language enforces a single fixed value; setting a different value warns.
  Fixed(&'static str),
  /// The concept does not exist or cannot be configured for this tool/language.
  Unsupported,
}

impl FacetSupport {
  pub fn is_configurable(&self) -> bool {
    matches!(self, FacetSupport::Configurable)
  }

  pub fn is_fixed(&self) -> bool {
    matches!(self, FacetSupport::Fixed(_))
  }

  pub fn is_unsupported(&self) -> bool {
    matches!(self, FacetSupport::Unsupported)
  }

  pub fn fixed_value(&self) -> Option<&'static str> {
    match self {
      FacetSupport::Fixed(v) => Some(v),
      _ => None,
    }
  }
}

impl std::fmt::Display for FacetSupport {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      FacetSupport::Configurable => write!(f, "configurable"),
      FacetSupport::Fixed(val) => write!(f, "fixed({})", val),
      FacetSupport::Unsupported => write!(f, "unsupported"),
    }
  }
}

/// Trait implemented by language surfaces to declare their facet capabilities.
pub trait DeclaresFacets {
  /// Queries the support state for a given canonical facet.
  fn facet_support(&self, facet: Facet) -> FacetSupport;

  /// Returns whether this surface allows configuring the given facet.
  fn is_facet_configurable(&self, facet: Facet) -> bool {
    self.facet_support(facet) == FacetSupport::Configurable
  }

  /// Returns all facets with their support status for this surface.
  fn declared_facets(&self) -> Vec<(Facet, FacetSupport)> {
    Facet::ALL
      .iter()
      .copied()
      .map(|f| (f, self.facet_support(f)))
      .collect()
  }

  /// Returns all configurable facets for this surface.
  fn configurable_facets(&self) -> Vec<Facet> {
    Facet::ALL
      .iter()
      .copied()
      .filter(|&f| self.facet_support(f) == FacetSupport::Configurable)
      .collect()
  }

  /// Returns all fixed facets for this surface.
  fn fixed_facets(&self) -> Vec<(Facet, &'static str)> {
    Facet::ALL
      .iter()
      .copied()
      .filter_map(|f| match self.facet_support(f) {
        FacetSupport::Fixed(val) => Some((f, val)),
        _ => None,
      })
      .collect()
  }

  /// Returns all unsupported facets for this surface.
  fn unsupported_facets(&self) -> Vec<Facet> {
    Facet::ALL
      .iter()
      .copied()
      .filter(|&f| self.facet_support(f) == FacetSupport::Unsupported)
      .collect()
  }
}

/// Diagnostic severity for facet validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetDiagnosticSeverity {
  Warning,
  Error,
}

/// Diagnostic produced when validating a facet configuration against a surface's declared capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FacetDiagnostic {
  pub surface: String,
  pub facet: Facet,
  pub support: FacetSupport,
  pub severity: FacetDiagnosticSeverity,
  pub message: String,
}

impl std::fmt::Display for FacetDiagnostic {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let level = match self.severity {
      FacetDiagnosticSeverity::Warning => "WARN",
      FacetDiagnosticSeverity::Error => "ERROR",
    };
    write!(
      f,
      "[{}] surface '{}', facet '{}' ({}): {}",
      level, self.surface, self.facet, self.support, self.message
    )
  }
}

/// Checks if a user-supplied configured value is semantically equivalent to a fixed value.
fn is_value_compatible_with_fixed(
  facet: Facet,
  fixed_expected: &str,
  configured_value: &str,
) -> bool {
  if configured_value.eq_ignore_ascii_case(fixed_expected) {
    return true;
  }
  match facet {
    Facet::IndentTabs => {
      if fixed_expected.eq_ignore_ascii_case("spaces")
        || fixed_expected.eq_ignore_ascii_case("space")
      {
        matches!(
          configured_value.to_ascii_lowercase().as_str(),
          "false" | "spaces" | "space" | "no" | "off"
        )
      } else if fixed_expected.eq_ignore_ascii_case("tabs")
        || fixed_expected.eq_ignore_ascii_case("tab")
      {
        matches!(
          configured_value.to_ascii_lowercase().as_str(),
          "true" | "tabs" | "tab" | "yes" | "on"
        )
      } else {
        false
      }
    }
    Facet::QuoteStyle => {
      if fixed_expected.eq_ignore_ascii_case("double") {
        matches!(
          configured_value.to_ascii_lowercase().as_str(),
          "double" | "\"" | "double_quotes" | "doublequote" | "doublequotes"
        )
      } else if fixed_expected.eq_ignore_ascii_case("single") {
        matches!(
          configured_value.to_ascii_lowercase().as_str(),
          "single" | "'" | "single_quotes" | "singlequote" | "singlequotes"
        )
      } else {
        false
      }
    }
    Facet::TrailingComma => {
      if fixed_expected.eq_ignore_ascii_case("none") {
        matches!(
          configured_value.to_ascii_lowercase().as_str(),
          "none" | "false" | "never" | "no" | "off"
        )
      } else if fixed_expected.eq_ignore_ascii_case("always") {
        matches!(
          configured_value.to_ascii_lowercase().as_str(),
          "always" | "true" | "yes" | "on" | "all"
        )
      } else {
        false
      }
    }
    _ => false,
  }
}

/// Validates a single facet configuration value against a declared support state.
pub fn validate_facet_value(
  surface_name: &str,
  support: FacetSupport,
  facet: Facet,
  configured_value: &str,
) -> Option<FacetDiagnostic> {
  match support {
    FacetSupport::Configurable => None,
    FacetSupport::Fixed(expected) => {
      if is_value_compatible_with_fixed(facet, expected, configured_value) {
        None
      } else {
        Some(FacetDiagnostic {
          surface: surface_name.to_string(),
          facet,
          support,
          severity: FacetDiagnosticSeverity::Warning,
          message: format!(
            "Tool/surface enforces fixed value '{}' for facet '{}', but '{}' was configured. The fixed value '{}' will be used.",
            expected,
            facet.name(),
            configured_value,
            expected
          ),
        })
      }
    }
    FacetSupport::Unsupported => Some(FacetDiagnostic {
      surface: surface_name.to_string(),
      facet,
      support,
      severity: FacetDiagnosticSeverity::Warning,
      message: format!(
        "Facet '{}' is unsupported by surface '{}'. Setting '{}' will have no effect.",
        facet.name(),
        surface_name,
        configured_value
      ),
    }),
  }
}

/// Validates an iterator of facet key-value pairs configured for a surface.
pub fn validate_facets<I, S>(
  surface: &dyn DeclaresFacets,
  surface_name: &str,
  facets: I,
) -> Vec<FacetDiagnostic>
where
  I: IntoIterator<Item = (Facet, S)>,
  S: AsRef<str>,
{
  let mut diagnostics = Vec::new();
  for (facet, val) in facets {
    let support = surface.facet_support(facet);
    if let Some(diag) =
      validate_facet_value(surface_name, support, facet, val.as_ref())
    {
      diagnostics.push(diag);
    }
  }
  diagnostics
}

/// Validates that a surface consistently reports support for every canonical facet.
pub fn validate_surface_reporting(
  surface: &dyn DeclaresFacets,
  name: &str,
) -> Vec<String> {
  let mut errors = Vec::new();
  for &facet in Facet::ALL {
    let support = surface.facet_support(facet);
    if let FacetSupport::Fixed(val) = support
      && val.is_empty()
    {
      errors.push(format!(
        "Surface '{}' declared Fixed support for facet {:?} with an empty string",
        name, facet
      ));
    }
  }
  errors
}

/// Validates facet reporting consistency across all provided language surfaces.
pub fn validate_all_surfaces_reporting(
  surfaces: &[Box<dyn crate::surfaces::LanguageSurface>],
) -> Result<(), Vec<String>> {
  let mut all_errors = Vec::new();
  for surface in surfaces {
    let errors = validate_surface_reporting(surface.as_ref(), surface.name());
    all_errors.extend(errors);
  }
  if all_errors.is_empty() {
    Ok(())
  } else {
    Err(all_errors)
  }
}

/// Validates a `FormalityConfig` against all active surfaces and produces facet diagnostics.
pub fn validate_config_facets(
  config: &crate::config::FormalityConfig,
  surfaces: &[Box<dyn crate::surfaces::LanguageSurface>],
) -> Vec<FacetDiagnostic> {
  let mut diagnostics = Vec::new();

  for surface in surfaces {
    let s_name = surface.name();

    if let Some(ref global) = config.global {
      if let Some(use_tabs) = global.use_tabs {
        let val_str = if use_tabs { "true" } else { "false" };
        let support = surface.facet_support(Facet::IndentTabs);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::IndentTabs, val_str)
        {
          diagnostics.push(diag);
        }
      }

      if let Some(indent_size) = global.indent_size {
        let val_str = indent_size.to_string();
        let support = surface.facet_support(Facet::IndentWidth);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::IndentWidth, &val_str)
        {
          diagnostics.push(diag);
        }
      }

      if let Some(line_length) = global.line_length {
        let val_str = line_length.to_string();
        let support = surface.facet_support(Facet::LineLength);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::LineLength, &val_str)
        {
          diagnostics.push(diag);
        }
      }
    }

    if let Some(lang_cfg) = config.lang.get(s_name) {
      if let Some(use_tabs) = lang_cfg.use_tabs {
        let val_str = if use_tabs { "true" } else { "false" };
        let support = surface.facet_support(Facet::IndentTabs);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::IndentTabs, val_str)
          && !diagnostics.contains(&diag)
        {
          diagnostics.push(diag);
        }
      }
      if let Some(indent_size) = lang_cfg.indent_size {
        let val_str = indent_size.to_string();
        let support = surface.facet_support(Facet::IndentWidth);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::IndentWidth, &val_str)
          && !diagnostics.contains(&diag)
        {
          diagnostics.push(diag);
        }
      }
      if let Some(line_length) = lang_cfg.line_length {
        let val_str = line_length.to_string();
        let support = surface.facet_support(Facet::LineLength);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::LineLength, &val_str)
          && !diagnostics.contains(&diag)
        {
          diagnostics.push(diag);
        }
      }
      if let Some(ref prose_wrap) = lang_cfg.prose_wrap {
        let support = surface.facet_support(Facet::ProseWrap);
        if let Some(diag) =
          validate_facet_value(s_name, support, Facet::ProseWrap, prose_wrap)
          && !diagnostics.contains(&diag)
        {
          diagnostics.push(diag);
        }
      }
    }
  }

  diagnostics
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::surfaces::all_surfaces;

  #[test]
  fn test_facet_vocabulary_all() {
    assert_eq!(Facet::ALL.len(), 9);

    for facet in Facet::ALL {
      assert!(!facet.name().is_empty());
      assert!(!facet.description().is_empty());
      assert_eq!(facet.to_string(), facet.name());
      assert_eq!(Facet::from_name(facet.name()), Some(*facet));
    }

    assert_eq!(Facet::from_name("use_tabs"), Some(Facet::IndentTabs));
    assert_eq!(Facet::from_name("indent_size"), Some(Facet::IndentWidth));
    assert_eq!(Facet::from_name("max_width"), Some(Facet::LineLength));
    assert_eq!(Facet::from_name("isort"), Some(Facet::ImportSort));
    assert_eq!(Facet::from_name("std"), Some(Facet::Standard));
    assert_eq!(Facet::from_name("nonexistent"), None);
  }

  #[test]
  fn test_facet_support_helpers() {
    let conf = FacetSupport::Configurable;
    assert!(conf.is_configurable());
    assert!(!conf.is_fixed());
    assert!(!conf.is_unsupported());
    assert_eq!(conf.fixed_value(), None);
    assert_eq!(conf.to_string(), "configurable");

    let fixed = FacetSupport::Fixed("spaces");
    assert!(!fixed.is_configurable());
    assert!(fixed.is_fixed());
    assert!(!fixed.is_unsupported());
    assert_eq!(fixed.fixed_value(), Some("spaces"));
    assert_eq!(fixed.to_string(), "fixed(spaces)");

    let unsupp = FacetSupport::Unsupported;
    assert!(!unsupp.is_configurable());
    assert!(!unsupp.is_fixed());
    assert!(unsupp.is_unsupported());
    assert_eq!(unsupp.fixed_value(), None);
    assert_eq!(unsupp.to_string(), "unsupported");
  }

  #[test]
  fn test_all_surfaces_reporting_engine() {
    let surfaces = all_surfaces();
    assert_eq!(surfaces.len(), 8);

    let result = validate_all_surfaces_reporting(&surfaces);
    assert!(result.is_ok(), "Errors: {:?}", result.err());

    for surface in &surfaces {
      let declared = surface.declared_facets();
      assert_eq!(declared.len(), 9);

      for (facet, support) in declared {
        assert_eq!(surface.facet_support(facet), support);
      }
    }
  }

  #[test]
  fn test_surface_facet_declarations() {
    let surfaces = all_surfaces();

    // 1. RustSurface
    let rust = surfaces.iter().find(|s| s.name() == "rust").unwrap();
    assert_eq!(
      rust.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("spaces")
    );
    assert_eq!(
      rust.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      rust.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      rust.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      rust.facet_support(Facet::Edition),
      FacetSupport::Configurable
    );
    assert_eq!(
      rust.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );
    assert_eq!(
      rust.facet_support(Facet::ProseWrap),
      FacetSupport::Unsupported
    );
    assert_eq!(
      rust.facet_support(Facet::TrailingComma),
      FacetSupport::Unsupported
    );
    assert_eq!(
      rust.facet_support(Facet::Standard),
      FacetSupport::Unsupported
    );

    // 2. PythonSurface
    let python = surfaces.iter().find(|s| s.name() == "python").unwrap();
    assert_eq!(
      python.facet_support(Facet::QuoteStyle),
      FacetSupport::Configurable
    );
    assert_eq!(
      python.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      python.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      python.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      python.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      python.facet_support(Facet::ProseWrap),
      FacetSupport::Unsupported
    );
    assert_eq!(
      python.facet_support(Facet::Edition),
      FacetSupport::Unsupported
    );
    assert_eq!(
      python.facet_support(Facet::Standard),
      FacetSupport::Unsupported
    );

    // 3. CppSurface
    let cpp = surfaces.iter().find(|s| s.name() == "cpp").unwrap();
    assert_eq!(
      cpp.facet_support(Facet::Standard),
      FacetSupport::Configurable
    );
    assert_eq!(
      cpp.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      cpp.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      cpp.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      cpp.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      cpp.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );

    // 4. MarkdownSurface
    let md = surfaces.iter().find(|s| s.name() == "markdown").unwrap();
    assert_eq!(
      md.facet_support(Facet::ProseWrap),
      FacetSupport::Configurable
    );
    assert_eq!(
      md.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      md.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      md.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      md.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );

    // 5. YamlSurface
    let yaml = surfaces.iter().find(|s| s.name() == "yaml").unwrap();
    assert_eq!(
      yaml.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("spaces")
    );
    assert_eq!(
      yaml.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      yaml.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      yaml.facet_support(Facet::QuoteStyle),
      FacetSupport::Configurable
    );
    assert_eq!(
      yaml.facet_support(Facet::ProseWrap),
      FacetSupport::Configurable
    );
    assert_eq!(
      yaml.facet_support(Facet::TrailingComma),
      FacetSupport::Unsupported
    );

    // 6. JsonSurface
    let json = surfaces.iter().find(|s| s.name() == "json").unwrap();
    assert_eq!(
      json.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      json.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      json.facet_support(Facet::QuoteStyle),
      FacetSupport::Fixed("double")
    );
    assert_eq!(
      json.facet_support(Facet::TrailingComma),
      FacetSupport::Fixed("none")
    );
    assert_eq!(
      json.facet_support(Facet::ImportSort),
      FacetSupport::Unsupported
    );
    assert_eq!(
      json.facet_support(Facet::ProseWrap),
      FacetSupport::Unsupported
    );

    // 7. TomlSurface
    let toml = surfaces.iter().find(|s| s.name() == "toml").unwrap();
    assert_eq!(
      toml.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      toml.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      toml.facet_support(Facet::IndentTabs),
      FacetSupport::Configurable
    );
    assert_eq!(
      toml.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );

    // 8. TypstSurface
    let typst = surfaces.iter().find(|s| s.name() == "typst").unwrap();
    assert_eq!(
      typst.facet_support(Facet::LineLength),
      FacetSupport::Configurable
    );
    assert_eq!(
      typst.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      typst.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("spaces")
    );
    assert_eq!(
      typst.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );
  }

  #[test]
  fn test_validate_facets_guardrails() {
    let surfaces = all_surfaces();
    let rust = surfaces.iter().find(|s| s.name() == "rust").unwrap();

    // 1. Configurable facet yields no diagnostic
    let diags = validate_facets(
      rust.as_ref(),
      rust.name(),
      vec![(Facet::LineLength, "100"), (Facet::IndentWidth, "4")],
    );
    assert!(diags.is_empty());

    // 2. Fixed facet with matching value yields no diagnostic
    let diags_fixed_ok = validate_facets(
      rust.as_ref(),
      rust.name(),
      vec![(Facet::IndentTabs, "spaces"), (Facet::IndentTabs, "false")],
    );
    assert!(diags_fixed_ok.is_empty());

    // 3. Fixed facet with conflicting value yields warning diagnostic
    let diags_fixed_err = validate_facets(
      rust.as_ref(),
      rust.name(),
      vec![(Facet::IndentTabs, "true"), (Facet::IndentTabs, "tabs")],
    );
    assert_eq!(diags_fixed_err.len(), 2);
    assert_eq!(
      diags_fixed_err[0].severity,
      FacetDiagnosticSeverity::Warning
    );
    assert!(diags_fixed_err[0].message.contains("fixed value 'spaces'"));

    // 4. Unsupported facet yields warning diagnostic
    let diags_unsupported = validate_facets(
      rust.as_ref(),
      rust.name(),
      vec![
        (Facet::QuoteStyle, "single"),
        (Facet::ProseWrap, "always"),
        (Facet::Standard, "c++20"),
      ],
    );
    assert_eq!(diags_unsupported.len(), 3);
    for diag in &diags_unsupported {
      assert_eq!(diag.severity, FacetDiagnosticSeverity::Warning);
      assert!(diag.message.contains("is unsupported by surface 'rust'"));
    }
  }

  #[test]
  fn test_validate_json_fixed_facets() {
    let surfaces = all_surfaces();
    let json = surfaces.iter().find(|s| s.name() == "json").unwrap();

    // Matching double quotes and no trailing commas
    let ok = validate_facets(
      json.as_ref(),
      json.name(),
      vec![
        (Facet::QuoteStyle, "double"),
        (Facet::TrailingComma, "none"),
        (Facet::TrailingComma, "false"),
      ],
    );
    assert!(ok.is_empty());

    // Mismatches
    let mismatches = validate_facets(
      json.as_ref(),
      json.name(),
      vec![
        (Facet::QuoteStyle, "single"),
        (Facet::TrailingComma, "always"),
      ],
    );
    assert_eq!(mismatches.len(), 2);
    assert!(mismatches[0].message.contains("fixed value 'double'"));
    assert!(mismatches[1].message.contains("fixed value 'none'"));
  }

  #[test]
  fn test_declares_facets_trait_helpers() {
    let surfaces = all_surfaces();
    let rust = surfaces.iter().find(|s| s.name() == "rust").unwrap();

    assert!(rust.is_facet_configurable(Facet::LineLength));
    assert!(rust.is_facet_configurable(Facet::IndentWidth));
    assert!(rust.is_facet_configurable(Facet::ImportSort));
    assert!(rust.is_facet_configurable(Facet::Edition));
    assert!(!rust.is_facet_configurable(Facet::QuoteStyle));
    assert!(!rust.is_facet_configurable(Facet::IndentTabs));

    let conf_facets = rust.configurable_facets();
    assert!(conf_facets.contains(&Facet::LineLength));
    assert!(conf_facets.contains(&Facet::IndentWidth));
    assert!(conf_facets.contains(&Facet::ImportSort));
    assert!(conf_facets.contains(&Facet::Edition));
    assert!(!conf_facets.contains(&Facet::QuoteStyle));

    let fixed_facets = rust.fixed_facets();
    assert_eq!(fixed_facets, vec![(Facet::IndentTabs, "spaces")]);

    let unsupp = rust.unsupported_facets();
    assert!(unsupp.contains(&Facet::QuoteStyle));
    assert!(unsupp.contains(&Facet::ProseWrap));
    assert!(unsupp.contains(&Facet::TrailingComma));
    assert!(unsupp.contains(&Facet::Standard));
  }
}
