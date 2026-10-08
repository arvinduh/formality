//! The surface registry: names, aliases, and detection over the fixtures
//! under `tests/fixtures`.

use std::path;

use fml::config;
use fml::surfaces;

#[test]
fn registry_and_aliases() {
  let surfaces = surfaces::registry::all_surfaces();
  assert_eq!(surfaces.len(), 12);

  let registry = surfaces::registry::SurfaceRegistry::default();
  let names: Vec<&str> = registry.surfaces().iter().map(|s| s.name()).collect();
  assert_eq!(
    names,
    vec![
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
    ]
  );

  let cases = [
    ("rust", "rust"),
    ("rs", "rust"),
    ("RS", "rust"),
    ("python", "python"),
    ("py", "python"),
    ("Py", "python"),
    ("cpp", "cpp"),
    ("c", "cpp"),
    ("c++", "cpp"),
    ("C++", "cpp"),
    ("cxx", "cpp"),
    ("CXX", "cpp"),
    ("java", "java"),
    ("JAVA", "java"),
    ("jav", "java"),
    ("Java", "java"),
    ("go", "go"),
    ("GO", "go"),
    ("golang", "go"),
    ("markdown", "markdown"),
    ("md", "markdown"),
    ("MD", "markdown"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("YML", "yaml"),
    ("json", "json"),
    ("JSON", "json"),
    ("toml", "toml"),
    ("TOML", "toml"),
    ("typst", "typst"),
    ("typ", "typst"),
    ("TYP", "typst"),
    ("javascript", "javascript"),
    ("js", "javascript"),
    ("ts", "javascript"),
    ("typescript", "javascript"),
    ("kotlin", "kotlin"),
    ("kt", "kotlin"),
  ];

  for (query, canonical) in cases {
    let surface = surfaces::registry::get_surface_by_name(query);
    assert!(surface.is_some(), "Lookup failed for query '{query}'");
    assert_eq!(surface.unwrap().name(), canonical);
    assert_eq!(
      surfaces::registry::default_registry().resolve_canonical_name(query),
      Some(canonical)
    );

    let reg_surface = registry.get_surface_by_name(query);
    assert!(reg_surface.is_some());
    assert_eq!(reg_surface.unwrap().name(), canonical);
  }

  assert!(surfaces::registry::get_surface_by_name("nonexistent").is_none());
  assert!(
    surfaces::registry::default_registry()
      .resolve_canonical_name("nonexistent")
      .is_none()
  );
}

#[test]
fn detection_in_fixtures() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let config = config::FormalityConfig::with_defaults();

  // Rust fixture
  let rust_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/rust_repo"),
    &config,
  );
  let rust_names: Vec<&str> = rust_detected.iter().map(|s| s.name()).collect();
  assert!(rust_names.contains(&"rust"));

  // Python fixture
  let py_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/python_repo"),
    &config,
  );
  let py_names: Vec<&str> = py_detected.iter().map(|s| s.name()).collect();
  assert!(py_names.contains(&"python"));

  // C++ fixture
  let cpp_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/cpp_repo"),
    &config,
  );
  let cpp_names: Vec<&str> = cpp_detected.iter().map(|s| s.name()).collect();
  assert!(cpp_names.contains(&"cpp"));

  // Typst fixture
  let typ_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/typst_repo"),
    &config,
  );
  let typ_names: Vec<&str> = typ_detected.iter().map(|s| s.name()).collect();
  assert!(typ_names.contains(&"typst"));

  // Java fixture
  let java_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/java_repo"),
    &config,
  );
  let java_names: Vec<&str> = java_detected.iter().map(|s| s.name()).collect();
  assert!(java_names.contains(&"java"));

  // Go fixture
  let go_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/go_repo"),
    &config,
  );
  let go_names: Vec<&str> = go_detected.iter().map(|s| s.name()).collect();
  assert!(go_names.contains(&"go"));

  // Kotlin fixture
  let kotlin_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/kotlin_repo"),
    &config,
  );
  let kotlin_names: Vec<&str> =
    kotlin_detected.iter().map(|s| s.name()).collect();
  assert!(kotlin_names.contains(&"kotlin"));

  // JavaScript fixture
  let js_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/javascript_repo"),
    &config,
  );
  let js_names: Vec<&str> = js_detected.iter().map(|s| s.name()).collect();
  assert!(js_names.contains(&"javascript"));

  // TOML fixture
  let toml_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/toml_repo"),
    &config,
  );
  let toml_names: Vec<&str> = toml_detected.iter().map(|s| s.name()).collect();
  assert!(toml_names.contains(&"toml"));

  // Polyglot fixture
  let poly_detected = surfaces::registry::detect_surfaces_smart(
    &manifest_dir.join("tests/fixtures/polyglot_repo"),
    &config,
  );
  let poly_names: Vec<&str> = poly_detected.iter().map(|s| s.name()).collect();
  assert!(poly_names.contains(&"rust"));
  assert!(poly_names.contains(&"python"));
  assert!(poly_names.contains(&"markdown"));
  assert!(poly_names.contains(&"yaml"));
  assert!(poly_names.contains(&"json"));
  assert!(poly_names.contains(&"typst"));
}
