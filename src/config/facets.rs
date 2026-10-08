//! Cross-language layout facet definitions.
//!
//! Defines the shared formatting facet vocabulary (`Facet`, `FacetSupport`).
//! Language surface implementations honoring these facets live in `crate::surfaces`.

use schemars;
use serde;

/// Common layout facets configuring formatting layout across tools.
#[derive(
  Debug,
  serde::Serialize,
  serde::Deserialize,
  schemars::JsonSchema,
  Clone,
  PartialEq,
  Default,
)]
#[serde(deny_unknown_fields)]
pub struct LayoutFacet {
  /// Indentation size in spaces.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub indent_size: Option<usize>,
  /// Maximum line length limit.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_length: Option<usize>,
  /// Whether to use tabs for indentation.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub use_tabs: Option<bool>,
  /// Prose wrapping strategy string.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub prose_wrap: Option<String>,
}

impl LayoutFacet {
  /// Merges values from `other` into `self`.
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
}

/// Canonical vocabulary of formatting & linting facets across all language surfaces.
#[derive(Debug, PartialEq, Clone, Copy)]
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

/// Level of support a surface provides for a given facet.
#[derive(Debug, PartialEq)]
pub enum FacetSupport {
  /// The user can freely configure this facet for the surface.
  Configurable,
  /// The tool/language enforces a single fixed value.
  Fixed(&'static str),
  /// The concept does not exist or cannot be configured for this tool/language.
  Unsupported,
}

/// Trait implemented by language surfaces to declare their facet capabilities.
pub trait DeclaresFacets {
  /// Queries the support state for a given canonical facet.
  fn facet_support(&self, facet: Facet) -> FacetSupport;
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::surfaces;

  /// Golden-value coverage for every cell of the facet rosetta table in
  /// `docs/facet-rosetta.md`: all 12 language surfaces x all 9 canonical
  /// facets. This is the audit fixture for issue #100 [pre-recreation] — the previous version
  /// of this test spot-checked only 8 of the 12 surfaces and only a handful of
  /// facets per surface, silently trusting the `Unsupported` default arms for
  /// everything else. Every `(surface, facet)` cell below is asserted
  /// explicitly against the documented table so a change to any surface's
  /// `facet_support` (or the table drifting out of sync with the code) shows
  /// up as a failing assertion instead of an untested edge.
  #[expect(
    clippy::too_many_lines,
    reason = "exhaustive golden-vector table for all 12 surfaces across 9 facets"
  )]
  #[test]
  fn surface_facet_declarations() {
    let surfaces = surfaces::registry::all_surfaces();
    let get = |name: &str| {
      surfaces
        .iter()
        .find(|s| s.name() == name)
        .unwrap_or_else(|| panic!("surface '{name}' not registered"))
    };

    let golden: &[(&str, [FacetSupport; 9])] = &[
      (
        "rust",
        [
          FacetSupport::Fixed("spaces"),
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "python",
        [
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "cpp",
        [
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
        ],
      ),
      (
        "java",
        [
          FacetSupport::Fixed("spaces"),
          FacetSupport::Configurable,
          FacetSupport::Fixed("100"),
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
        ],
      ),
      (
        "go",
        [
          FacetSupport::Fixed("tab"),
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "markdown",
        [
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "yaml",
        [
          FacetSupport::Fixed("spaces"),
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "json",
        [
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Fixed("double"),
          FacetSupport::Fixed("none"),
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "toml",
        [
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "typst",
        [
          FacetSupport::Fixed("spaces"),
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "javascript",
        [
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
      (
        "kotlin",
        [
          FacetSupport::Fixed("spaces"),
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Fixed("double"),
          FacetSupport::Configurable,
          FacetSupport::Configurable,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
          FacetSupport::Unsupported,
        ],
      ),
    ];

    let facet_order = [
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

    assert_eq!(
      golden.len(),
      12,
      "golden table must cover all 12 language surfaces"
    );

    for (surface_name, expected_row) in golden {
      let surface = get(surface_name);
      for (facet, expected) in facet_order.iter().zip(expected_row.iter()) {
        assert_eq!(
          surface.facet_support(*facet),
          *expected,
          "surface '{surface_name}' facet {facet:?}: expected {expected:?} \
           per docs/facet-rosetta.md, got {:?}",
          surface.facet_support(*facet)
        );
      }
    }
  }
}
