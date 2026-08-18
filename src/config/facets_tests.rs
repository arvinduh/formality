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
  assert_eq!(surfaces.len(), 10);

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
