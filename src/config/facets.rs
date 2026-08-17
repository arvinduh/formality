use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Common layout facets configuring formatting layout across tools.
#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
pub struct LayoutFacet {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
}

impl LayoutFacet {
  pub fn merge(&mut self, other: LayoutFacet) {
    if other.indent_size.is_some() {
      self.indent_size = other.indent_size;
    }
    if other.line_length.is_some() {
      self.line_length = other.line_length;
    }
    if other.use_tabs.is_some() {
      self.use_tabs = other.use_tabs;
    }
    if other.prose_wrap.is_some() {
      self.prose_wrap = other.prose_wrap;
    }
  }

  pub fn is_empty(&self) -> bool {
    self.indent_size.is_none()
      && self.line_length.is_none()
      && self.use_tabs.is_none()
      && self.prose_wrap.is_none()
  }
}

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
#[path = "facets_tests.rs"]
mod tests;
