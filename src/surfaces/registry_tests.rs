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

  let fleet_order = crate::editorconfig::CANONICAL_FLEET_ORDER;
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

  let child_lsp_surfaces: Vec<&str> = crate::commands::lsp::CHILD_LSP_REGISTRY
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
