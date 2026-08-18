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
      "Surface '{}' missing from all_surfaces()",
      exp
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
      "Failed to resolve surface for query '{}'",
      query
    );
    assert_eq!(
      surface.unwrap().name(),
      canonical,
      "Query '{}' resolved to unexpected surface name",
      query
    );

    // Verify resolve_canonical_name
    assert_eq!(
      resolve_canonical_name(query),
      Some(canonical),
      "resolve_canonical_name failed for '{}'",
      query
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
      "Case-insensitive lookup failed for '{}'",
      query
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
