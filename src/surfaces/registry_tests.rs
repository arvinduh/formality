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
